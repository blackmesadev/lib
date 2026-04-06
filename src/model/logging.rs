use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{discord::Id, model::Uuid};

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

/// Typed event data passed to the logging system.
/// Each variant carries exactly the fields needed for its template placeholders,
/// removing the need to manually build a `HashMap` at every call site.
#[derive(Debug)]
pub enum LogEvent {
    // Moderation
    ModerationKick {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
        infraction_id: Uuid,
    },
    ModerationBan {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
        duration: Option<u64>,
        infraction_id: Uuid,
    },
    ModerationMute {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
        duration: Option<u64>,
        infraction_id: Uuid,
    },
    ModerationWarn {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
        duration: Option<u64>,
        infraction_id: Uuid,
    },
    ModerationUnban {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
    },
    ModerationUnmute {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
    },
    ModerationPardon {
        guild_id: Id,
        user_id: Id,
        moderator_id: Id,
        reason: String,
        infraction_id: Uuid,
    },
    // -- Automod --
    AutomodSpam {
        guild_id: Id,
        user_id: Id,
        username: String,
        channel_id: Id,
        reason: String,
        spam_type: String,
        count: u64,
        interval: u64,
    },
    AutomodCensor {
        guild_id: Id,
        user_id: Id,
        username: String,
        channel_id: Id,
        reason: String,
        filter_type: String,
        offending_content: String,
    },
    // -- Discord gateway
    GuildUpdate {
        guild_id: Id,
        guild_name: String,
    },
    GuildMemberAdd {
        guild_id: Id,
        user_id: Id,
        username: String,
    },
    GuildMemberUpdate {
        guild_id: Id,
        user_id: Id,
        username: String,
        roles: Vec<Id>,
    },
    GuildMemberRemove {
        guild_id: Id,
        user_id: Id,
        username: String,
    },
    ChannelCreate {
        guild_id: Id,
        channel_id: Id,
        channel_name: String,
    },
    ChannelUpdate {
        guild_id: Id,
        channel_id: Id,
        channel_name: String,
    },
    ChannelDelete {
        guild_id: Id,
        channel_id: Id,
        channel_name: String,
    },
    MessageDelete {
        guild_id: Id,
        channel_id: Id,
        message_id: Id,
    },
    RoleCreate {
        guild_id: Id,
        role_id: Id,
        role_name: String,
    },
    RoleUpdate {
        guild_id: Id,
        role_id: Id,
        role_name: String,
    },
    RoleDelete {
        guild_id: Id,
        role_id: Id,
    },
    GuildBanAdd {
        guild_id: Id,
        user_id: Id,
        username: String,
    },
    GuildBanRemove {
        guild_id: Id,
        user_id: Id,
        username: String,
    },
    InviteCreate {
        guild_id: Id,
        channel_id: Id,
        code: String,
        inviter_id: Option<Id>,
    },
    InviteDelete {
        guild_id: Id,
        channel_id: Id,
        code: String,
    },
}

impl LogEvent {
    /// The guild this event belongs to.
    pub fn guild_id(&self) -> &Id {
        match self {
            Self::ModerationKick { guild_id, .. }
            | Self::ModerationBan { guild_id, .. }
            | Self::ModerationMute { guild_id, .. }
            | Self::ModerationWarn { guild_id, .. }
            | Self::ModerationUnban { guild_id, .. }
            | Self::ModerationUnmute { guild_id, .. }
            | Self::ModerationPardon { guild_id, .. }
            | Self::AutomodSpam { guild_id, .. }
            | Self::AutomodCensor { guild_id, .. }
            | Self::GuildUpdate { guild_id, .. }
            | Self::GuildMemberAdd { guild_id, .. }
            | Self::GuildMemberUpdate { guild_id, .. }
            | Self::GuildMemberRemove { guild_id, .. }
            | Self::ChannelCreate { guild_id, .. }
            | Self::ChannelUpdate { guild_id, .. }
            | Self::ChannelDelete { guild_id, .. }
            | Self::MessageDelete { guild_id, .. }
            | Self::RoleCreate { guild_id, .. }
            | Self::RoleUpdate { guild_id, .. }
            | Self::RoleDelete { guild_id, .. }
            | Self::GuildBanAdd { guild_id, .. }
            | Self::GuildBanRemove { guild_id, .. }
            | Self::InviteCreate { guild_id, .. }
            | Self::InviteDelete { guild_id, .. } => guild_id,
        }
    }

    /// The associated `LogEventType` used for DB lookups.
    pub fn event_type(&self) -> LogEventType {
        match self {
            Self::ModerationKick { .. } => LogEventType::Mesa(MesaLogEvent::ModerationKick),
            Self::ModerationBan { .. } => LogEventType::Mesa(MesaLogEvent::ModerationBan),
            Self::ModerationMute { .. } => LogEventType::Mesa(MesaLogEvent::ModerationMute),
            Self::ModerationWarn { .. } => LogEventType::Mesa(MesaLogEvent::ModerationWarn),
            Self::ModerationUnban { .. } => LogEventType::Mesa(MesaLogEvent::ModerationUnban),
            Self::ModerationUnmute { .. } => LogEventType::Mesa(MesaLogEvent::ModerationUnmute),
            Self::ModerationPardon { .. } => LogEventType::Mesa(MesaLogEvent::ModerationPardon),
            Self::AutomodSpam { .. } => LogEventType::Mesa(MesaLogEvent::AutomodSpam),
            Self::AutomodCensor { .. } => LogEventType::Mesa(MesaLogEvent::AutomodCensor),
            Self::GuildUpdate { .. } => LogEventType::Discord(DiscordLogEvent::GuildUpdate),
            Self::GuildMemberAdd { .. } => LogEventType::Discord(DiscordLogEvent::GuildMemberAdd),
            Self::GuildMemberUpdate { .. } => LogEventType::Discord(DiscordLogEvent::GuildMemberUpdate),
            Self::GuildMemberRemove { .. } => LogEventType::Discord(DiscordLogEvent::GuildMemberRemove),
            Self::ChannelCreate { .. } => LogEventType::Discord(DiscordLogEvent::ChannelCreate),
            Self::ChannelUpdate { .. } => LogEventType::Discord(DiscordLogEvent::ChannelUpdate),
            Self::ChannelDelete { .. } => LogEventType::Discord(DiscordLogEvent::ChannelDelete),
            Self::MessageDelete { .. } => LogEventType::Discord(DiscordLogEvent::MessageDelete),
            Self::RoleCreate { .. } => LogEventType::Discord(DiscordLogEvent::RoleCreate),
            Self::RoleUpdate { .. } => LogEventType::Discord(DiscordLogEvent::RoleUpdate),
            Self::RoleDelete { .. } => LogEventType::Discord(DiscordLogEvent::RoleDelete),
            Self::GuildBanAdd { .. } => LogEventType::Discord(DiscordLogEvent::GuildBanAdd),
            Self::GuildBanRemove { .. } => LogEventType::Discord(DiscordLogEvent::GuildBanRemove),
            Self::InviteCreate { .. } => LogEventType::Discord(DiscordLogEvent::InviteCreate),
            Self::InviteDelete { .. } => LogEventType::Discord(DiscordLogEvent::InviteDelete),
        }
    }

    /// Consume the event and produce the template placeholder map.
    pub fn into_vars(self) -> HashMap<String, String> {
        // Reduce per-entry noise: keys stay as &str literals, values call Display.
        macro_rules! vars {
            ($($key:literal => $val:expr),* $(,)?) => {{
                let mut m = HashMap::new();
                $( m.insert($key.to_owned(), $val.to_string()); )*
                m
            }};
        }

        match self {
            Self::ModerationKick { guild_id, user_id, moderator_id, reason, infraction_id } => vars! {
                "guild_id"      => guild_id,
                "user_id"       => user_id,
                "username"      => format!("<@{user_id}>"),
                "moderator_id"  => moderator_id,
                "reason"        => reason,
                "infraction_id" => infraction_id,
            },
            Self::ModerationBan { guild_id, user_id, moderator_id, reason, duration, infraction_id } => vars! {
                "guild_id"      => guild_id,
                "user_id"       => user_id,
                "username"      => format!("<@{user_id}>"),
                "moderator_id"  => moderator_id,
                "reason"        => reason,
                "duration"      => duration.map(|d| d.to_string()).unwrap_or_else(|| "Permanent".into()),
                "infraction_id" => infraction_id,
            },
            Self::ModerationMute { guild_id, user_id, moderator_id, reason, duration, infraction_id } => vars! {
                "guild_id"      => guild_id,
                "user_id"       => user_id,
                "username"      => format!("<@{user_id}>"),
                "moderator_id"  => moderator_id,
                "reason"        => reason,
                "duration"      => duration.map(|d| d.to_string()).unwrap_or_else(|| "Permanent".into()),
                "infraction_id" => infraction_id,
            },
            Self::ModerationWarn { guild_id, user_id, moderator_id, reason, duration, infraction_id } => vars! {
                "guild_id"      => guild_id,
                "user_id"       => user_id,
                "username"      => format!("<@{user_id}>"),
                "moderator_id"  => moderator_id,
                "reason"        => reason,
                "duration"      => duration.map(|d| d.to_string()).unwrap_or_else(|| "Permanent".into()),
                "infraction_id" => infraction_id,
            },
            Self::ModerationUnban { guild_id, user_id, moderator_id, reason } => vars! {
                "guild_id"     => guild_id,
                "user_id"      => user_id,
                "username"     => format!("<@{user_id}>"),
                "moderator_id" => moderator_id,
                "reason"       => reason,
            },
            Self::ModerationUnmute { guild_id, user_id, moderator_id, reason } => vars! {
                "guild_id"     => guild_id,
                "user_id"      => user_id,
                "username"     => format!("<@{user_id}>"),
                "moderator_id" => moderator_id,
                "reason"       => reason,
            },
            Self::ModerationPardon { guild_id, user_id, moderator_id, reason, infraction_id } => vars! {
                "guild_id"      => guild_id,
                "user_id"       => user_id,
                "username"      => format!("<@{user_id}>"),
                "moderator_id"  => moderator_id,
                "reason"        => reason,
                "infraction_id" => infraction_id,
            },
            Self::AutomodSpam { guild_id, user_id, username, channel_id, reason, spam_type, count, interval } => vars! {
                "guild_id"   => guild_id,
                "user_id"    => user_id,
                "username"   => username,
                "channel_id" => channel_id,
                "reason"     => reason,
                "spam_type"  => spam_type,
                "count"      => count,
                "interval"   => interval,
            },
            Self::AutomodCensor { guild_id, user_id, username, channel_id, reason, filter_type, offending_content } => vars! {
                "guild_id"          => guild_id,
                "user_id"           => user_id,
                "username"          => username,
                "channel_id"        => channel_id,
                "reason"            => reason,
                "filter_type"       => filter_type,
                "offending_content" => offending_content,
            },
            Self::GuildUpdate { guild_id, guild_name } => vars! {
                "guild_id"   => guild_id,
                "guild_name" => guild_name,
            },
            Self::GuildMemberAdd { guild_id, user_id, username }
            | Self::GuildMemberRemove { guild_id, user_id, username } => vars! {
                "guild_id" => guild_id,
                "user_id"  => user_id,
                "username" => username,
            },
            Self::GuildMemberUpdate { guild_id, user_id, username, roles } => vars! {
                "guild_id" => guild_id,
                "user_id"  => user_id,
                "username" => username,
                "roles"    => roles.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(","),
            },
            Self::ChannelCreate { guild_id, channel_id, channel_name }
            | Self::ChannelUpdate { guild_id, channel_id, channel_name }
            | Self::ChannelDelete { guild_id, channel_id, channel_name } => vars! {
                "guild_id"     => guild_id,
                "channel_id"   => channel_id,
                "channel_name" => channel_name,
            },
            Self::MessageDelete { guild_id, channel_id, message_id } => vars! {
                "guild_id"   => guild_id,
                "channel_id" => channel_id,
                "message_id" => message_id,
            },
            Self::RoleCreate { guild_id, role_id, role_name }
            | Self::RoleUpdate { guild_id, role_id, role_name } => vars! {
                "guild_id"  => guild_id,
                "role_id"   => role_id,
                "role_name" => role_name,
            },
            Self::RoleDelete { guild_id, role_id } => vars! {
                "guild_id"  => guild_id,
                "role_id"   => role_id,
                "role_name" => "",
            },
            Self::GuildBanAdd { guild_id, user_id, username }
            | Self::GuildBanRemove { guild_id, user_id, username } => vars! {
                "guild_id" => guild_id,
                "user_id"  => user_id,
                "username" => username,
            },
            Self::InviteCreate { guild_id, channel_id, code, inviter_id } => vars! {
                "guild_id"   => guild_id,
                "channel_id" => channel_id,
                "code"       => code,
                "inviter_id" => inviter_id.map(|u| u.to_string()).unwrap_or_default(),
            },
            Self::InviteDelete { guild_id, channel_id, code } => vars! {
                "guild_id"   => guild_id,
                "channel_id" => channel_id,
                "code"       => code,
            },
        }
    }
}
