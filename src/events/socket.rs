use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::envelope::SocketEnvelope;

/// What arrived on the wire, once the JSON has been understood.
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// Slack's greeting, which also says how many connections this app holds.
    Hello {
        connections: u32,
    },
    /// Slack is about to close this connection. Every one of these is normal:
    /// connections are refreshed on a schedule.
    Disconnect {
        reason: String,
    },
    /// Something to acknowledge. Only `events_api` carries an event; the rest
    /// are acknowledged and ignored.
    Envelope(SocketEnvelope),
    Other,
}

impl PartialEq for SocketEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.envelope_id == other.envelope_id
            && self.kind == other.kind
            && self.payload == other.payload
            && self.retry_attempt == other.retry_attempt
    }
}

pub fn classify(value: &Value) -> Frame {
    match value.get("type").and_then(Value::as_str) {
        Some("hello") => Frame::Hello {
            connections: value
                .get("num_connections")
                .and_then(Value::as_u64)
                .unwrap_or(1) as u32,
        },
        Some("disconnect") => Frame::Disconnect {
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unspecified")
                .to_string(),
        },
        Some(_) if value.get("envelope_id").is_some() => {
            Frame::Envelope(SocketEnvelope::parse(value))
        }
        _ => Frame::Other,
    }
}

/// One live Socket Mode connection.
///
/// The single invariant it enforces: **an envelope is acknowledged before it
/// is handed on**. Slack redelivers what is not acknowledged and disables
/// event delivery to an app that stops answering, so acknowledgement is a
/// durability boundary and never a statement that the work is done.
pub struct SocketStream {
    inner: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl SocketStream {
    pub async fn connect(url: &str) -> Result<Self> {
        let (inner, _) = tokio_tungstenite::connect_async(url)
            .await
            .context("could not open the Socket Mode connection")?;
        Ok(Self { inner })
    }

    /// The next event envelope, or `None` when this connection is finished and
    /// the caller should open another.
    pub async fn next_event(&mut self) -> Result<Option<SocketEnvelope>> {
        while let Some(message) = self.inner.next().await {
            match message.context("the Socket Mode connection failed")? {
                Message::Text(text) => {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else {
                        tracing::debug!("ignoring a frame that is not JSON");
                        continue;
                    };

                    match classify(&value) {
                        Frame::Hello { connections } => {
                            tracing::info!(
                                connections,
                                "Socket Mode connected. Slack load-balances across an app's \
                                 connections, so run one daemon per app"
                            );
                        }
                        Frame::Disconnect { reason } => {
                            tracing::info!(%reason, "Slack asked to close this connection");
                            return Ok(None);
                        }
                        Frame::Envelope(envelope) => {
                            if let Some(id) = envelope.envelope_id.as_deref() {
                                self.acknowledge(id).await?;
                            }
                            if envelope.retry_attempt > 0 {
                                tracing::debug!(
                                    attempt = envelope.retry_attempt,
                                    "Slack redelivered an envelope"
                                );
                            }
                            if envelope.kind == "events_api" {
                                return Ok(Some(envelope));
                            }
                        }
                        Frame::Other => {}
                    }
                }
                Message::Ping(payload) => {
                    self.inner.send(Message::Pong(payload)).await?;
                }
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }

    async fn acknowledge(&mut self, envelope_id: &str) -> Result<()> {
        let ack = json!({ "envelope_id": envelope_id }).to_string();
        self.inner
            .send(Message::text(ack))
            .await
            .context("could not acknowledge an envelope")
    }

    pub async fn close(mut self) {
        let _ = self.inner.close(None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::SinkExt;
    use tokio::net::TcpListener;

    #[test]
    fn a_greeting_reports_how_many_connections_the_app_holds() {
        let frame = classify(&json!({ "type": "hello", "num_connections": 3 }));
        assert_eq!(frame, Frame::Hello { connections: 3 });
    }

    #[test]
    fn a_disconnect_carries_its_reason() {
        let frame = classify(&json!({ "type": "disconnect", "reason": "refresh_requested" }));
        assert_eq!(
            frame,
            Frame::Disconnect {
                reason: "refresh_requested".into()
            }
        );
    }

    #[test]
    fn anything_with_an_envelope_id_is_something_to_acknowledge() {
        let frame = classify(&json!({
            "type": "events_api",
            "envelope_id": "env-1",
            "payload": { "event": { "type": "message" } },
        }));
        match frame {
            Frame::Envelope(envelope) => {
                assert_eq!(envelope.envelope_id.as_deref(), Some("env-1"));
                assert_eq!(envelope.kind, "events_api");
            }
            other => panic!("expected an envelope, got {other:?}"),
        }
    }

    #[test]
    fn an_unrecognised_frame_is_not_an_error() {
        assert_eq!(classify(&json!({ "type": "unknown_thing" })), Frame::Other);
        assert_eq!(classify(&json!({})), Frame::Other);
    }

    /// The invariant, tested against a real socket: the acknowledgement is on
    /// the wire before the caller has seen the event, so a slow consumer can
    /// never turn into redelivery.
    #[tokio::test]
    async fn an_envelope_is_acknowledged_before_it_is_handed_on() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

            ws.send(Message::text(
                json!({ "type": "hello", "num_connections": 1 }).to_string(),
            ))
            .await
            .unwrap();
            ws.send(Message::text(
                json!({
                    "type": "events_api",
                    "envelope_id": "env-1",
                    "payload": {
                        "team_id": "T01",
                        "event_id": "Ev01",
                        "event": { "type": "message", "channel": "C01", "ts": "1.0" },
                    },
                })
                .to_string(),
            ))
            .await
            .unwrap();

            // Whatever comes back first must be the acknowledgement.
            let reply = ws.next().await.unwrap().unwrap();
            let ack: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
            ack
        });

        let mut socket = SocketStream::connect(&format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let envelope = socket.next_event().await.unwrap().unwrap();
        assert_eq!(envelope.envelope_id.as_deref(), Some("env-1"));

        let ack = server.await.unwrap();
        assert_eq!(ack["envelope_id"], "env-1");
    }

    #[tokio::test]
    async fn a_disconnect_frame_ends_the_stream_rather_than_failing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::text(
                json!({ "type": "disconnect", "reason": "refresh_requested" }).to_string(),
            ))
            .await
            .unwrap();
            // Hold the connection open so the end comes from the frame.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        });

        let mut socket = SocketStream::connect(&format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        assert!(socket.next_event().await.unwrap().is_none());
    }

    /// Slash commands and interactivity share the connection. They are
    /// acknowledged like anything else and then dropped, so an app subscribed
    /// to more than this daemon understands does not stall.
    #[tokio::test]
    async fn a_non_event_envelope_is_acknowledged_and_skipped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::text(
                json!({ "type": "slash_commands", "envelope_id": "env-slash", "payload": {} })
                    .to_string(),
            ))
            .await
            .unwrap();
            ws.send(Message::text(
                json!({
                    "type": "events_api",
                    "envelope_id": "env-real",
                    "payload": { "event": { "type": "message", "channel": "C01", "ts": "1.0" } },
                })
                .to_string(),
            ))
            .await
            .unwrap();

            let mut acks = Vec::new();
            for _ in 0..2 {
                let reply = ws.next().await.unwrap().unwrap();
                let value: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
                acks.push(value["envelope_id"].as_str().unwrap().to_string());
            }
            acks
        });

        let mut socket = SocketStream::connect(&format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let envelope = socket.next_event().await.unwrap().unwrap();
        assert_eq!(envelope.envelope_id.as_deref(), Some("env-real"));

        let acks = server.await.unwrap();
        assert_eq!(acks, vec!["env-slash".to_string(), "env-real".to_string()]);
    }
}
