use crate::discord::{Id, Permissions, Role};
use std::fmt;

use bitflags::{bitflags, Flags};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionOverride {
    pub groups: Vec<String>,
    pub roles: Vec<Id>,
    pub users: Vec<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGroup {
    pub name: String,
    pub roles: Vec<Id>,
    pub users: Vec<Id>,
    pub permissions: Permission,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Permission: u64 {
        // Moderation
        const MODERATION_KICK   = 1 << 0;
        const MODERATION_BAN    = 1 << 1;
        const MODERATION_UNBAN  = 1 << 2;
        const MODERATION_MUTE   = 1 << 3;
        const MODERATION_UNMUTE = 1 << 4;
        const MODERATION_WARN   = 1 << 5;
        const MODERATION_PARDON = 1 << 6;
        const MODERATION_PURGE  = 1 << 7;
        const MODERATION_LOOKUP = 1 << 8;

        // Music
        const MUSIC_PLAY    = 1 << 9;
        const MUSIC_SKIP    = 1 << 10;
        const MUSIC_STOP    = 1 << 11;
        const MUSIC_PAUSE   = 1 << 12;
        const MUSIC_RESUME  = 1 << 13;
        const MUSIC_CLEAR   = 1 << 14;
        const MUSIC_VOLUME  = 1 << 15;
        const MUSIC_SHUFFLE = 1 << 16;

        // Config
        const CONFIG_VIEW = 1 << 17;
        const CONFIG_EDIT = 1 << 18;

        // Infraction
        const INFRACTION_VIEW = 1 << 19;
        const INFRACTION_EDIT = 1 << 20;

        // Utility
        const UTILITY_INFO       = 1 << 21;
        const UTILITY_USERINFO   = 1 << 22;
        const UTILITY_SERVERINFO = 1 << 23;
        const UTILITY_HELP       = 1 << 24;
        const UTILITY_PING       = 1 << 25;
        const UTILITY_INVITE     = 1 << 26;
        const UTILITY_SELFLOOKUP = 1 << 27;

        // Category composites
        const MODERATION = Self::MODERATION_KICK.bits() | Self::MODERATION_BAN.bits()
            | Self::MODERATION_UNBAN.bits() | Self::MODERATION_MUTE.bits()
            | Self::MODERATION_UNMUTE.bits() | Self::MODERATION_WARN.bits()
            | Self::MODERATION_PARDON.bits() | Self::MODERATION_PURGE.bits()
            | Self::MODERATION_LOOKUP.bits();

        const MUSIC = Self::MUSIC_PLAY.bits() | Self::MUSIC_SKIP.bits()
            | Self::MUSIC_STOP.bits() | Self::MUSIC_PAUSE.bits()
            | Self::MUSIC_RESUME.bits() | Self::MUSIC_CLEAR.bits()
            | Self::MUSIC_VOLUME.bits() | Self::MUSIC_SHUFFLE.bits();

        const CONFIG = Self::CONFIG_VIEW.bits() | Self::CONFIG_EDIT.bits();

        const INFRACTION = Self::INFRACTION_VIEW.bits() | Self::INFRACTION_EDIT.bits();

        const UTILITY = Self::UTILITY_INFO.bits() | Self::UTILITY_USERINFO.bits()
            | Self::UTILITY_SERVERINFO.bits() | Self::UTILITY_HELP.bits()
            | Self::UTILITY_PING.bits() | Self::UTILITY_INVITE.bits()
            | Self::UTILITY_SELFLOOKUP.bits();

        const ALL = Self::MODERATION.bits() | Self::MUSIC.bits()
            | Self::CONFIG.bits() | Self::INFRACTION.bits()
            | Self::UTILITY.bits();
    }
}

impl Default for Permission {
    fn default() -> Self {
        Self::empty()
    }
}

impl Permission {
    /// Parse a case-insensitive permission name (e.g. `"moderation_kick"` or `"ALL"`).
    /// Delegates to the native bitflags `from_name` after uppercasing.
    pub fn from_str(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        Self::from_name(&upper)
    }

    /// All named permissions (categories + leaves) for enumeration.
    pub fn all_permissions_vec() -> Vec<Self> {
        Self::FLAGS.iter().map(|f| *f.value()).collect()
    }

    /// Raw u64 bits - for storing in the database as BIGINT.
    #[inline]
    pub fn to_bits(self) -> u64 {
        self.bits()
    }

    /// Reconstruct from raw bits (ignores unknown bits).
    #[inline]
    pub fn from_bits_safe(bits: u64) -> Self {
        Self::from_bits_truncate(bits)
    }

    #[inline]
    pub fn has_permission(self, perm: Permission) -> bool {
        self.contains(perm)
    }

    /// Create a PermissionSet from Discord role permissions.
    pub fn from_discord_permissions(roles: &[Role], present: &Vec<Id>) -> Self {
        let perms = roles
            .iter()
            .filter(|role| present.contains(&role.id))
            .map(|role| role.permissions)
            .fold(Permissions::empty(), |acc, perm| acc | perm);

        if perms.contains(Permissions::ADMINISTRATOR) {
            return Self::ALL;
        }

        let mut set = Self::empty();

        if perms.contains(Permissions::KICK_MEMBERS) {
            set |= Self::MODERATION_KICK;
        }
        if perms.contains(Permissions::BAN_MEMBERS) {
            set |= Self::MODERATION_BAN | Self::MODERATION_UNBAN;
        }
        if perms.contains(Permissions::MANAGE_MESSAGES) {
            set |= Self::MODERATION_MUTE
                | Self::MODERATION_UNMUTE
                | Self::MODERATION_WARN
                | Self::MODERATION_PARDON;
        }
        if perms.contains(Permissions::MANAGE_CHANNELS | Permissions::MANAGE_MESSAGES) {
            set |= Self::MODERATION_PURGE;
        }
        if perms.contains(Permissions::CONNECT) {
            set |= Self::MUSIC_PLAY;
        }
        if perms.contains(Permissions::MUTE_MEMBERS)
            || perms.contains(Permissions::DEAFEN_MEMBERS)
            || perms.contains(Permissions::MOVE_MEMBERS)
        {
            set |= Self::MUSIC_STOP
                | Self::MUSIC_PAUSE
                | Self::MUSIC_RESUME
                | Self::MUSIC_CLEAR
                | Self::MUSIC_VOLUME
                | Self::MUSIC_SHUFFLE
                | Self::MUSIC_SKIP;
        }
        if perms.contains(Permissions::MANAGE_CHANNELS) {
            set |= Self::CONFIG_EDIT | Self::INFRACTION_EDIT;
        }
        if perms.contains(Permissions::VIEW_CHANNEL) {
            set |= Self::CONFIG_VIEW | Self::INFRACTION_VIEW | Self::UTILITY;
        }

        set
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        bitflags::parser::to_writer(self, f)
    }
}

// Custom Serialize/Deserialize to support both integer and string representations
impl Serialize for Permission {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Always serialize as u64 for compatibility with database and frontend
        serializer.serialize_u64(self.bits())
    }
}

impl<'de> Deserialize<'de> for Permission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PermissionSetVisitor;

        impl<'de> serde::de::Visitor<'de> for PermissionSetVisitor {
            type Value = Permission;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a u64 integer or a string with | separated permission flags")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Permission, E>
            where
                E: serde::de::Error,
            {
                // Accept raw integer bits
                Ok(Permission::from_bits_truncate(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Permission, E>
            where
                E: serde::de::Error,
            {
                // Accept signed integers too, convert to unsigned
                if value < 0 {
                    return Err(E::custom("permission bits cannot be negative"));
                }
                Ok(Permission::from_bits_truncate(value as u64))
            }

            fn visit_str<E>(self, value: &str) -> Result<Permission, E>
            where
                E: serde::de::Error,
            {
                // Accept string representation like "MODERATION_KICK | CONFIG_VIEW"
                bitflags::parser::from_str(value)
                    .map_err(|e| E::custom(format!("invalid permission flags: {}", e)))
            }
        }

        deserializer.deserialize_any(PermissionSetVisitor)
    }
}

impl PermissionGroup {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            roles: Vec::new(),
            users: Vec::new(),
            permissions: Default::default(),
        }
    }
}
