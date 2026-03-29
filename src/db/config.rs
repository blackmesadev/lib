use super::Database;

use tracing::instrument;

use crate::{discord::Id, model::Config};

pub use super::DbError;

impl Database {
    #[instrument(
        name = "db_get_config",
        skip(self),
        fields(
            guild_id = %guild_id
        )
    )]
    pub async fn get_config(&self, guild_id: &Id) -> Result<Option<Config>, DbError> {
        let guild_id_i64 = guild_id.get() as i64;

        let row = sqlx::query_as::<_, ConfigRow>(
            r#"SELECT
                   id,
                   prefix,
                   mute_role,
                   default_warn_duration,
                   log_channel,
                   prefer_embeds,
                   inherit_discord_perms,
                   alert_on_infraction,
                   send_permission_denied,
                   permission_groups,
                   automod,
                   command_aliases
               FROM configs
               WHERE id = $1"#,
        )
        .bind(guild_id_i64)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            None => Ok(None),
            Some(row) => Ok(Some(Config {
                id: Id::new(row.id as u64),
                prefix: row.prefix,
                mute_role: row.mute_role.map(|v| Id::new(v as u64)),
                default_warn_duration: row.default_warn_duration.map(|v| v as u64),
                log_channel: row.log_channel.map(|v| Id::new(v as u64)),
                prefer_embeds: row.prefer_embeds,
                inherit_discord_perms: row.inherit_discord_perms,
                alert_on_infraction: row.alert_on_infraction,
                send_permission_denied: row.send_permission_denied,
                permission_groups: parse_json_opt(row.permission_groups, "permission_groups")?,
                automod: parse_json_opt(row.automod, "automod")?,
                command_aliases: parse_json_opt(row.command_aliases, "command_aliases")?,
            })),
        }
    }

    #[instrument(
        name = "db_update_config",
        skip(self, config),
        fields(
            guild_id = %config.id
        )
    )]
    pub async fn update_config(&self, guild_id: &Id, config: &Config) -> Result<(), DbError> {
        let guild_id_i64 = guild_id.get() as i64;
        let permission_groups = to_json_opt(&config.permission_groups, "permission_groups")?;
        let automod = to_json_opt(&config.automod, "automod")?;
        let command_aliases = to_json_opt(&config.command_aliases, "command_aliases")?;

        sqlx::query(
            r#"UPDATE configs
               SET
                   prefix = $2,
                   mute_role = $3,
                   default_warn_duration = $4,
                   log_channel = $5,
                   prefer_embeds = $6,
                   inherit_discord_perms = $7,
                   alert_on_infraction = $8,
                   send_permission_denied = $9,
                   permission_groups = $10,
                   automod = $11,
                   command_aliases = $12
               WHERE id = $1"#,
        )
        .bind(guild_id_i64)
        .bind(&config.prefix)
        .bind(config.mute_role.map(|v| v.get() as i64))
        .bind(config.default_warn_duration.map(|v| v as i64))
        .bind(config.log_channel.map(|v| v.get() as i64))
        .bind(config.prefer_embeds)
        .bind(config.inherit_discord_perms)
        .bind(config.alert_on_infraction)
        .bind(config.send_permission_denied)
        .bind(permission_groups)
        .bind(automod)
        .bind(command_aliases)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[instrument(
        name = "db_create_config",
        skip(self, config),
        fields(
            guild_id = %config.id
        )
    )]
    pub async fn create_config(&self, config: &Config) -> Result<(), DbError> {
        let guild_id_i64 = config.id.get() as i64;
        let permission_groups = to_json_opt(&config.permission_groups, "permission_groups")?;
        let automod = to_json_opt(&config.automod, "automod")?;
        let command_aliases = to_json_opt(&config.command_aliases, "command_aliases")?;

        sqlx::query(
            r#"INSERT INTO configs (
                   id,
                   prefix,
                   mute_role,
                   default_warn_duration,
                   log_channel,
                   prefer_embeds,
                   inherit_discord_perms,
                   alert_on_infraction,
                   send_permission_denied,
                   permission_groups,
                   automod,
                   command_aliases
               ) VALUES (
                   $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
               )"#,
        )
        .bind(guild_id_i64)
        .bind(&config.prefix)
        .bind(config.mute_role.map(|v| v.get() as i64))
        .bind(config.default_warn_duration.map(|v| v as i64))
        .bind(config.log_channel.map(|v| v.get() as i64))
        .bind(config.prefer_embeds)
        .bind(config.inherit_discord_perms)
        .bind(config.alert_on_infraction)
        .bind(config.send_permission_denied)
        .bind(permission_groups)
        .bind(automod)
        .bind(command_aliases)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct ConfigRow {
    id: i64,
    prefix: String,
    mute_role: Option<i64>,
    default_warn_duration: Option<i64>,
    log_channel: Option<i64>,
    prefer_embeds: bool,
    inherit_discord_perms: bool,
    alert_on_infraction: bool,
    send_permission_denied: bool,
    permission_groups: Option<serde_json::Value>,
    automod: Option<serde_json::Value>,
    command_aliases: Option<serde_json::Value>,
}

fn parse_json_opt<T: serde::de::DeserializeOwned>(
    value: Option<serde_json::Value>,
    field_name: &str,
) -> Result<Option<T>, DbError> {
    match value {
        None => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|e| DbError::Protocol(format!("Failed to deserialize {field_name}: {e}"))),
    }
}

fn to_json_opt<T: serde::Serialize>(
    value: &Option<T>,
    field_name: &str,
) -> Result<Option<serde_json::Value>, DbError> {
    value
        .as_ref()
        .map(|value| {
            serde_json::to_value(value)
                .map_err(|e| DbError::Protocol(format!("Failed to serialize {field_name}: {e}")))
        })
        .transpose()
}
