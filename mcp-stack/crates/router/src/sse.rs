use std::convert::Infallible;

use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::router::RouterState;

#[derive(Debug, Clone, Serialize)]
pub struct RouterEvent {
    pub id: String,
    pub event: String,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct SseHub {
    sender: broadcast::Sender<RouterEvent>,
}

impl SseHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self { sender }
    }

    pub fn publish(&self, event: RouterEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RouterEvent> {
        self.sender.subscribe()
    }
}

pub async fn stream(
    State(state): State<RouterState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.sse.subscribe();
    let stream = stream! {
        loop {
            match receiver.recv().await {
                Ok(evt) => {
                    let mut event = Event::default()
                        .json_data(&evt.payload)
                        .unwrap_or_else(|_| Event::default());
                    event = event.id(evt.id).event(evt.event);
                    yield Ok(event);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publishes_and_receives_events() {
        let hub = SseHub::new();
        let mut rx = hub.subscribe();
        let event = RouterEvent {
            id: "1".into(),
            event: "test".into(),
            payload: serde_json::json!({"ok": true}),
        };
        hub.publish(event.clone());
        let received = rx.recv().await.expect("receive event");
        assert_eq!(received.event, event.event);
        assert_eq!(received.id, event.id);
    }

    #[tokio::test]
    async fn lagged_receiver_reports_skipped_messages() {
        let hub = SseHub::new();
        let mut rx = hub.subscribe();
        for i in 0..1100 {
            hub.publish(RouterEvent {
                id: i.to_string(),
                event: "tick".into(),
                payload: serde_json::json!({"seq": i}),
            });
        }
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(skipped)) => assert!(skipped > 0),
            other => panic!("expected lagged error, got {other:?}"),
        }
    }
}
