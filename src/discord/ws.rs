use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use crate::discord::{
    error::{DiscordError, DiscordResult},
    model::{ConnectionProperties, Identify, Payload},
    Message as DiscordMessage,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::{
    net::TcpStream,
    time::{sleep, Interval},
};
use tokio::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::instrument;

use super::{DiscordRestClient, Guild, GuildMemberUpdate, Hello, Intents, Ready, ShardConfig};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

// Discord Gateway opcodes
const OPCODE_DISPATCH: u8 = 0;
const OPCODE_HEARTBEAT: u8 = 1;
const OPCODE_IDENTIFY: u8 = 2;
const OPCODE_RESUME: u8 = 6;
const OPCODE_RECONNECT: u8 = 7;
const OPCODE_INVALID_SESSION: u8 = 9;
const OPCODE_HELLO: u8 = 10;
const OPCODE_HEARTBEAT_ACK: u8 = 11;

// Event type constants
const EVENT_READY: &str = "READY";
const EVENT_GUILD_CREATE: &str = "GUILD_CREATE";
const EVENT_GUILD_UPDATE: &str = "GUILD_UPDATE";
const EVENT_GUILD_MEMBER_UPDATE: &str = "GUILD_MEMBER_UPDATE";
const EVENT_MESSAGE_CREATE: &str = "MESSAGE_CREATE";
const EVENT_MESSAGE_UPDATE: &str = "MESSAGE_UPDATE";

#[derive(serde::Deserialize)]
struct PayloadEnvelope {
    op: u8,
    s: Option<u64>,
    t: Option<String>,
}

pub enum Event {
    Ready(Ready),
    MessageCreate(DiscordMessage),
    MessageUpdate(DiscordMessage),
    GuildCreate(Guild),
    GuildUpdate(Guild),
    GuildMemberUpdate(GuildMemberUpdate),
}

pub struct DiscordWebsocket {
    token: Cow<'static, str>,
    socket: WsStream,
    heartbeat_interval: Interval,
    sequence: u64,
    shard_config: ShardConfig,
    session_id: Option<String>,
    heartbeat_ack_received: Arc<AtomicBool>,
    last_heartbeat: Option<Instant>,
    ping_nanos: Arc<AtomicU64>,
}

impl DiscordWebsocket {
    #[instrument(skip(rest, token), fields(shard_id = shard_config.shard_id, num_shards = shard_config.num_shards))]
    pub async fn connect(
        rest: Arc<DiscordRestClient>,
        token: &str,
        shard_config: ShardConfig,
        ping_nanos: Arc<AtomicU64>,
    ) -> DiscordResult<Self> {
        tracing::info!("Initiating WebSocket connection");

        let gateway = rest.get_gateway().await?;
        tracing::debug!(gateway_url = %gateway.url, "Retrieved gateway URL");

        let (socket, response) = connect_async(&gateway.url)
            .await
            .map_err(DiscordError::WebSocket)?;

        tracing::info!(
            status = response.status().as_u16(),
            "WebSocket connection established"
        );

        let trimmed_token = token.trim().to_string();
        let token = Cow::Owned(trimmed_token);

        let discord = DiscordWebsocket {
            token,
            socket,
            heartbeat_interval: tokio::time::interval(Duration::from_secs(45)),
            sequence: 0,
            shard_config,
            session_id: None,
            heartbeat_ack_received: Arc::new(AtomicBool::new(true)),
            last_heartbeat: None,
            ping_nanos,
        };

        Ok(discord)
    }

    #[instrument(skip(self))]
    pub async fn handle_initial_connection(&mut self) -> DiscordResult<()> {
        let hello: Payload<Hello> = {
            let msg = self
                .socket
                .next()
                .await
                .ok_or_else(|| DiscordError::NotConnected)?
                .map_err(DiscordError::WebSocket)?;
            let text = msg.into_text().map_err(DiscordError::WebSocket)?;
            serde_json::from_str(&text)?
        };

        self.heartbeat_interval = tokio::time::interval(Duration::from_millis(
            hello
                .d
                .as_ref()
                .ok_or_else(|| DiscordError::InvalidPayload("Hello missing d field".to_string()))?
                .heartbeat_interval,
        ));

        self.send_heartbeat().await?;

        if let Some(session_id) = &self.session_id {
            self.resume(session_id.clone()).await?;
        } else {
            self.identify().await?;
        }

        Ok(())
    }

    pub async fn next_event(&mut self) -> DiscordResult<Option<Event>> {
        loop {
            if let Some(last_heartbeat) = self.last_heartbeat {
                if !self.heartbeat_ack_received.load(Ordering::Relaxed)
                    && last_heartbeat.elapsed() > Duration::from_secs(15)
                {
                    tracing::warn!("Heartbeat ACK not received within 15 seconds, connection unhealthy");
                    return Err(DiscordError::NotConnected);
                }
            }

            tokio::select! {
                _ = self.heartbeat_interval.tick() => {
                    if !self.heartbeat_ack_received.load(Ordering::Relaxed) {
                        tracing::warn!("Previous heartbeat ACK not received, connection may be unhealthy");
                        return Err(DiscordError::NotConnected);
                    }
                    self.heartbeat_ack_received.store(false, Ordering::Relaxed);
                    if let Err(e) = self.send_heartbeat().await {
                        tracing::warn!("Failed to send heartbeat: {:?}", e);
                        return Err(e);
                    }
                    self.last_heartbeat = Some(Instant::now());
                },
                msg = self.socket.next() => {
                    match msg {
                        Some(Ok(msg)) => {
                            match self.handle_message(msg).await {
                                Ok(Some(event)) => return Ok(Some(event)),
                                Ok(None) => continue,
                                Err(e) => {
                                    tracing::warn!("Error handling WebSocket message: {:?}", e);
                                    return Err(e);
                                }
                            }
                        },
                        Some(Err(e)) => {
                            tracing::warn!("WebSocket error: {:?}", e);
                            return Err(DiscordError::WebSocket(e));
                        },
                        None => {
                            tracing::warn!("WebSocket stream closed unexpectedly");
                            return Err(DiscordError::NotConnected);
                        },
                    }
                },
            }
        }
    }

    async fn handle_message(&mut self, msg: Message) -> DiscordResult<Option<Event>> {
        let text = match msg {
            Message::Text(text) => text,
            Message::Close(close_frame) => {
                tracing::warn!("Received WebSocket close frame: {:?}", close_frame);
                return Err(DiscordError::NotConnected);
            }
            Message::Ping(_) => {
                return Ok(None);
            }
            Message::Pong(_) => {
                return Ok(None);
            }
            _ => {
                return Ok(None);
            }
        };

        let envelope: PayloadEnvelope = match serde_json::from_str(&text) {
            Ok(envelope) => envelope,
            Err(e) => {
                tracing::warn!("Failed to parse WebSocket message envelope: {} (message: {})", e, text.chars().take(100).collect::<String>());
                return Ok(None);
            }
        };

        match envelope.op {
            OPCODE_DISPATCH => {
                if let Some(seq) = envelope.s {
                    self.sequence = seq;
                }

                if let Some(event_type) = envelope.t.as_ref() {
                    match event_type.as_str() {
                        EVENT_READY => {
                            if let Ok(payload) = serde_json::from_str::<Payload<Ready>>(&text) {
                                if let Some(ready) = payload.d {
                                    self.session_id = ready.session_id.clone();
                                    tracing::info!("Bot ready with session ID: {:?}", self.session_id);
                                    return Ok(Some(Event::Ready(ready)));
                                }
                            }
                        }
                        EVENT_GUILD_CREATE => {
                            if let Ok(payload) = serde_json::from_str::<Payload<Guild>>(&text) {
                                if let Some(guild) = payload.d {
                                    return Ok(Some(Event::GuildCreate(guild)));
                                }
                            }
                        }
                        EVENT_GUILD_UPDATE => {
                            if let Ok(payload) = serde_json::from_str::<Payload<Guild>>(&text) {
                                if let Some(guild) = payload.d {
                                    return Ok(Some(Event::GuildUpdate(guild)));
                                }
                            }
                        }
                        EVENT_GUILD_MEMBER_UPDATE => {
                            if let Ok(payload) = serde_json::from_str::<Payload<GuildMemberUpdate>>(&text) {
                                if let Some(member_update) = payload.d {
                                    return Ok(Some(Event::GuildMemberUpdate(member_update)));
                                }
                            }
                        }
                        EVENT_MESSAGE_CREATE => {
                            if let Ok(payload) = serde_json::from_str::<Payload<DiscordMessage>>(&text) {
                                if let Some(message) = payload.d {
                                    return Ok(Some(Event::MessageCreate(message)));
                                }
                            }
                        }
                        EVENT_MESSAGE_UPDATE => {
                            if let Ok(payload) = serde_json::from_str::<Payload<DiscordMessage>>(&text) {
                                if let Some(message) = payload.d {
                                    return Ok(Some(Event::MessageUpdate(message)));
                                }
                            }
                        }
                        _ => {
                            tracing::debug!("Ignoring unknown event type: {}", event_type);
                        }
                    }
                }
            }
            OPCODE_HEARTBEAT_ACK => {
                self.heartbeat_ack_received.store(true, Ordering::Relaxed);
                if let Some(last_hb) = self.last_heartbeat {
                    let ping_nanos = last_hb.elapsed().as_nanos() as u64;
                    self.ping_nanos.store(ping_nanos, Ordering::Relaxed);
                }
            }
            OPCODE_RECONNECT => {
                tracing::warn!("Discord requested reconnection");
                return Err(DiscordError::NotConnected);
            }
            OPCODE_INVALID_SESSION => {
                tracing::warn!("Discord sent invalid session, clearing session ID and reconnecting");
                self.session_id = None;
                sleep(Duration::from_secs(5)).await;
                return Err(DiscordError::NotConnected);
            }
            OPCODE_HELLO => {
                tracing::debug!("Received HELLO opcode after initial connection");
            }
            _ => {
                tracing::debug!("Received unknown opcode: {}", envelope.op);
            }
        }

        Ok(None)
    }

    pub async fn send_payload<T: Serialize>(
        &mut self,
        payload: &Payload<T>,
    ) -> DiscordResult<()> {
        let msg_text = serde_json::to_string(payload).map_err(DiscordError::Json)?;
        let msg = Message::Text(msg_text.into());

        self.socket
            .send(msg)
            .await
            .map_err(DiscordError::WebSocket)?;

        Ok(())
    }

    pub async fn send_heartbeat(&mut self) -> DiscordResult<()> {
        let payload = Payload {
            op: OPCODE_HEARTBEAT,
            d: Some(self.sequence),
            s: None,
            t: None,
        };

        tracing::debug!(
            shard_id = self.shard_config.shard_id,
            seq = self.sequence,
            "Sending HEARTBEAT payload"
        );

        self.send_payload(&payload).await?;

        Ok(())
    }

    async fn identify(&mut self) -> DiscordResult<()> {
        let identify = Identify {
            token: self.token.clone(),
            properties: ConnectionProperties::new(),
            intents: Intents::all(),
            shard: self.shard_config.to_array(),
        };

        let payload = Payload {
            op: OPCODE_IDENTIFY,
            d: Some(identify),
            s: None,
            t: None,
        };
        tracing::info!(
            shard_id = self.shard_config.shard_id,
            num_shards = self.shard_config.num_shards,
            token = self.token.as_ref(),
            "Sending IDENTIFY payload"
        );
        self.send_payload(&payload).await
    }

    async fn resume(&mut self, session_id: String) -> DiscordResult<()> {
        let resume = serde_json::json!({
            "token": self.token,
            "session_id": session_id,
            "seq": self.sequence
        });

        let payload = Payload {
            op: OPCODE_RESUME,
            d: Some(resume),
            s: None,
            t: None,
        };
        self.send_payload(&payload).await
    }

    pub fn ping(&self) -> Duration {
        Duration::from_nanos(self.ping_nanos.load(Ordering::Relaxed))
    }

    pub fn is_healthy(&self) -> bool {
        self.heartbeat_ack_received.load(Ordering::Relaxed)
    }
}
