use std::io;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::error;

use crate::model::mesastream::VoiceBridgePayload;

use super::error::{DiscordError, DiscordResult};
use super::Id;

const DISCORD_VOICE_GATEWAY_VERSION: u8 = 8;
const DISCORD_VOICE_GATEWAY_ENCODING: &str = "json";

/// Opus/RTP timing constants used when building RTP packet timestamps.
const SAMPLE_RATE: u32 = 48000;
const FRAME_LENGTH_MS: u32 = 20;

/// Discord speaking state flags
pub const SPEAKING_MICROPHONE: u8 = 1 << 0; // 1
pub const SPEAKING_SOUNDSHARE: u8 = 1 << 1; // 2
pub const SPEAKING_PRIORITY: u8 = 1 << 2; // 4

#[derive(Debug, Clone)]
pub struct VoiceConnectionDetails {
    pub guild_id: Id,
    pub player_id: Id,
    pub channel_id: Id,
    pub user_id: Id,
    pub session_id: String,
    pub token: String,
    pub endpoint: String,
}

impl VoiceConnectionDetails {
    pub fn voice_gateway_url(&self) -> String {
        format!(
            "wss://{}/?v={}&encoding={}",
            self.endpoint, DISCORD_VOICE_GATEWAY_VERSION, DISCORD_VOICE_GATEWAY_ENCODING
        )
    }

    pub fn into_bridge_payload(self) -> VoiceBridgePayload {
        let gateway_url = self.voice_gateway_url();

        VoiceBridgePayload {
            guild_id: self.guild_id,
            player_id: self.player_id,
            channel_id: self.channel_id,
            user_id: self.user_id,
            session_id: self.session_id,
            token: self.token,
            endpoint: self.endpoint,
            gateway_url,
        }
    }

    pub fn needs_endpoint_update(&self, new_endpoint: &str) -> bool {
        self.endpoint != new_endpoint
    }

    pub fn with_endpoint(&self, new_endpoint: String, new_token: String) -> Self {
        Self {
            endpoint: new_endpoint,
            token: new_token,
            ..self.clone()
        }
    }
}

/// UDP transport for sending encrypted Opus frames to Discord's voice server.
pub struct VoiceUdpConnection {
    socket: UdpSocket,
    ssrc: u32,
    sequence: Arc<RwLock<u16>>,
    timestamp: Arc<RwLock<u32>>,
    speaking: Arc<RwLock<u8>>,
    #[allow(dead_code)] // used once UDP-layer encryption is implemented
    encryption_key: [u8; 32],
}

impl VoiceUdpConnection {
    pub async fn connect(
        server_addr: SocketAddr,
        ssrc: u32,
        encryption_key: [u8; 32],
    ) -> DiscordResult<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|error| {
            error!(error = %error, "Failed to bind UDP socket for voice transport");
            DiscordError::Voice("Voice connection failed".to_string())
        })?;

        socket
            .connect(server_addr)
            .await
            .map_err(|error| {
                error!(error = %error, server_addr = %server_addr, "Failed to connect voice UDP socket");
                DiscordError::Voice("Voice connection failed".to_string())
            })?;

        Ok(Self {
            socket,
            ssrc,
            sequence: Arc::new(RwLock::new(0)),
            speaking: Arc::new(RwLock::new(0)),
            timestamp: Arc::new(RwLock::new(0)),
            encryption_key,
        })
    }

    pub async fn send_opus_packet(&self, opus_data: &[u8]) -> DiscordResult<()> {
        let mut packet = Vec::with_capacity(12 + opus_data.len());

        // RTP Header
        packet.extend_from_slice(&[0x80, 0x78]); // Version & Payload Type

        let sequence = {
            let mut seq = self.sequence.write().await;
            *seq = seq.wrapping_add(1);
            *seq
        };
        packet.extend_from_slice(&sequence.to_be_bytes());

        let timestamp = {
            let mut ts = self.timestamp.write().await;
            *ts = ts.wrapping_add(SAMPLE_RATE / 1000 * FRAME_LENGTH_MS);
            *ts
        };
        packet.extend_from_slice(&timestamp.to_be_bytes());

        packet.extend_from_slice(&self.ssrc.to_be_bytes());
        packet.extend_from_slice(opus_data);

        // encrypt the packet here using the encryption_key

        self.socket.send(&packet).await.map_err(|error| {
            error!(error = %error, "Failed to send voice packet");
            DiscordError::Voice("Voice packet send failed".to_string())
        })?;

        Ok(())
    }

    pub async fn send_silence(&self) -> DiscordResult<()> {
        // Standard Opus silence frame
        const SILENCE_FRAME: [u8; 3] = [0xF8, 0xFF, 0xFE];
        self.send_opus_packet(&SILENCE_FRAME).await
    }

    pub async fn set_speaking(&self, flags: u8) {
        *self.speaking.write().await = flags;
    }

    /// Get the current speaking state flags.
    pub async fn get_speaking(&self) -> u8 {
        *self.speaking.read().await
    }

    pub async fn close(&self) -> io::Result<()> {
        Ok(())
    }
}
