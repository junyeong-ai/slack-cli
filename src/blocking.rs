use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinError;

#[derive(Clone, Debug)]
pub(crate) struct BlockingExecutor {
    permits: Arc<Semaphore>,
    capacity: u32,
}

impl BlockingExecutor {
    pub(crate) fn new(capacity: u32) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(capacity as usize)),
            capacity,
        }
    }

    pub(crate) async fn drain(&self) {
        let _permits = self
            .permits
            .acquire_many(self.capacity)
            .await
            .expect("database executor is never closed");
    }

    pub(crate) async fn execute<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<JoinError> + Send + 'static,
        F: FnOnce() -> Result<T, E> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .expect("database executor is never closed");
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation()
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_keeps_capacity_reserved_until_the_worker_finishes() {
        let executor = BlockingExecutor::new(1);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let worker = executor.clone();
        let caller = tokio::spawn(async move {
            worker
                .execute(move || -> anyhow::Result<()> {
                    started_tx.send(()).unwrap();
                    finish_rx.recv()?;
                    Ok(())
                })
                .await
        });
        started_rx.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert_eq!(executor.permits.available_permits(), 0);
        finish_tx.send(()).unwrap();
        executor.drain().await;
        executor
            .execute(|| Ok::<_, anyhow::Error>(()))
            .await
            .unwrap();
        assert_eq!(executor.permits.available_permits(), 1);
    }
}
