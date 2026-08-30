use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Notify;

use crate::config::OverflowPolicy;

/// Whether an item may be discarded to make room for another.
///
/// The queue drops so that the socket never has to wait, but not everything in
/// it costs the same to lose. A live event Slack will still have in its own
/// pipeline is a different thing from one that was fetched precisely because
/// Slack will never send it again.
pub trait Evictable {
    fn may_evict(&self) -> bool;
}

/// How long a producer waiting on capacity sleeps before looking again. Short
/// enough to be invisible, long enough that a stalled pipeline costs nothing.
const DRAIN_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// The seam between receiving an event and doing anything with it.
///
/// Slack expects an envelope to be acknowledged promptly, and stops delivering
/// to an app that falls below its response threshold. So the receiving side
/// must never wait on the processing side: this queue is bounded and *drops*,
/// and the drop is counted rather than hidden. With no event log behind it,
/// loss is a policy the operator chose, and a policy has to be visible.
pub struct EventQueue<T> {
    inner: Mutex<VecDeque<T>>,
    notify: Notify,
    /// Signalled as items leave, for a producer waiting on capacity.
    drained: Notify,
    capacity: usize,
    policy: OverflowPolicy,
    dropped: AtomicU64,
    closed: AtomicBool,
}

impl<T: Evictable> EventQueue<T> {
    pub fn new(capacity: usize, policy: OverflowPolicy) -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            notify: Notify::new(),
            drained: Notify::new(),
            capacity: capacity.max(1),
            policy,
            dropped: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// Never blocks and never fails. Returns whether something had to be
    /// discarded to make room.
    ///
    /// A closed queue takes nothing: the consumer has gone, so anything added
    /// after that would sit there unread and hold memory until the process
    /// ended — and a recovery pass still finishing during shutdown is exactly
    /// the caller that would do it.
    pub fn push(&self, item: T) -> bool {
        if self.closed.load(Ordering::Acquire) {
            return false;
        }

        let mut discarded = false;
        {
            let mut queue = self.lock();
            if queue.len() >= self.capacity {
                discarded = true;
                self.dropped.fetch_add(1, Ordering::Relaxed);
                match self.policy {
                    // The oldest *evictable* item, not simply the oldest. A
                    // recovery pass fills this queue from the front with events
                    // Slack will not send again, and whose channel cursor has
                    // not passed them yet — evicting one loses it for good,
                    // while the newer events behind it move the cursor past the
                    // hole. So the eviction walks past them, and if the whole
                    // queue is unevictable the incoming item goes instead.
                    OverflowPolicy::DropOldest => {
                        if let Some(index) = queue.iter().position(|queued| queued.may_evict()) {
                            queue.remove(index);
                            queue.push_back(item);
                        }
                    }
                    OverflowPolicy::DropNewest => {}
                }
            } else {
                queue.push_back(item);
            }
        }
        self.notify.notify_one();
        discarded
    }

    /// Waits for the next item, or `None` once the queue is closed and drained.
    pub async fn pop(&self) -> Option<T> {
        loop {
            // Registered before the queue is inspected: a push landing between
            // the check and the await stores a permit that this consumes,
            // rather than a wake-up that arrives before anyone is listening.
            let waiting = self.notify.notified();

            if let Some(item) = self.lock().pop_front() {
                self.drained.notify_waiters();
                return Some(item);
            }
            if self.closed.load(Ordering::Acquire) {
                return None;
            }

            waiting.await;
        }
    }

    /// Ends the stream once what is queued has been drained.
    ///
    /// Both notifications are needed. `notify_waiters` wakes a consumer that
    /// is already parked; `notify_one` stores a permit for one that has
    /// created its `Notified` but not yet polled it, which `notify_waiters`
    /// would sail straight past — and that consumer would then wait for a
    /// wake-up that can never come again. One consumer drains this queue, so
    /// one stored permit is enough.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.notify.notify_waiters();
        self.notify.notify_one();
        // A producer waiting on capacity must not wait for a consumer that has
        // gone; it checks `closed` and gives up.
        self.drained.notify_waiters();
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Waits until fewer than `room` items are queued.
    ///
    /// For producers that are *allowed* to wait. The socket task is not one of
    /// them — it has already acknowledged what it is handing over, and making
    /// it wait would turn a slow sink into redelivery. Recovery is: it reads
    /// from a rationed endpoint at its own pace and nothing is acknowledged on
    /// its behalf, so it can hold back rather than overflow the queue and lose
    /// the oldest of what it just went to the trouble of fetching.
    ///
    /// Polls on a short timeout rather than relying on a wake-up alone, so a
    /// pop that lands between the check and the wait cannot strand it.
    pub async fn wait_for_room(&self, room: usize) {
        while !self.closed.load(Ordering::Acquire) && self.len() >= room.max(1) {
            let _ = tokio::time::timeout(DRAIN_POLL, self.drained.notified()).await;
        }
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A poisoned lock means a panic while holding it. Nothing here can
    /// panic — the guarded section only moves items — so recovering keeps a
    /// long-running daemon alive rather than cascading someone else's failure.
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<T>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The queue's own behaviour, tested on a value that is always evictable —
    /// the protection of un-evictable items has its own test below.
    impl Evictable for u8 {
        fn may_evict(&self) -> bool {
            true
        }
    }

    impl Evictable for i32 {
        fn may_evict(&self) -> bool {
            true
        }
    }
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn items_come_back_in_order_until_the_bound() {
        let queue = EventQueue::new(4, OverflowPolicy::DropOldest);
        for item in 0..3 {
            assert!(!queue.push(item));
        }
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn dropping_the_oldest_keeps_the_newest_view() {
        let queue = EventQueue::new(2, OverflowPolicy::DropOldest);
        queue.push(1);
        queue.push(2);
        assert!(queue.push(3));

        assert_eq!(queue.dropped(), 1);
        let mut drained = Vec::new();
        while let Some(item) = queue.lock().pop_front() {
            drained.push(item);
        }
        assert_eq!(drained, vec![2, 3]);
    }

    #[test]
    fn dropping_the_newest_preserves_arrival_order() {
        let queue = EventQueue::new(2, OverflowPolicy::DropNewest);
        queue.push(1);
        queue.push(2);
        assert!(queue.push(3));

        let mut drained = Vec::new();
        while let Some(item) = queue.lock().pop_front() {
            drained.push(item);
        }
        assert_eq!(drained, vec![1, 2]);
        assert_eq!(queue.dropped(), 1);
    }

    /// The item the queue must not choose: a recovered event exists because
    /// Slack will not send it again, and the newer events queued behind it
    /// will move its channel's cursor past the gap losing it would leave.
    #[test]
    fn an_unevictable_item_is_never_the_one_discarded() {
        #[derive(Debug, PartialEq, Eq)]
        struct Item {
            id: u8,
            recovered: bool,
        }
        impl Evictable for Item {
            fn may_evict(&self) -> bool {
                !self.recovered
            }
        }

        let queue = EventQueue::new(3, OverflowPolicy::DropOldest);
        queue.push(Item {
            id: 1,
            recovered: true,
        });
        queue.push(Item {
            id: 2,
            recovered: false,
        });
        queue.push(Item {
            id: 3,
            recovered: true,
        });

        // Full. The oldest is recovered, so the eviction walks past it.
        assert!(queue.push(Item {
            id: 4,
            recovered: false
        }));

        let mut left = Vec::new();
        while let Some(item) = queue.lock().pop_front() {
            left.push(item.id);
        }
        assert_eq!(
            left,
            vec![1, 3, 4],
            "the live item should have gone, not a recovered one"
        );
    }

    /// When nothing may be evicted the incoming item goes instead, rather than
    /// the queue discarding something it promised to keep.
    #[test]
    fn a_queue_of_unevictable_items_refuses_the_newcomer() {
        struct Kept;
        impl Evictable for Kept {
            fn may_evict(&self) -> bool {
                false
            }
        }

        let queue = EventQueue::new(2, OverflowPolicy::DropOldest);
        queue.push(Kept);
        queue.push(Kept);
        assert!(queue.push(Kept));
        assert_eq!(queue.len(), 2);
    }

    #[tokio::test]
    async fn a_waiting_consumer_is_woken_by_a_push() {
        let queue = Arc::new(EventQueue::new(8, OverflowPolicy::DropOldest));
        let consumer = queue.clone();
        let task = tokio::spawn(async move { consumer.pop().await });

        tokio::time::sleep(Duration::from_millis(20)).await;
        queue.push(7);

        assert_eq!(task.await.unwrap(), Some(7));
    }

    /// The receiving side must never be made to wait, so a full queue has to
    /// return immediately even with nobody draining it.
    #[tokio::test]
    async fn pushing_into_a_full_queue_does_not_block() {
        let queue = EventQueue::new(1, OverflowPolicy::DropOldest);
        let pushes = tokio::time::timeout(Duration::from_millis(200), async {
            for item in 0..1000 {
                queue.push(item);
            }
        })
        .await;
        assert!(
            pushes.is_ok(),
            "a bounded push must not wait for a consumer"
        );
        assert_eq!(queue.dropped(), 999);
    }

    /// A task still running during shutdown must not be able to grow the
    /// queue behind the consumer that has already left.
    #[test]
    fn a_closed_queue_accepts_nothing_more() {
        let queue = EventQueue::new(8, OverflowPolicy::DropOldest);
        queue.push(1);
        queue.close();
        queue.push(2);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue.dropped(), 0, "a refused push is not an overflow");
    }

    #[tokio::test]
    async fn closing_drains_what_is_left_and_then_ends() {
        let queue = Arc::new(EventQueue::new(4, OverflowPolicy::DropOldest));
        queue.push(1);
        queue.push(2);
        queue.close();

        assert_eq!(queue.pop().await, Some(1));
        assert_eq!(queue.pop().await, Some(2));
        assert_eq!(queue.pop().await, None);
    }

    /// The race `notify_waiters` alone would lose: the consumer has created
    /// its `Notified` but not polled it when `close` runs, so there is no
    /// registered waiter to wake and the permit is the only thing that saves
    /// it. Run many times because the window is small.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closing_races_a_consumer_that_has_not_parked_yet() {
        for _ in 0..200 {
            let queue = Arc::new(EventQueue::<u8>::new(4, OverflowPolicy::DropOldest));
            let consumer = queue.clone();
            let task = tokio::spawn(async move { consumer.pop().await });
            queue.close();

            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), task)
                    .await
                    .expect("close must never strand a consumer")
                    .unwrap(),
                None
            );
        }
    }

    #[tokio::test]
    async fn a_consumer_blocked_on_an_empty_queue_ends_when_it_closes() {
        let queue = Arc::new(EventQueue::<u8>::new(4, OverflowPolicy::DropOldest));
        let consumer = queue.clone();
        let task = tokio::spawn(async move { consumer.pop().await });

        tokio::time::sleep(Duration::from_millis(20)).await;
        queue.close();

        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), task)
                .await
                .expect("close must wake a blocked consumer")
                .unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod backpressure_tests {
    // The `Evictable` impls for the plain values used here live in the module
    // above; a trait impl is visible wherever its trait and type are.
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    /// A producer that is allowed to wait should wait, not overflow. Recovery
    /// is that producer: under the default policy an overflow discards the
    /// *oldest* queued events, which during a catch-up are exactly the ones
    /// the cursor has not passed — so overflowing turns a gap just fetched
    /// into a permanent hole.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_patient_producer_waits_instead_of_dropping() {
        let queue = Arc::new(EventQueue::new(4, OverflowPolicy::DropOldest));

        let producer = {
            let queue = queue.clone();
            tokio::spawn(async move {
                for item in 0..20 {
                    queue.wait_for_room(2).await;
                    queue.push(item);
                }
            })
        };

        let mut drained = Vec::new();
        while drained.len() < 20 {
            match tokio::time::timeout(Duration::from_secs(5), queue.pop()).await {
                Ok(Some(item)) => drained.push(item),
                _ => break,
            }
        }
        producer.await.unwrap();

        assert_eq!(drained, (0..20).collect::<Vec<_>>());
        assert_eq!(queue.dropped(), 0, "a producer that can wait must not drop");
    }

    /// It must give up rather than wait for a consumer that has gone.
    #[tokio::test]
    async fn waiting_for_room_ends_when_the_queue_closes() {
        let queue = Arc::new(EventQueue::new(2, OverflowPolicy::DropOldest));
        queue.push(1);
        queue.push(2);

        let waiter = {
            let queue = queue.clone();
            tokio::spawn(async move { queue.wait_for_room(1).await })
        };

        tokio::time::sleep(Duration::from_millis(20)).await;
        queue.close();

        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("close must release a producer waiting on capacity")
            .unwrap();
    }
}
