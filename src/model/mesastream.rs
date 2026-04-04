use serde::{Deserialize, Serialize};

use crate::discord::Id;

/// All Discord voice session fields needed by mesastream to connect.
/// Built by Black Mesa via `VoiceConnectionDetails::into_bridge_payload()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceBridgePayload {
    pub guild_id: Id,
    /// Identifies the player; equals `channel_id` by convention.
    pub player_id: Id,
    pub channel_id: Id,
    pub user_id: Id,
    pub session_id: String,
    pub token: String,
    pub endpoint: String,
    /// Full WSS URL for the Discord Voice Gateway, e.g. `wss://endpoint/?v=4&encoding=json`
    pub gateway_url: String,
}

pub type CreatePlayerRequest = VoiceBridgePayload;
pub type UpdateConnectionRequest = VoiceBridgePayload;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueRequest {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeekRequest {
    pub position_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeRequest {
    /// Volume multiplier in [0.0, 2.0].
    pub volume: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Youtube,
    Soundcloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerPlaybackStatus {
    Idle,
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub artist: String,
    pub title: String,
    pub duration_ms: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// UUID string assigned by mesastream.
    pub id: String,
    pub source: SourceKind,
    pub source_url: String,
    pub stream_url: String,
    pub metadata: TrackMetadata,
    /// Error message if track failed to play (set during playback).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 3-band equalizer settings for audio processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqualizerSettings {
    /// Bass gain multiplier (0.0 - 2.0, default 1.0). Affects frequencies < 250 Hz.
    pub bass: f32,
    /// Mid gain multiplier (0.0 - 2.0, default 1.0). Affects frequencies 250 Hz - 4000 Hz.
    pub mid: f32,
    /// Treble gain multiplier (0.0 - 2.0, default 1.0). Affects frequencies > 4000 Hz.
    pub treble: f32,
}

impl Default for EqualizerSettings {
    fn default() -> Self {
        Self {
            bass: 1.0,
            mid: 1.0,
            treble: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStateSnapshot {
    /// Discord voice channel ID (snowflake).
    pub player_id: Id,
    pub connection: VoiceBridgePayload,
    pub status: PlayerPlaybackStatus,
    pub queue: Vec<Track>,
    pub current_track: Option<Track>,
    pub position_ms: u64,
    pub volume: f32,
    pub equalizer: EqualizerSettings,
    /// Whether the voice transport is currently connected to Discord.
    pub voice_connected: bool,
    /// Current playback session ID for telemetry correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSnapshot {
    /// Discord voice channel ID (snowflake).
    pub player_id: Id,
    pub name: String,
    pub tracks: Vec<Track>,
}

/// Current track info returned by the `/current` endpoint.
/// Includes playback position and duration with both raw ms and formatted strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentTrackResponse {
    pub track: Track,
    /// Current playback position in milliseconds.
    pub position_ms: u64,
    /// Total track duration in milliseconds.
    pub duration_ms: u64,
    /// Current position as "MM:SS".
    pub position: String,
    /// Total duration as "MM:SS".
    pub duration: String,
    /// Remaining time as "MM:SS".
    pub remaining: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}

/// Envelope for mesastream → client WebSocket events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MesastreamEvent {
    /// A new player was created (queue/position may have been restored from cache).
    PlayerCreated {
        guild_id: Id,
        player_id: Id,
        /// Number of tracks restored from the persisted queue.
        restored_queue_len: usize,
        /// Playback position restored from cache (0 if none).
        restored_position_ms: u64,
    },
    /// A player was destroyed (explicit stop or auto-destroy after queue drained).
    PlayerDestroyed {
        guild_id: Id,
        player_id: Id,
        /// `true` when the player was explicitly stopped, `false` when auto-destroyed.
        was_stopped: bool,
    },
    /// A track started playing.
    TrackStarted {
        guild_id: Id,
        player_id: Id,
        track: Track,
        /// Position the track started at (>0 when resuming mid-track).
        position_ms: u64,
    },
    /// A track finished playing (natural end, not skip/stop).
    TrackEnded {
        guild_id: Id,
        player_id: Id,
        track_id: String,
    },
    /// Voice transport disconnected - mesastream cannot send audio.
    /// Black Mesa should obtain fresh credentials from Discord and push
    /// them via `PUT /players/{id}/connection`.
    VoiceDisconnected {
        guild_id: Id,
        player_id: Id,
        reason: String,
    },
    /// Server is shutting down - clients should prepare to reconnect.
    Goodbye,
    /// Emitted locally by the WS client (not the server) when a connection
    /// is established or re-established.  Black Mesa uses this to detect
    /// mesastream restarts and recreate players.
    Connected,
}
