use std::fmt;

use serde::{Deserialize, Serialize};

use crate::discord::Id;

/// Top-level enum that unifies Discord gateway events and Mesa-internal events.
/// This allows the logging system to use a single event type for lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "source", content = "event")]
pub enum LogEventType {
    /// Discord gateway events relevant to guild admins
    Discord(DiscordLogEvent),
    /// Black Mesa internal events (automod, moderation actions, etc.)
    Mesa(MesaLogEvent),
}

impl LogEventType {
    /// Returns the string key used in the database `event` column.
    pub fn as_db_key(&self) -> &'static str {
        match self {
            LogEventType::Discord(e) => e.as_str(),
            LogEventType::Mesa(e) => e.as_str(),
        }
    }

    /// Parse from the database `event` column string.
    pub fn from_db_key(s: &str) -> Option<Self> {
        if let Some(e) = DiscordLogEvent::from_str(s) {
            return Some(LogEventType::Discord(e));
        }
        if let Some(e) = MesaLogEvent::from_str(s) {
            return Some(LogEventType::Mesa(e));
        }
        None
    }

    /// List all supported log event types.
    pub fn all() -> Vec<LogEventType> {
        let mut events = Vec::new();
        for e in DiscordLogEvent::all() {
            events.push(LogEventType::Discord(e));
        }
        for e in MesaLogEvent::all() {
            events.push(LogEventType::Mesa(e));
        }
        events
    }

    /// Available placeholder variables for this event type.
    pub fn placeholders(&self) -> &'static [&'static str] {
        match self {
            LogEventType::Discord(e) => e.placeholders(),
            LogEventType::Mesa(e) => e.placeholders(),
        }
    }
}

impl fmt::Display for LogEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_key())
    }
}

/// Discord gateway events that guild admins would care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscordLogEvent {
    MessageDelete,
    MessageUpdate,
    GuildMemberAdd,
    GuildMemberRemove,
    GuildMemberUpdate,
    GuildUpdate,
    VoiceStateUpdate,
    MessageCreate,
    ChannelCreate,
    ChannelUpdate,
    ChannelDelete,
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    GuildBanAdd,
    GuildBanRemove,
    InviteCreate,
    InviteDelete,
}

impl DiscordLogEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MessageDelete => "message_delete",
            Self::MessageUpdate => "message_update",
            Self::GuildMemberAdd => "guild_member_add",
            Self::GuildMemberRemove => "guild_member_remove",
            Self::GuildMemberUpdate => "guild_member_update",
            Self::GuildUpdate => "guild_update",
            Self::VoiceStateUpdate => "voice_state_update",
            Self::MessageCreate => "message_create",
            Self::ChannelCreate => "channel_create",
            Self::ChannelUpdate => "channel_update",
            Self::ChannelDelete => "channel_delete",
            Self::RoleCreate => "role_create",
            Self::RoleUpdate => "role_update",
            Self::RoleDelete => "role_delete",
            Self::GuildBanAdd => "guild_ban_add",
            Self::GuildBanRemove => "guild_ban_remove",
            Self::InviteCreate => "invite_create",
            Self::InviteDelete => "invite_delete",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "message_delete" => Some(Self::MessageDelete),
            "message_update" => Some(Self::MessageUpdate),
            "guild_member_add" => Some(Self::GuildMemberAdd),
            "guild_member_remove" => Some(Self::GuildMemberRemove),
            "guild_member_update" => Some(Self::GuildMemberUpdate),
            "guild_update" => Some(Self::GuildUpdate),
            "voice_state_update" => Some(Self::VoiceStateUpdate),
            "message_create" => Some(Self::MessageCreate),
            "channel_create" => Some(Self::ChannelCreate),
            "channel_update" => Some(Self::ChannelUpdate),
            "channel_delete" => Some(Self::ChannelDelete),
            "role_create" => Some(Self::RoleCreate),
            "role_update" => Some(Self::RoleUpdate),
            "role_delete" => Some(Self::RoleDelete),
            "guild_ban_add" => Some(Self::GuildBanAdd),
            "guild_ban_remove" => Some(Self::GuildBanRemove),
            "invite_create" => Some(Self::InviteCreate),
            "invite_delete" => Some(Self::InviteDelete),
            _ => None,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::MessageDelete,
            Self::MessageUpdate,
            Self::GuildMemberAdd,
            Self::GuildMemberRemove,
            Self::GuildMemberUpdate,
            Self::GuildUpdate,
            Self::VoiceStateUpdate,
            Self::MessageCreate,
            Self::ChannelCreate,
            Self::ChannelUpdate,
            Self::ChannelDelete,
            Self::RoleCreate,
            Self::RoleUpdate,
            Self::RoleDelete,
            Self::GuildBanAdd,
            Self::GuildBanRemove,
            Self::InviteCreate,
            Self::InviteDelete,
        ]
    }

    pub fn placeholders(&self) -> &'static [&'static str] {
        match self {
            Self::MessageDelete => &["channel_id", "message_id", "guild_id"],
            Self::MessageUpdate => &["channel_id", "message_id", "user_id", "content", "guild_id"],
            Self::GuildMemberAdd => &["user_id", "username", "guild_id"],
            Self::GuildMemberRemove => &["user_id", "username", "guild_id"],
            Self::GuildMemberUpdate => &["user_id", "username", "guild_id", "roles"],
            Self::GuildUpdate => &["guild_id", "guild_name"],
            Self::VoiceStateUpdate => &["user_id", "channel_id", "guild_id"],
            Self::MessageCreate => &["channel_id", "message_id", "user_id", "content", "guild_id"],
            Self::ChannelCreate => &["channel_id", "channel_name", "guild_id"],
            Self::ChannelUpdate => &["channel_id", "channel_name", "guild_id"],
            Self::ChannelDelete => &["channel_id", "channel_name", "guild_id"],
            Self::RoleCreate => &["role_id", "role_name", "guild_id"],
            Self::RoleUpdate => &["role_id", "role_name", "guild_id"],
            Self::RoleDelete => &["role_id", "role_name", "guild_id"],
            Self::GuildBanAdd => &["user_id", "username", "guild_id"],
            Self::GuildBanRemove => &["user_id", "username", "guild_id"],
            Self::InviteCreate => &["channel_id", "inviter_id", "code", "guild_id"],
            Self::InviteDelete => &["channel_id", "code", "guild_id"],
        }
    }
}

/// Internal Black Mesa events that the logging system can dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MesaLogEvent {
    AutomodCensor,
    AutomodSpam,
    ModerationWarn,
    ModerationMute,
    ModerationKick,
    ModerationBan,
    ModerationUnmute,
    ModerationUnban,
    ModerationPardon,
}

impl MesaLogEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutomodCensor => "mesa_automod_censor",
            Self::AutomodSpam => "mesa_automod_spam",
            Self::ModerationWarn => "mesa_moderation_warn",
            Self::ModerationMute => "mesa_moderation_mute",
            Self::ModerationKick => "mesa_moderation_kick",
            Self::ModerationBan => "mesa_moderation_ban",
            Self::ModerationUnmute => "mesa_moderation_unmute",
            Self::ModerationUnban => "mesa_moderation_unban",
            Self::ModerationPardon => "mesa_moderation_pardon",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mesa_automod_censor" => Some(Self::AutomodCensor),
            "mesa_automod_spam" => Some(Self::AutomodSpam),
            "mesa_moderation_warn" => Some(Self::ModerationWarn),
            "mesa_moderation_mute" => Some(Self::ModerationMute),
            "mesa_moderation_kick" => Some(Self::ModerationKick),
            "mesa_moderation_ban" => Some(Self::ModerationBan),
            "mesa_moderation_unmute" => Some(Self::ModerationUnmute),
            "mesa_moderation_unban" => Some(Self::ModerationUnban),
            "mesa_moderation_pardon" => Some(Self::ModerationPardon),
            _ => None,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::AutomodCensor,
            Self::AutomodSpam,
            Self::ModerationWarn,
            Self::ModerationMute,
            Self::ModerationKick,
            Self::ModerationBan,
            Self::ModerationUnmute,
            Self::ModerationUnban,
            Self::ModerationPardon,
        ]
    }

    pub fn placeholders(&self) -> &'static [&'static str] {
        match self {
            Self::AutomodCensor => &[
                "user_id", "username", "channel_id", "guild_id",
                "reason", "filter_type", "offending_content",
            ],
            Self::AutomodSpam => &[
                "user_id", "username", "channel_id", "guild_id",
                "reason", "spam_type", "count", "interval",
            ],
            Self::ModerationWarn => &[
                "user_id", "username", "moderator_id", "guild_id",
                "reason", "duration", "infraction_id",
            ],
            Self::ModerationMute => &[
                "user_id", "username", "moderator_id", "guild_id",
                "reason", "duration", "infraction_id",
            ],
            Self::ModerationKick => &[
                "user_id", "username", "moderator_id", "guild_id",
                "reason", "infraction_id",
            ],
            Self::ModerationBan => &[
                "user_id", "username", "moderator_id", "guild_id",
                "reason", "duration", "infraction_id",
            ],
            Self::ModerationUnmute => &[
                "user_id", "username", "moderator_id", "guild_id", "reason",
            ],
            Self::ModerationUnban => &[
                "user_id", "username", "moderator_id", "guild_id", "reason",
            ],
            Self::ModerationPardon => &[
                "user_id", "username", "moderator_id", "guild_id",
                "reason", "infraction_id",
            ],
        }
    }
}

/// Per-event logging configuration stored in the `log_configs` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub guild_id: Id,
    pub event: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<Id>,
    pub embed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_footer: Option<String>,
}

/// Description of all supported log events, keyed by db event string.
/// Used by the dashboard to display available events and their placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEventInfo {
    pub key: String,
    pub label: String,
    pub source: String,
    pub placeholders: Vec<String>,
}
