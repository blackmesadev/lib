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
    time::{sleep, timeout, Interval},
};
use tokio::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

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
    json_buffer: String,
}

impl DiscordWebsocket {
    pub async fn connect(
        rest: Arc<DiscordRestClient>,
        token: Cow<'static, str>,
        shard_config: ShardConfig,
        ping_nanos: Arc<AtomicU64>,
    ) -> DiscordResult<Self> {
        tracing::info!(shard = ?shard_config.shard_id, "Starting WebSocket connection");

        let gateway = rest.get_gateway().await?;
        let (socket, _) = connect_async(&gateway.url)
            .await
            .map_err(DiscordError::WebSocket)?;

        let discord = DiscordWebsocket {
            token: token.clone(),
            socket,
            heartbeat_interval: tokio::time::interval(Duration::from_secs(45)),
            sequence: 0,
            shard_config,
            session_id: None,
            heartbeat_ack_received: Arc::new(AtomicBool::new(true)),
            last_heartbeat: None,
            ping_nanos,
            json_buffer: String::with_capacity(1024),
        };

        Ok(discord)
    }

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

        self.socket
            .next()
            .await
            .ok_or_else(|| DiscordError::NotConnected)?
            .map_err(DiscordError::WebSocket)?;

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
                    && last_heartbeat.elapsed() > Duration::from_secs(10)
                {
                    return Err(DiscordError::NotConnected);
                }
            }

            tokio::select! {
                _ = self.heartbeat_interval.tick() => {
                    if !self.heartbeat_ack_received.load(Ordering::Relaxed) {
                        return Err(DiscordError::NotConnected);
                    }
                    self.heartbeat_ack_received.store(false, Ordering::Relaxed);
                    self.send_heartbeat().await?;
                    self.last_heartbeat = Some(Instant::now());
                },
                msg = timeout(Duration::from_secs(60), self.socket.next()) => {
                    match msg {
                        Ok(Some(Ok(msg))) => {
                            if let Some(event) = self.handle_message(msg).await? {
                                return Ok(Some(event));
                            }
                        },
                        Ok(Some(Err(e))) => return Err(DiscordError::WebSocket(e)),
                        Ok(None) => return Ok(None),
                        Err(_) => return Err(DiscordError::NotConnected),
                    }
                },
            }
        }
    }

    async fn handle_message(&mut self, msg: Message) -> DiscordResult<Option<Event>> {
        let text = match msg {
            Message::Text(text) => text,
            _ => return Ok(None),
        };

        let envelope: PayloadEnvelope = match serde_json::from_str(&text) {
            Ok(envelope) => envelope,
            Err(_) => return Ok(None),
        };

        match envelope.op {
            OPCODE_DISPATCH => {
                if let Some(seq) = envelope.s {
                    self.sequence = seq;
                }

                match envelope.t.as_deref() {
                    Some(EVENT_READY) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<Ready>>(&text) {
                            if let Some(ready) = payload.d {
                                self.session_id = ready.session_id.clone();
                                return Ok(Some(Event::Ready(ready)));
                            }
                        }
                    }
                    Some(EVENT_GUILD_CREATE) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<Guild>>(&text) {
                            if let Some(guild) = payload.d {
                                return Ok(Some(Event::GuildCreate(guild)));
                            }
                        }
                    }
                    Some(EVENT_GUILD_UPDATE) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<Guild>>(&text) {
                            if let Some(guild) = payload.d {
                                return Ok(Some(Event::GuildUpdate(guild)));
                            }
                        }
                    }
                    Some(EVENT_GUILD_MEMBER_UPDATE) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<GuildMemberUpdate>>(&text) {
                            if let Some(member_update) = payload.d {
                                return Ok(Some(Event::GuildMemberUpdate(member_update)));
                            }
                        }
                    }
                    Some(EVENT_MESSAGE_CREATE) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<DiscordMessage>>(&text) {
                            if let Some(message) = payload.d {
                                return Ok(Some(Event::MessageCreate(message)));
                            }
                        }
                    }
                    Some(EVENT_MESSAGE_UPDATE) => {
                        if let Ok(payload) = serde_json::from_str::<Payload<DiscordMessage>>(&text) {
                            if let Some(message) = payload.d {
                                return Ok(Some(Event::MessageUpdate(message)));
                            }
                        }
                    }
                    _ => {}
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
                return Err(DiscordError::NotConnected);
            }
            OPCODE_INVALID_SESSION => {
                self.session_id = None;
                sleep(Duration::from_secs(5)).await;
                return Err(DiscordError::NotConnected);
            }
            _ => {}
        }

        Ok(None)
    }

    pub async fn send_payload<T: Serialize>(
        &mut self,
        payload: &Payload<T>,
    ) -> DiscordResult<()> {
        self.json_buffer.clear();

        let json_str = serde_json::to_string(payload).map_err(DiscordError::Json)?;

        let msg = Message::text(json_str);

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
        self.send_payload(&payload).await
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

    async fn set_speaking(&mut self, speaking: bool) -> DiscordResult<()> {
        let speaking_data = serde_json::json!({
            "speaking": speaking,
            "delay": 0,
            "ssrc": 1
        });

        let payload = Payload {
            op: 5,
            d: Some(speaking_data),
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
