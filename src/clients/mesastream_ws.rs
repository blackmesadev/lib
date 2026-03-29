//! WebSocket client for receiving real-time [`MesastreamEvent`]s from the
//! mesastream service.
//!
//! Usage:
//! ```ignore
//! let (client, mut rx) = MesastreamWsClient::new("ws://localhost:8070/ws");
//! tokio::spawn(async move { client.run().await; });
//! while let Some(event) = rx.recv().await { /* handle event */ }
//! ```

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::model::mesastream::MesastreamEvent;

const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(15);
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Async WebSocket client that connects to the mesastream event stream,
/// automatically reconnects on failure, and forwards parsed events to
/// an `mpsc` channel.
pub struct MesastreamWsClient {
    url: String,
    tx: mpsc::Sender<MesastreamEvent>,
}

impl MesastreamWsClient {
    /// Creates a new client and the receiving end of the event channel.
    ///
    /// Call [`run`](Self::run) in a spawned task to start the connection loop.
    pub fn new(url: impl Into<String>) -> (Self, mpsc::Receiver<MesastreamEvent>) {
        let (tx, rx) = mpsc::channel(64);
        (
            Self {
                url: url.into(),
                tx,
            },
            rx,
        )
    }

    /// Run the connection loop forever.  Reconnects automatically with
    /// exponential back-off when the connection drops.
    ///
    /// Exits only when the receiving end of the event channel is dropped
    /// (i.e. the consumer is gone).
    pub async fn run(&self) {
        let mut attempts: u32 = 0;

        loop {
            if self.tx.is_closed() {
                info!("mesastream ws: event receiver dropped — stopping");
                return;
            }

            info!(url = %self.url, attempt = attempts + 1, "mesastream ws: connecting");

            match connect_async(&self.url).await {
                Ok((ws_stream, _)) => {
                    attempts = 0;
                    info!("mesastream ws: connected");

                    // Notify consumer that we're (re)connected so it can
                    // recreate players or refresh state.
                    if self.tx.send(MesastreamEvent::Connected).await.is_err() {
                        return; // receiver dropped
                    }

                    let (mut sink, mut stream) = ws_stream.split();

                    let mut ping_interval = tokio::time::interval(PING_INTERVAL);
                    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                    loop {
                        tokio::select! {
                            msg = stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        match serde_json::from_str::<MesastreamEvent>(&text) {
                                            Ok(event) => {
                                                if matches!(&event, MesastreamEvent::Goodbye) {
                                                    info!("mesastream ws: received Goodbye — server shutting down");
                                                }
                                                if self.tx.send(event).await.is_err() {
                                                    return; // receiver dropped
                                                }
                                            }
                                            Err(e) => {
                                                warn!(error = %e, raw = %text, "mesastream ws: failed to parse event");
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Ping(data))) => {
                                        let _ = sink.send(Message::Pong(data)).await;
                                    }
                                    Some(Ok(Message::Close(_))) | None => {
                                        info!("mesastream ws: connection closed");
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        warn!(error = %e, "mesastream ws: read error");
                                        break;
                                    }
                                    _ => {} // binary, pong — ignore
                                }
                            }
                            _ = ping_interval.tick() => {
                                if sink.send(Message::Ping(vec![].into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "mesastream ws: connect failed");
                }
            }

            attempts += 1;
            let delay = std::cmp::min(
                RECONNECT_BASE_DELAY * 2u32.saturating_pow(attempts.min(10)),
                RECONNECT_MAX_DELAY,
            );
            warn!(delay_ms = delay.as_millis() as u64, attempt = attempts, "mesastream ws: reconnecting");
            tokio::time::sleep(delay).await;
        }
    }
}
