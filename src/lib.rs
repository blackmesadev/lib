mod config;

pub const LIB_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod cache;
pub mod clients;
pub mod db;
pub mod discord;
pub mod emojis;
pub mod model;
pub mod permissions;
pub mod util;
