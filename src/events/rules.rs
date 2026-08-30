use anyhow::Result;

use super::envelope::{Event, EventKind};
use super::state::EventState;
use crate::config::RuleConfig;

/// What the rules made of one event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Rules that matched, in declaration order.
    pub rules: Vec<String>,
    /// Sinks to deliver to — the union over the matching rules, with a rule
    /// naming no sink meaning every sink.
    pub sinks: Vec<String>,
    pub subscribed: usize,
    pub unsubscribed: usize,
}

impl Outcome {
    pub fn matched(&self) -> bool {
        !self.rules.is_empty()
    }
}

/// The declarative rules, compiled once, evaluated against every event.
///
/// The engine is stateful by design: a reaction does not match anything by
/// itself, it *subscribes* a thread, and the replies that arrive later are
/// what match. Keeping that state here rather than in the caller is what makes
/// an emoji usable as a subscribe button.
pub struct RuleEngine {
    rules: Vec<CompiledRule>,
    all_sinks: Vec<String>,
    me: Option<String>,
    mention: Option<String>,
}

struct CompiledRule {
    config: RuleConfig,
    keywords: Vec<String>,
}

impl RuleEngine {
    /// `me` is the authenticated user id from `auth.test`. Without it a
    /// mention cannot be recognised and a reaction cannot be attributed, so
    /// the rules that depend on it simply never fire.
    pub fn new(rules: Vec<RuleConfig>, all_sinks: Vec<String>, me: Option<String>) -> Self {
        Self {
            rules: rules
                .into_iter()
                .map(|config| CompiledRule {
                    keywords: config.keywords.iter().map(|k| k.to_lowercase()).collect(),
                    config,
                })
                .collect(),
            all_sinks,
            mention: me.as_ref().map(|id| format!("<@{id}")),
            me,
        }
    }

    /// Channels a rule names outright. Marked recoverable at startup so a
    /// disconnect can be repaired for them before they have ever matched.
    pub fn declared_channels(&self) -> Vec<&str> {
        let mut channels: Vec<&str> = self
            .rules
            .iter()
            .flat_map(|rule| rule.config.channels.iter().map(String::as_str))
            .collect();
        channels.sort_unstable();
        channels.dedup();
        channels
    }

    pub fn rule_names(&self) -> Vec<&str> {
        self.rules
            .iter()
            .map(|rule| rule.config.name.as_str())
            .collect()
    }

    pub fn evaluate(&self, event: &Event, state: &EventState) -> Result<Outcome> {
        let mut outcome = Outcome::default();

        for rule in &self.rules {
            if !rule
                .config
                .on
                .iter()
                .any(|configured| event.kind.matches_config(*configured))
            {
                continue;
            }

            let Some(channel) = event.channel.as_deref() else {
                continue;
            };
            if !rule.config.channels.is_empty()
                && !rule.config.channels.iter().any(|id| id == channel)
            {
                continue;
            }

            // Subscription first: a reaction changes what later messages mean,
            // and it does so whether or not anything matches right now.
            if let Some(emoji) = rule.config.subscribe_emoji.as_deref() {
                match self.apply_subscription(rule, emoji, event, channel, state)? {
                    Subscription::Added => {
                        outcome.subscribed += 1;
                        continue;
                    }
                    Subscription::Removed => {
                        outcome.unsubscribed += 1;
                        continue;
                    }
                    Subscription::Unchanged => {}
                }
            }

            if self.rule_matches(rule, event, channel, state)? {
                outcome.rules.push(rule.config.name.clone());
                if rule.config.sinks.is_empty() {
                    outcome.sinks.extend(self.all_sinks.iter().cloned());
                } else {
                    outcome.sinks.extend(rule.config.sinks.iter().cloned());
                }
            }
        }

        outcome.sinks.sort_unstable();
        outcome.sinks.dedup();
        Ok(outcome)
    }

    /// Adds or removes a thread subscription.
    ///
    /// Only the authenticated user's own reaction counts. The gesture means
    /// "I am following this", so a colleague placing the same emoji must not
    /// enrol someone else's assistant.
    fn apply_subscription(
        &self,
        rule: &CompiledRule,
        emoji: &str,
        event: &Event,
        channel: &str,
        state: &EventState,
    ) -> Result<Subscription> {
        let is_reaction = matches!(
            event.kind,
            EventKind::ReactionAdded | EventKind::ReactionRemoved
        );
        if !is_reaction
            || event.reaction.as_deref() != Some(emoji)
            || event.user.is_none()
            || event.user != self.me
        {
            return Ok(Subscription::Unchanged);
        }

        // The reaction names the message it sits on, and a thread is rooted at
        // its first message — so reacting to that message is what subscribes
        // the thread. A reaction placed on a reply names only the reply.
        let Some(thread_ts) = event.ts.as_deref() else {
            return Ok(Subscription::Unchanged);
        };

        match event.kind {
            EventKind::ReactionAdded => {
                // From the reaction, not from the top of the thread: the live
                // path only matches replies that arrive after the subscription,
                // so recovery must start in the same place or a reconnect
                // would deliver everything said before it.
                state.watch_thread(
                    channel,
                    thread_ts,
                    &rule.config.name,
                    emoji,
                    self.me.as_deref(),
                    event.event_ts.as_deref(),
                )?;
                Ok(Subscription::Added)
            }
            EventKind::ReactionRemoved => {
                state.unwatch_thread(channel, thread_ts, &rule.config.name)?;
                Ok(Subscription::Removed)
            }
            EventKind::Message => Ok(Subscription::Unchanged),
        }
    }

    /// Whether the message text mentions the authenticated user.
    ///
    /// Slack writes a mention as `<@U123>`, but an older client or a link with
    /// a label writes `<@U123|alice>`, so matching the closing bracket alone
    /// would silently miss it. The prefix is matched and the character after
    /// it is required to end the reference, which also stops `<@U12>` from
    /// matching a longer id that merely starts the same way.
    fn names_me(&self, text: &str) -> bool {
        let Some(prefix) = self.mention.as_deref() else {
            return false;
        };
        text.match_indices(prefix).any(|(at, _)| {
            matches!(
                text[at + prefix.len()..].chars().next(),
                Some('>') | Some('|')
            )
        })
    }

    fn rule_matches(
        &self,
        rule: &CompiledRule,
        event: &Event,
        channel: &str,
        state: &EventState,
    ) -> Result<bool> {
        if event.kind == EventKind::Message
            && !rule.config.include_own_messages
            && event.user.is_some()
            && event.user == self.me
        {
            return Ok(false);
        }

        let text = event.text.as_deref().unwrap_or_default().to_lowercase();

        let mentioned = rule.config.mentions_me
            && event
                .text
                .as_deref()
                .is_some_and(|body| self.names_me(body));

        let keyword_hit =
            !rule.keywords.is_empty() && rule.keywords.iter().any(|needle| text.contains(needle));

        let from_hit = !rule.config.from_users.is_empty()
            && event
                .user
                .as_deref()
                .is_some_and(|author| rule.config.from_users.iter().any(|id| id == author));

        let in_watched_thread = rule.config.subscribe_emoji.is_some()
            && event.kind == EventKind::Message
            && match event.thread_root() {
                Some(root) => state.is_watched(channel, root, &rule.config.name)?,
                None => false,
            };

        // A rule that states no predicate is a channel filter, and the filter
        // has already passed by the time evaluation reaches here.
        let has_predicate = rule.config.mentions_me
            || !rule.keywords.is_empty()
            || !rule.config.from_users.is_empty()
            || rule.config.subscribe_emoji.is_some();

        Ok(if has_predicate {
            mentioned || keyword_hit || from_hit || in_watched_thread
        } else {
            true
        })
    }
}

enum Subscription {
    Added,
    Removed,
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EventKindConfig;
    use crate::events::envelope::{EVENT_SCHEMA, EventSource};
    use chrono::Utc;

    const ME: &str = "U_ME";

    fn state() -> (EventState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = EventState::open(&dir.path().join("state.db")).unwrap();
        (state, dir)
    }

    fn rule(name: &str) -> RuleConfig {
        RuleConfig {
            name: name.to_string(),
            on: vec![EventKindConfig::Message],
            mentions_me: false,
            keywords: Vec::new(),
            from_users: Vec::new(),
            channels: Vec::new(),
            subscribe_emoji: None,
            include_own_messages: false,
            sinks: Vec::new(),
        }
    }

    fn base(kind: EventKind) -> Event {
        Event {
            schema: EVENT_SCHEMA.to_string(),
            id: "Ev1".into(),
            seq: 0,
            kind,
            source: EventSource::Socket,
            team_id: None,
            channel: Some("C0000001".into()),
            channel_type: Some("channel".into()),
            user: Some("U_OTHER".into()),
            bot_id: None,
            ts: Some("1700000000.000100".into()),
            event_ts: None,
            thread_ts: None,
            subtype: None,
            text: None,
            reaction: None,
            item_user: None,
            received_at: Utc::now(),
            matched: Vec::new(),
            raw: None,
        }
    }

    fn message(text: &str) -> Event {
        Event {
            text: Some(text.to_string()),
            ..base(EventKind::Message)
        }
    }

    fn engine(rules: Vec<RuleConfig>) -> RuleEngine {
        RuleEngine::new(rules, vec!["stdout".into()], Some(ME.to_string()))
    }

    #[test]
    fn a_mention_rule_matches_only_a_message_naming_me() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            mentions_me: true,
            ..rule("mention")
        }]);

        let hit = engine
            .evaluate(&message("hey <@U_ME> look"), &state)
            .unwrap();
        assert_eq!(hit.rules, vec!["mention".to_string()]);
        assert_eq!(hit.sinks, vec!["stdout".to_string()]);

        let miss = engine
            .evaluate(&message("hey <@U_SOMEONE>"), &state)
            .unwrap();
        assert!(!miss.matched());
    }

    /// An assistant that answers its own messages is a loop, so the author
    /// filter is the default and opting out of it is explicit.
    #[test]
    fn my_own_messages_do_not_match_unless_asked_for() {
        let (state, _dir) = state();
        let mine = Event {
            user: Some(ME.into()),
            ..message("note to self <@U_ME>")
        };

        let guarded = engine(vec![RuleConfig {
            mentions_me: true,
            ..rule("mention")
        }]);
        assert!(!guarded.evaluate(&mine, &state).unwrap().matched());

        let opted_in = engine(vec![RuleConfig {
            mentions_me: true,
            include_own_messages: true,
            ..rule("mention")
        }]);
        assert!(opted_in.evaluate(&mine, &state).unwrap().matched());
    }

    /// An older client, or a mention carrying a label, writes `<@U123|alice>`.
    /// Matching only `<@U123>` would drop those on the floor.
    #[test]
    fn a_labelled_mention_still_names_me() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            mentions_me: true,
            ..rule("mention")
        }]);

        assert!(
            engine
                .evaluate(&message("hey <@U_ME|tester> look"), &state)
                .unwrap()
                .matched()
        );
    }

    /// `<@U_ME>` must not be found inside `<@U_MERGED>`: the reference has to
    /// end where the id does.
    #[test]
    fn a_longer_id_that_starts_the_same_is_not_me() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            mentions_me: true,
            ..rule("mention")
        }]);

        assert!(
            !engine
                .evaluate(&message("hey <@U_MERGED> look"), &state)
                .unwrap()
                .matched()
        );
    }

    #[test]
    fn keywords_are_matched_without_regard_to_case() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            keywords: vec!["Deploy".into()],
            ..rule("deploys")
        }]);

        assert!(
            engine
                .evaluate(&message("DEPLOYING now"), &state)
                .unwrap()
                .matched()
        );
        assert!(
            !engine
                .evaluate(&message("nothing here"), &state)
                .unwrap()
                .matched()
        );
    }

    #[test]
    fn a_channel_allowlist_narrows_every_other_condition() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            mentions_me: true,
            channels: vec!["C0000002".into()],
            ..rule("mention")
        }]);

        assert!(
            !engine
                .evaluate(&message("<@U_ME>"), &state)
                .unwrap()
                .matched()
        );

        let elsewhere = Event {
            channel: Some("C0000002".into()),
            ..message("<@U_ME>")
        };
        assert!(engine.evaluate(&elsewhere, &state).unwrap().matched());
    }

    #[test]
    fn a_rule_with_only_a_channel_forwards_that_channel() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            channels: vec!["C0000001".into()],
            ..rule("ops")
        }]);
        assert!(
            engine
                .evaluate(&message("anything"), &state)
                .unwrap()
                .matched()
        );
    }

    /// The whole point of the reaction rule: the emoji subscribes, and the
    /// replies that come afterwards are what reach the agent.
    #[test]
    fn reacting_subscribes_the_thread_and_later_replies_match() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            on: vec![
                EventKindConfig::Message,
                EventKindConfig::ReactionAdded,
                EventKindConfig::ReactionRemoved,
            ],
            subscribe_emoji: Some("eyes".into()),
            ..rule("watched")
        }]);

        let reply = Event {
            thread_ts: Some("1700000000.000100".into()),
            ts: Some("1700000050.000200".into()),
            ..message("a reply")
        };
        assert!(!engine.evaluate(&reply, &state).unwrap().matched());

        let reaction = Event {
            user: Some(ME.into()),
            reaction: Some("eyes".into()),
            ..base(EventKind::ReactionAdded)
        };
        let subscribing = engine.evaluate(&reaction, &state).unwrap();
        assert_eq!(subscribing.subscribed, 1);
        // Subscribing is not itself something to forward.
        assert!(!subscribing.matched());

        assert!(engine.evaluate(&reply, &state).unwrap().matched());

        let removal = Event {
            kind: EventKind::ReactionRemoved,
            ..reaction
        };
        assert_eq!(engine.evaluate(&removal, &state).unwrap().unsubscribed, 1);
        assert!(!engine.evaluate(&reply, &state).unwrap().matched());
    }

    /// The gesture means "I am following this". A colleague's identical emoji
    /// must not enrol someone else's assistant.
    #[test]
    fn someone_elses_reaction_subscribes_nothing() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            on: vec![EventKindConfig::Message, EventKindConfig::ReactionAdded],
            subscribe_emoji: Some("eyes".into()),
            ..rule("watched")
        }]);

        let theirs = Event {
            user: Some("U_OTHER".into()),
            reaction: Some("eyes".into()),
            ..base(EventKind::ReactionAdded)
        };
        assert_eq!(engine.evaluate(&theirs, &state).unwrap().subscribed, 0);
    }

    #[test]
    fn a_different_emoji_subscribes_nothing() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            on: vec![EventKindConfig::Message, EventKindConfig::ReactionAdded],
            subscribe_emoji: Some("eyes".into()),
            ..rule("watched")
        }]);

        let other = Event {
            user: Some(ME.into()),
            reaction: Some("thumbsup".into()),
            ..base(EventKind::ReactionAdded)
        };
        assert_eq!(engine.evaluate(&other, &state).unwrap().subscribed, 0);
    }

    #[test]
    fn a_rule_reaches_only_the_sinks_it_names() {
        let (state, _dir) = state();
        let engine = RuleEngine::new(
            vec![RuleConfig {
                mentions_me: true,
                sinks: vec!["agent".into()],
                ..rule("mention")
            }],
            vec!["stdout".into(), "agent".into()],
            Some(ME.to_string()),
        );

        let outcome = engine.evaluate(&message("<@U_ME>"), &state).unwrap();
        assert_eq!(outcome.sinks, vec!["agent".to_string()]);
    }

    #[test]
    fn matching_rules_contribute_the_union_of_their_sinks() {
        let (state, _dir) = state();
        let engine = RuleEngine::new(
            vec![
                RuleConfig {
                    mentions_me: true,
                    sinks: vec!["agent".into()],
                    ..rule("mention")
                },
                RuleConfig {
                    keywords: vec!["deploy".into()],
                    sinks: vec!["pager".into()],
                    ..rule("deploys")
                },
            ],
            vec!["agent".into(), "pager".into()],
            Some(ME.to_string()),
        );

        let outcome = engine
            .evaluate(&message("<@U_ME> deploy now"), &state)
            .unwrap();
        assert_eq!(
            outcome.rules,
            vec!["mention".to_string(), "deploys".to_string()]
        );
        assert_eq!(
            outcome.sinks,
            vec!["agent".to_string(), "pager".to_string()]
        );
    }

    #[test]
    fn a_rule_ignores_event_kinds_it_did_not_subscribe_to() {
        let (state, _dir) = state();
        let engine = engine(vec![RuleConfig {
            mentions_me: true,
            on: vec![EventKindConfig::Message],
            ..rule("mention")
        }]);

        let reaction = Event {
            user: Some(ME.into()),
            reaction: Some("eyes".into()),
            ..base(EventKind::ReactionAdded)
        };
        assert!(!engine.evaluate(&reaction, &state).unwrap().matched());
    }

    #[test]
    fn without_an_identity_nothing_that_depends_on_one_fires() {
        let (state, _dir) = state();
        let engine = RuleEngine::new(
            vec![RuleConfig {
                mentions_me: true,
                ..rule("mention")
            }],
            vec!["stdout".into()],
            None,
        );
        assert!(
            !engine
                .evaluate(&message("<@U_ME>"), &state)
                .unwrap()
                .matched()
        );
    }

    #[test]
    fn declared_channels_are_reported_once_each() {
        let engine = engine(vec![
            RuleConfig {
                channels: vec!["C0000001".into(), "C0000002".into()],
                ..rule("a")
            },
            RuleConfig {
                channels: vec!["C0000002".into()],
                ..rule("b")
            },
        ]);
        assert_eq!(engine.declared_channels(), vec!["C0000001", "C0000002"]);
    }
}
