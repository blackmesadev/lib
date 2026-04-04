pub mod automod;
pub mod config;
pub mod infraction;
pub mod logging;
pub mod mesastream;
pub mod permissions;

pub use config::Config;
pub use infraction::{Infraction, InfractionType};
pub use logging::LogConfig;
pub use permissions::{Permission, PermissionGroup};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnixTimestamp(u64);

impl UnixTimestamp {
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp() as u64)
    }

    pub fn as_secs(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Uuid(uuid::Uuid);

impl Default for Uuid {
    fn default() -> Self {
        Self::new()
    }
}

impl Uuid {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Option<Self> {
        uuid::Uuid::parse_str(s).map(Self).ok()
    }

    pub fn inner(&self) -> &uuid::Uuid {
        &self.0
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<uuid::Uuid> for Uuid {
    fn from(id: uuid::Uuid) -> Self {
        Self(id)
    }
}
