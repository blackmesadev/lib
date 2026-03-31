use super::automod::Automod;
use crate::{discord::Id, model::permissions::PermissionGroup};

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub id: Id, // Guild ID
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_role: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_warn_duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_channel: Option<Id>,
    #[serde(default)]
    pub prefer_embeds: bool,
    #[serde(default = "default_true")]
    pub inherit_discord_perms: bool,
    #[serde(default = "default_true")]
    pub alert_on_infraction: bool,
    #[serde(default = "default_true")]
    pub send_permission_denied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_groups: Option<Vec<PermissionGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_aliases: Option<HashMap<String, String>>,
    #[serde(default)]
    pub automod_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automod: Option<Automod>,
    #[serde(default)]
    pub music_enabled: bool,
    #[serde(default)]
    pub moderation_enabled: bool,
}

#[inline]
fn default_prefix() -> String {
    "!".to_string()
}

#[inline]
fn default_true() -> bool {
    true
}

impl Config {
    pub fn new(id: &Id) -> Self {
        Self {
            id: id.clone(),
            prefix: "!".to_string(),
            mute_role: None,
            default_warn_duration: None,
            log_channel: None,
            prefer_embeds: false,
            inherit_discord_perms: true,
            alert_on_infraction: true,
            send_permission_denied: true,
            permission_groups: None,
            automod_enabled: false,
            music_enabled: false,
            moderation_enabled: false,
            automod: None,
            command_aliases: None,
        }
    }
}
