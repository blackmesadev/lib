use std::{
    borrow::Cow, sync::{
        Arc, atomic::{AtomicBool, AtomicU64, Ordering}
    }, time::Instant
};

use crate::discord::{
    error::{DiscordError, DiscordResult},
    model::{ConnectionProperties, Identify, Payload, Resume, VoiceStateUpdateOutgoing},
    Message as DiscordMessage,
};
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::{
    net::TcpStream,
    sync::{mpsc, Mutex},
    time::{sleep, Duration},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::instrument;

use super::{DiscordRestClient, Guild, GuildMemberRemove, GuildMember, Hello, Intents, Ready, ShardConfig};
use super::{Channel, Id, VoiceServerUpdate, VoiceStateUpdate};
use super::{GuildBanEvent, GuildRoleDeleteEvent, GuildRoleEvent, InviteCreateEvent, InviteDeleteEvent, MessageDelete};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type WsSource = futures_util::stream::SplitStream<WsStream>;

// Opcode constants from https://docs.discord.com/developers/topics/opcodes-and-status-codes

const OPCODE_DISPATCH: u8 = 0;
const OPCODE_HEARTBEAT: u8 = 1;
const OPCODE_IDENTIFY: u8 = 2;
#[allow(dead_code)]
const OPCODE_PRESENCE_UPDATE: u8 = 3;
const OPCODE_VOICE_STATE_UPDATE: u8 = 4;
const OPCODE_RESUME: u8 = 6;
const OPCODE_RECONNECT: u8 = 7;
#[allow(dead_code)]
const OPCODE_REQUEST_GUILD_MEMBERS: u8 = 8;
const OPCODE_INVALID_SESSION: u8 = 9;
const OPCODE_HELLO: u8 = 10;
const OPCODE_HEARTBEAT_ACK: u8 = 11;

// Event type constants from https://docs.discord.com/developers/events/gateway-events

const EVENT_READY: &str = "READY";
const EVENT_GUILD_CREATE: &str = "GUILD_CREATE";
const EVENT_GUILD_UPDATE: &str = "GUILD_UPDATE";
const EVENT_GUILD_MEMBER_ADD: &str = "GUILD_MEMBER_ADD";
const EVENT_GUILD_MEMBER_UPDATE: &str = "GUILD_MEMBER_UPDATE";
const EVENT_GUILD_MEMBER_REMOVE: &str = "GUILD_MEMBER_REMOVE";
const EVENT_MESSAGE_CREATE: &str = "MESSAGE_CREATE";
const EVENT_MESSAGE_UPDATE: &str = "MESSAGE_UPDATE";
const EVENT_MESSAGE_DELETE: &str = "MESSAGE_DELETE";
const EVENT_VOICE_STATE_UPDATE: &str = "VOICE_STATE_UPDATE";
const EVENT_VOICE_SERVER_UPDATE: &str = "VOICE_SERVER_UPDATE";
const EVENT_CHANNEL_CREATE: &str = "CHANNEL_CREATE";
const EVENT_CHANNEL_UPDATE: &str = "CHANNEL_UPDATE";
const EVENT_CHANNEL_DELETE: &str = "CHANNEL_DELETE";
const EVENT_GUILD_ROLE_CREATE: &str = "GUILD_ROLE_CREATE";
const EVENT_GUILD_ROLE_UPDATE: &str = "GUILD_ROLE_UPDATE";
const EVENT_GUILD_ROLE_DELETE: &str = "GUILD_ROLE_DELETE";
const EVENT_GUILD_BAN_ADD: &str = "GUILD_BAN_ADD";
const EVENT_GUILD_BAN_REMOVE: &str = "GUILD_BAN_REMOVE";
const EVENT_INVITE_CREATE: &str = "INVITE_CREATE";
const EVENT_INVITE_DELETE: &str = "INVITE_DELETE";

const INVALID_SESSION_RETRY_SECS: u64 = 5;
const CHANNEL_CAPACITY: usize = 256;

#[derive(serde::Deserialize)]
struct PayloadEnvelope {
    op: u8,
    d: Option<serde_json::Value>,
    s: Option<u64>,
    t: Option<String>,
}

pub enum Event {
    Ready(Ready),
    MessageCreate(DiscordMessage),
    MessageUpdate(DiscordMessage),
    MessageDelete(MessageDelete),
    GuildCreate(Guild),
    GuildUpdate(Guild),
    GuildMemberAdd(GuildMember),
    GuildMemberUpdate(GuildMember),
    GuildMemberRemove(GuildMemberRemove),
    VoiceStateUpdate(VoiceStateUpdate),
    VoiceServerUpdate(VoiceServerUpdate),
    ChannelCreate(Channel),
    ChannelUpdate(Channel),
    ChannelDelete(Channel),
    GuildRoleCreate(GuildRoleEvent),
    GuildRoleUpdate(GuildRoleEvent),
    GuildRoleDelete(GuildRoleDeleteEvent),
    GuildBanAdd(GuildBanEvent),
    GuildBanRemove(GuildBanEvent),
    InviteCreate(InviteCreateEvent),
    InviteDelete(InviteDeleteEvent),
}

impl Event {
    /// Returns the event type name as a static string.
    pub const fn event_name(&self) -> &'static str {
        match self {
            Event::Ready(_) => "Ready",
            Event::MessageCreate(_) => "MessageCreate",
            Event::MessageUpdate(_) => "MessageUpdate",
            Event::MessageDelete(_) => "MessageDelete",
            Event::GuildCreate(_) => "GuildCreate",
            Event::GuildUpdate(_) => "GuildUpdate",
            Event::GuildMemberAdd(_) => "GuildMemberAdd",
            Event::GuildMemberUpdate(_) => "GuildMemberUpdate",
            Event::GuildMemberRemove(_) => "GuildMemberRemove",
            Event::VoiceStateUpdate(_) => "VoiceStateUpdate",
            Event::VoiceServerUpdate(_) => "VoiceServerUpdate",
            Event::ChannelCreate(_) => "ChannelCreate",
            Event::ChannelUpdate(_) => "ChannelUpdate",
            Event::ChannelDelete(_) => "ChannelDelete",
            Event::GuildRoleCreate(_) => "GuildRoleCreate",
            Event::GuildRoleUpdate(_) => "GuildRoleUpdate",
            Event::GuildRoleDelete(_) => "GuildRoleDelete",
            Event::GuildBanAdd(_) => "GuildBanAdd",
            Event::GuildBanRemove(_) => "GuildBanRemove",
            Event::InviteCreate(_) => "InviteCreate",
            Event::InviteDelete(_) => "InviteDelete",
        }
    }
}

struct Shared {
    sequence: AtomicU64,
    heartbeat_ack_received: AtomicBool,
    ping_nanos: AtomicU64,
    last_heartbeat: Mutex<Option<Instant>>,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sequence: AtomicU64::new(0),
            heartbeat_ack_received: AtomicBool::new(true),
            ping_nanos: AtomicU64::new(0),
            last_heartbeat: Mutex::new(None),
        })
    }
}

// Owns the WebSocket sink. Drives the heartbeat timer and flushes any outbound
// payloads posted by the handle via `outbound_rx`.
async fn run_tx(
    mut sink: WsSink,
    mut outbound_rx: mpsc::Receiver<Message>,
    heartbeat_interval_ms: u64,
    shared: Arc<Shared>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(heartbeat_interval_ms));
    // Discard the first immediate tick so heartbeats align with the interval.
    interval.tick().await;

    loop {
        tokio::select! {
            biased;

            msg = outbound_rx.recv() => {
                let Some(msg) = msg else {
                    tracing::debug!("TX: outbound channel closed, shutting down");
                    break;
                };
                if let Err(e) = sink.send(msg).await {
                    tracing::warn!(error = %e, "TX: failed to send outbound message");
                    break;
                }
            }

            _ = interval.tick() => {
                if !shared.heartbeat_ack_received.load(Ordering::Relaxed) {
                    tracing::warn!(
                        "TX: heartbeat ACK not received within interval; \
                         connection unhealthy"
                    );
                    break;
                }

                shared.heartbeat_ack_received.store(false, Ordering::Relaxed);

                let seq = shared.sequence.load(Ordering::Relaxed);
                let text = match serde_json::to_string(&heartbeat_payload(seq)) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "TX: failed to serialise heartbeat");
                        break;
                    }
                };

                tracing::debug!(seq, "TX: sending HEARTBEAT");
                *shared.last_heartbeat.lock().await = Some(Instant::now());

                if let Err(e) = sink.send(Message::Text(text.into())).await {
                    tracing::warn!(error = %e, "TX: failed to send HEARTBEAT");
                    break;
                }
            }
        }
    }

    tracing::debug!("TX task exited");
}

// Owns the WebSocket source. Parses inbound frames, updates shared state for
// control opcodes, and forwards dispatch events to the handle via `event_tx`.
async fn run_rx(
    mut source: WsSource,
    event_tx: mpsc::Sender<DiscordResult<Event>>,
    shared: Arc<Shared>,
) {
    loop {
        let raw = match source.next().await {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => {
                tracing::warn!(error = %e, "RX: WebSocket stream error");
                let _ = event_tx.send(Err(DiscordError::WebSocket(e))).await;
                break;
            }
            None => {
                tracing::warn!("RX: WebSocket stream closed unexpectedly");
                let _ = event_tx.send(Err(DiscordError::NotConnected)).await;
                break;
            }
        };

        match dispatch_raw(raw, &shared).await {
            Ok(Some(event)) => {
                if event_tx.send(Ok(event)).await.is_err() {
                    tracing::debug!("RX: event channel closed, shutting down");
                    break;
                }
            }
            Ok(None) => {} // control frame - no consumer-visible event
            Err(fatal) => {
                let _ = event_tx.send(Err(fatal)).await;
                break;
            }
        }
    }

    tracing::debug!("RX task exited");
}

/// Parse and handle a single raw WebSocket frame.
/// Returns `Err` only for fatal conditions that should terminate the RX task.
async fn dispatch_raw(msg: Message, shared: &Shared) -> DiscordResult<Option<Event>> {
    let text = match msg {
        Message::Text(t) => t,
        Message::Close(frame) => {
            tracing::warn!(?frame, "RX: received WebSocket close frame");
            return Err(DiscordError::NotConnected);
        }
        Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {
            return Ok(None);
        }
    };

    let envelope: PayloadEnvelope = match serde_json::from_str(&text) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "RX: failed to parse message envelope");
            return Ok(None);
        }
    };

    match envelope.op {
        OPCODE_DISPATCH => {
            if let Some(seq) = envelope.s {
                shared.sequence.store(seq, Ordering::Relaxed);
            }

            let event_type = match envelope.t.as_deref() {
                Some(t) => t,
                None => return Ok(None),
            };

            let event = match event_type {
                EVENT_READY => parse_dispatch::<Ready>(&text, EVENT_READY)?.map(Event::Ready),
                EVENT_GUILD_CREATE => {
                    parse_dispatch::<Guild>(&text, EVENT_GUILD_CREATE)?.map(Event::GuildCreate)
                }
                EVENT_GUILD_UPDATE => {
                    parse_dispatch::<Guild>(&text, EVENT_GUILD_UPDATE)?.map(Event::GuildUpdate)
                }
                EVENT_GUILD_MEMBER_ADD => {
                    parse_dispatch::<GuildMember>(&text, EVENT_GUILD_MEMBER_ADD)?
                        .map(Event::GuildMemberAdd)
                }
                EVENT_GUILD_MEMBER_UPDATE => {
                    parse_dispatch::<GuildMember>(&text, EVENT_GUILD_MEMBER_UPDATE)?
                        .map(Event::GuildMemberUpdate)
                }
                EVENT_GUILD_MEMBER_REMOVE => {
                    parse_dispatch::<GuildMemberRemove>(&text, EVENT_GUILD_MEMBER_REMOVE)?
                        .map(Event::GuildMemberRemove)
                }
                EVENT_MESSAGE_CREATE => {
                    tracing::debug!("RX: MESSAGE_CREATE");
                    parse_dispatch::<DiscordMessage>(&text, EVENT_MESSAGE_CREATE)?
                        .map(Event::MessageCreate)
                }
                EVENT_MESSAGE_UPDATE => {
                    parse_dispatch::<DiscordMessage>(&text, EVENT_MESSAGE_UPDATE)?
                        .map(Event::MessageUpdate)
                }
                EVENT_VOICE_STATE_UPDATE => {
                    parse_dispatch::<VoiceStateUpdate>(&text, EVENT_VOICE_STATE_UPDATE)?
                        .map(Event::VoiceStateUpdate)
                }
                EVENT_VOICE_SERVER_UPDATE => {
                    parse_dispatch::<VoiceServerUpdate>(&text, EVENT_VOICE_SERVER_UPDATE)?
                        .map(Event::VoiceServerUpdate)
                }
                EVENT_MESSAGE_DELETE => {
                    parse_dispatch::<MessageDelete>(&text, EVENT_MESSAGE_DELETE)?
                        .map(Event::MessageDelete)
                }
                EVENT_CHANNEL_CREATE => {
                    parse_dispatch::<Channel>(&text, EVENT_CHANNEL_CREATE)?
                        .map(Event::ChannelCreate)
                }
                EVENT_CHANNEL_UPDATE => {
                    parse_dispatch::<Channel>(&text, EVENT_CHANNEL_UPDATE)?
                        .map(Event::ChannelUpdate)
                }
                EVENT_CHANNEL_DELETE => {
                    parse_dispatch::<Channel>(&text, EVENT_CHANNEL_DELETE)?
                        .map(Event::ChannelDelete)
                }
                EVENT_GUILD_ROLE_CREATE => {
                    parse_dispatch::<GuildRoleEvent>(&text, EVENT_GUILD_ROLE_CREATE)?
                        .map(Event::GuildRoleCreate)
                }
                EVENT_GUILD_ROLE_UPDATE => {
                    parse_dispatch::<GuildRoleEvent>(&text, EVENT_GUILD_ROLE_UPDATE)?
                        .map(Event::GuildRoleUpdate)
                }
                EVENT_GUILD_ROLE_DELETE => {
                    parse_dispatch::<GuildRoleDeleteEvent>(&text, EVENT_GUILD_ROLE_DELETE)?
                        .map(Event::GuildRoleDelete)
                }
                EVENT_GUILD_BAN_ADD => {
                    parse_dispatch::<GuildBanEvent>(&text, EVENT_GUILD_BAN_ADD)?
                        .map(Event::GuildBanAdd)
                }
                EVENT_GUILD_BAN_REMOVE => {
                    parse_dispatch::<GuildBanEvent>(&text, EVENT_GUILD_BAN_REMOVE)?
                        .map(Event::GuildBanRemove)
                }
                EVENT_INVITE_CREATE => {
                    parse_dispatch::<InviteCreateEvent>(&text, EVENT_INVITE_CREATE)?
                        .map(Event::InviteCreate)
                }
                EVENT_INVITE_DELETE => {
                    parse_dispatch::<InviteDeleteEvent>(&text, EVENT_INVITE_DELETE)?
                        .map(Event::InviteDelete)
                }
                unknown => {
                    tracing::debug!(event = unknown, "RX: ignoring unknown event type");
                    None
                }
            };

            Ok(event)
        }

        OPCODE_HEARTBEAT_ACK => {
            shared.heartbeat_ack_received.store(true, Ordering::Relaxed);
            if let Some(sent_at) = *shared.last_heartbeat.lock().await {
                shared
                    .ping_nanos
                    .store(sent_at.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            Ok(None)
        }

        OPCODE_RECONNECT => {
            tracing::warn!("RX: Discord requested reconnection (op 7)");
            Err(DiscordError::Reconnect)
        }

        OPCODE_INVALID_SESSION => {
            // `d` is a boolean: true = resumable, false = must re-IDENTIFY
            let resumable = envelope
                .d
                .as_ref()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            tracing::warn!(resumable, "RX: INVALID_SESSION (op 9)");
            sleep(Duration::from_secs(INVALID_SESSION_RETRY_SECS)).await;
            Err(DiscordError::InvalidSession(resumable))
        }

        OPCODE_HELLO => {
            tracing::debug!("RX: HELLO received after initial connection");
            Ok(None)
        }

        unknown => {
            tracing::debug!(op = unknown, "RX: unknown opcode");
            Ok(None)
        }
    }
}

/// Parses a DISPATCH payload (op 0) into the specified event type.
/// Returns `Ok(None)` if parsing fails (non-fatal).
fn parse_dispatch<T: DeserializeOwned>(
    text: &str,
    event_type: &'static str,
) -> DiscordResult<Option<T>> {
    match serde_json::from_str::<Payload<T>>(text) {
        Ok(p) => Ok(p.d),
        Err(e) => {
            tracing::warn!(error = %e, event = event_type, "RX: failed to parse dispatch payload");
            Ok(None)
        }
    }
}

/// A lightweight, cheaply-cloneable handle for sending payloads to the Discord
/// gateway.  Obtained from [`DiscordWebsocket::gateway_sender`] after the
/// initial connection handshake completes.
#[derive(Clone)]
pub struct GatewaySender {
    tx: mpsc::Sender<Message>,
}

impl GatewaySender {
    /// Serializes and sends a payload to the Discord gateway.
    pub async fn send_payload(&self, payload: &impl Serialize) -> DiscordResult<()> {
        let text = serde_json::to_string(payload).map_err(DiscordError::Json)?;
        self.tx
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| DiscordError::NotConnected)
    }

    /// Sends a raw JSON value to the Discord gateway.
    pub async fn send_raw(&self, value: serde_json::Value) -> DiscordResult<()> {
        self.send_payload(&value).await
    }

    /// Sends a VOICE_STATE_UPDATE (op 4) to join or leave a voice channel.
    /// Pass `None` for `channel_id` to leave.
    pub async fn update_voice_state(
        &self,
        guild_id: &Id,
        channel_id: Option<&Id>,
        self_mute: bool,
        self_deaf: bool,
    ) -> DiscordResult<()> {
        self.send_payload(&Payload {
            op: OPCODE_VOICE_STATE_UPDATE,
            d: Some(VoiceStateUpdateOutgoing {
                guild_id: *guild_id,
                channel_id: channel_id.copied(),
                self_mute,
                self_deaf,
            }),
            s: None,
            t: None,
        })
        .await
    }
}

pub struct DiscordWebsocket {
    token: Cow<'static, str>,
    shard_config: ShardConfig,
    session_id: Option<String>,
    resume_gateway_url: Option<String>,

    /// Raw socket used during the HELLO/IDENTIFY handshake (before tasks spawn).
    pre_spawn_socket: Option<WsStream>,
    /// Heartbeat interval received from HELLO; passed to the TX task on spawn.
    heartbeat_interval_ms: u64,
    /// Sequence number to seed into Shared when spawning tasks.
    /// Non-zero when resuming a previous session.
    initial_seq: u64,

    /// Post-spawn: channel to the TX task.
    outbound_tx: Option<mpsc::Sender<Message>>,
    /// Post-spawn: channel from the RX task.
    event_rx: Option<mpsc::Receiver<DiscordResult<Event>>>,

    /// Shared atomic state - also accessible post-spawn.
    shared: Option<Arc<Shared>>,

    /// Caller-supplied latch updated each `next_event` call.
    ping_nanos_out: Arc<AtomicU64>,
}

/// State required to RESUME a previously established gateway session.
///
/// Extract from a disconnected [`DiscordWebsocket`] via [`DiscordWebsocket::resume_state`]
/// and pass to the next [`DiscordWebsocket::connect`] call.
#[derive(Debug, Clone)]
pub struct ResumeState {
    pub session_id: String,
    pub resume_gateway_url: String,
    pub seq: u64,
}

impl DiscordWebsocket {
    /// Connects to the Discord gateway WebSocket.
    ///
    /// If `resume` is `Some`, connects to the `resume_gateway_url` and prepares
    /// for RESUME (op 6).  Otherwise fetches the gateway URL from REST and
    /// prepares for IDENTIFY (op 2).
    #[instrument(skip(rest, token, resume), fields(shard_id = shard_config.shard_id, num_shards = shard_config.num_shards))]
    pub async fn connect(
        rest: Arc<DiscordRestClient>,
        token: &str,
        shard_config: ShardConfig,
        ping_nanos: Arc<AtomicU64>,
        resume: Option<ResumeState>,
    ) -> DiscordResult<Self> {
        let (session_id, resume_gateway_url, resume_seq, gateway_url) = if let Some(r) = resume {
            tracing::info!(
                resume_url = %r.resume_gateway_url,
                seq = r.seq,
                "attempting RESUME"
            );
            let url = r.resume_gateway_url.clone();
            (Some(r.session_id), Some(r.resume_gateway_url), r.seq, url)
        } else {
            tracing::info!("Initiating fresh WebSocket connection");
            let gateway = rest.get_gateway().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to fetch Discord gateway URL");
                DiscordError::ConnectionFailed("Failed to connect to Discord gateway".into())
            })?;
            tracing::debug!(gateway_url = %gateway.url, "Retrieved gateway URL");
            (None, None, 0, gateway.url)
        };

        let (socket, response) = connect_async(&gateway_url).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to establish WebSocket connection");
            DiscordError::ConnectionFailed("Failed to connect to Discord gateway".into())
        })?;

        tracing::info!(
            status = response.status().as_u16(),
            "WebSocket connection established"
        );

        // When resuming, seed the shared sequence counter with the last known
        // value so the RESUME payload and heartbeats carry the correct seq.
        let initial_seq = if session_id.is_some() { resume_seq } else { 0 };

        Ok(Self {
            token: Cow::Owned(token.trim().to_string()),
            shard_config,
            session_id,
            resume_gateway_url,
            pre_spawn_socket: Some(socket),
            heartbeat_interval_ms: 45_000,
            initial_seq,
            outbound_tx: None,
            event_rx: None,
            shared: None,
            ping_nanos_out: ping_nanos,
        })
    }

    /// Performs the HELLO / HEARTBEAT / IDENTIFY (or RESUME) handshake on the
    /// raw socket, then splits it and spawns the independent RX and TX tasks.
    ///
    /// After this returns, all sends go through `outbound_tx` and all events
    /// arrive from `event_rx`.
    #[instrument(skip(self))]
    pub async fn handle_initial_connection(&mut self) -> DiscordResult<()> {
        // recv HELLO
        let hello_text = {
            let socket = self
                .pre_spawn_socket
                .as_mut()
                .ok_or_else(|| DiscordError::ConnectionFailed("Tasks already spawned".into()))?;
            receive_one(socket, "HELLO").await?
        };

        let hello: Payload<Hello> = serde_json::from_str(&hello_text).map_err(|e| {
            tracing::warn!(error = %e, "Failed to parse HELLO payload");
            DiscordError::InvalidPayload("Invalid gateway payload".into())
        })?;

        let interval_ms = hello
            .d
            .as_ref()
            .ok_or_else(|| DiscordError::InvalidPayload("HELLO missing d field".into()))?
            .heartbeat_interval;
        self.heartbeat_interval_ms = interval_ms;

        tracing::debug!(heartbeat_ms = interval_ms, "Received HELLO");

        // Send initial HEARTBEAT

        self.send_payload(&Payload::<u64> {
            op: OPCODE_HEARTBEAT,
            d: Some(0),
            s: None,
            t: None,
        })
        .await?;

        // Send IDENTIFY or RESUME
        if let Some(session_id) = self.session_id.clone() {
            // Use the last received sequence number for RESUME.
            let seq = self
                .shared
                .as_ref()
                .map(|s| s.sequence.load(Ordering::Relaxed))
                .unwrap_or(0);

            self.send_payload(&Payload {
                op: OPCODE_RESUME,
                d: Some(Resume {
                    token: self.token.clone(),
                    session_id,
                    seq,
                }),
                s: None,
                t: None,
            })
            .await?;

            tracing::info!(seq, "Sent RESUME");
        } else {
            self.send_payload(&Payload {
                op: OPCODE_IDENTIFY,
                d: Some(Identify {
                    token: self.token.clone(),
                    properties: ConnectionProperties::new(),
                    intents: Intents::all(),
                    shard: self.shard_config.to_array(),
                }),
                s: None,
                t: None,
            })
            .await?;

            tracing::info!(
                shard_id = self.shard_config.shard_id,
                num_shards = self.shard_config.num_shards,
                "Sent IDENTIFY"
            );
        }

        // Spawn RX and TX tasks, handing off the socket and shared state
        let socket = self.pre_spawn_socket.take().expect("set above");
        let (sink, source) = socket.split();

        let shared = Shared::new();

        // Seed the shared sequence counter when resuming so heartbeats
        // carry the correct seq and the INVALID_SESSION check can extract
        // the right value.
        if self.initial_seq > 0 {
            shared.sequence.store(self.initial_seq, Ordering::Relaxed);
        }

        let (outbound_tx, outbound_rx) = mpsc::channel::<Message>(CHANNEL_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel::<DiscordResult<Event>>(CHANNEL_CAPACITY);

        tokio::spawn(run_tx(
            sink,
            outbound_rx,
            self.heartbeat_interval_ms,
            Arc::clone(&shared),
        ));

        tokio::spawn(run_rx(source, event_tx, Arc::clone(&shared)));

        self.outbound_tx = Some(outbound_tx);
        self.event_rx = Some(event_rx);
        self.shared = Some(shared);

        tracing::info!(
            shard_id = self.shard_config.shard_id,
            heartbeat_ms = interval_ms,
            "RX and TX tasks spawned"
        );

        Ok(())
    }

    /// Receives the next event from the gateway.
    /// Returns `Ok(None)` if the RX task exited.
    pub async fn next_event(&mut self) -> DiscordResult<Option<Event>> {
        let rx = self
            .event_rx
            .as_mut()
            .ok_or_else(|| DiscordError::NotConnected)?;

        if let Some(shared) = &self.shared {
            self.ping_nanos_out
                .store(shared.ping_nanos.load(Ordering::Relaxed), Ordering::Relaxed);
        }

        match rx.recv().await {
            Some(Ok(event)) => {
                if let Event::Ready(ref r) = event {
                    self.session_id = r.session_id.clone();
                    self.resume_gateway_url = r.resume_gateway_url.clone();
                    tracing::info!(
                        has_session = self.session_id.is_some(),
                        resume_url = ?self.resume_gateway_url,
                        "Bot READY",
                    );
                }
                Ok(Some(event))
            }
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Sends a serialized payload to the gateway.
    pub async fn send_payload(&mut self, payload: &impl Serialize) -> DiscordResult<()> {
        let text = serde_json::to_string(payload).map_err(DiscordError::Json)?;
        self.enqueue(Message::Text(text.into())).await
    }

    /// Sends a raw JSON value to the gateway.
    pub async fn send_raw(&mut self, value: serde_json::Value) -> DiscordResult<()> {
        self.send_payload(&value).await
    }

    /// Manually sends a HEARTBEAT (op 1) with the current sequence number.
    pub async fn send_heartbeat(&mut self) -> DiscordResult<()> {
        let seq = self
            .shared
            .as_ref()
            .map(|s| s.sequence.load(Ordering::Relaxed))
            .unwrap_or(0);
        self.send_payload(&Payload::<u64> {
            op: OPCODE_HEARTBEAT,
            d: Some(seq),
            s: None,
            t: None,
        })
        .await
    }

    /// Sends a VOICE_STATE_UPDATE (op 4) to join or leave a voice channel.
    /// Pass `None` for `channel_id` to leave the current voice channel.
    pub async fn update_voice_state(
        &mut self,
        guild_id: &Id,
        channel_id: Option<&Id>,
        self_mute: bool,
        self_deaf: bool,
    ) -> DiscordResult<()> {
        self.send_payload(&Payload {
            op: OPCODE_VOICE_STATE_UPDATE,
            d: Some(VoiceStateUpdateOutgoing {
                guild_id: *guild_id,
                channel_id: channel_id.copied(),
                self_mute,
                self_deaf,
            }),
            s: None,
            t: None,
        })
        .await
    }

    async fn enqueue(&mut self, msg: Message) -> DiscordResult<()> {
        if let Some(tx) = &self.outbound_tx {
            tx.send(msg).await.map_err(|_| {
                tracing::warn!("Outbound TX channel closed");
                DiscordError::NotConnected
            })
        } else if let Some(socket) = &mut self.pre_spawn_socket {
            socket.send(msg).await.map_err(|e| {
                tracing::warn!(error = %e, "Failed to send on pre-spawn socket");
                DiscordError::ConnectionFailed("Failed to send gateway payload".into())
            })
        } else {
            Err(DiscordError::NotConnected)
        }
    }

    /// Returns a [`GatewaySender`] that can send payloads to the gateway from
    /// any task without needing `&mut self` or access to this struct.
    /// Returns `None` if called before [`handle_initial_connection`].
    pub fn gateway_sender(&self) -> Option<GatewaySender> {
        self.outbound_tx.clone().map(|tx| GatewaySender { tx })
    }

    /// Returns the last measured gateway round-trip ping.
    pub fn ping(&self) -> Duration {
        Duration::from_nanos(self.ping_nanos_out.load(Ordering::Relaxed))
    }

    /// Returns a snapshot of the resume state for this session, if available.
    /// Used to carry `session_id`, `resume_gateway_url`, and `seq` across
    /// reconnection attempts so a RESUME can be sent instead of a full IDENTIFY.
    pub fn resume_state(&self) -> Option<ResumeState> {
        let session_id = self.session_id.clone()?;
        let resume_gateway_url = self.resume_gateway_url.clone()?;
        let seq = self
            .shared
            .as_ref()
            .map(|s| s.sequence.load(Ordering::Relaxed))
            .unwrap_or(0);
        Some(ResumeState {
            session_id,
            resume_gateway_url,
            seq,
        })
    }

    /// Returns `true` if the last heartbeat was acknowledged (connection is healthy).
    pub fn is_healthy(&self) -> bool {
        self.shared
            .as_ref()
            .map(|s| s.heartbeat_ack_received.load(Ordering::Relaxed))
            .unwrap_or(false)
    }
}

/// Reads a single text frame from the socket (used before tasks are spawned).
/// Ignores ping/pong frames and returns the first text message.
async fn receive_one(socket: &mut WsStream, context: &'static str) -> DiscordResult<String> {
    loop {
        match socket.next().await {
            None => {
                tracing::warn!(context, "Socket closed before expected payload");
                return Err(DiscordError::NotConnected);
            }
            Some(Err(e)) => {
                tracing::warn!(error = %e, context, "WebSocket error during handshake");
                return Err(DiscordError::ConnectionFailed(
                    "Gateway initialization failed".into(),
                ));
            }
            Some(Ok(Message::Text(t))) => return Ok(t.to_string()),
            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
            Some(Ok(_)) => {
                return Err(DiscordError::InvalidPayload(
                    "Non-text payload during handshake".into(),
                ));
            }
        }
    }
}

/// Constructs a HEARTBEAT payload (op 1) with the given sequence number.
fn heartbeat_payload(seq: u64) -> serde_json::Value {
    serde_json::json!({ "op": OPCODE_HEARTBEAT, "d": seq })
}
