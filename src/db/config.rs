use super::Database;
use sqlx::Row;

use tracing::instrument;

use crate::{
    discord::Id,
    model::{
        config::Config,
        permissions::{PermissionGroup, Permission},
    },
};

pub use super::DbError;

impl Database {
    #[instrument(name = "db_list_guild_ids", skip(self))]
    pub async fn list_guild_ids(&self) -> Result<Vec<Id>, DbError> {
        let rows = sqlx::query_scalar::<_, i64>(
            r#"SELECT id
               FROM configs
               ORDER BY id"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|id| Id::new(id as u64)).collect())
    }

    #[instrument(
        name = "db_get_config",
        skip(self),
        fields(
            guild_id = %guild_id
        )
    )]
    pub async fn get_config(&self, guild_id: &Id) -> Result<Option<Config>, DbError> {
        let guild_id_i64 = guild_id.get() as i64;

        let config_fut = sqlx::query_as::<_, Config>("SELECT * FROM configs WHERE id = $1")
            .bind(guild_id_i64)
            .fetch_optional(&self.pool);

        let groups_fut = sqlx::query_as::<_, PermissionGroup>(
            "SELECT name, permissions, roles, users FROM permissions WHERE guild_id = $1 ORDER BY id",
        )
        .bind(guild_id_i64)
        .fetch_all(&self.pool);

        let (config, groups) = tokio::try_join!(config_fut, groups_fut)?;

        let Some(mut config) = config else {
            return Ok(None);
        };

        config.permission_groups = if groups.is_empty() {
            None
        } else {
            Some(groups)
        };

        Ok(Some(config))
    }

    #[instrument(
        name = "db_update_config",
        skip(self, config),
        fields(
            guild_id = %config.id
        )
    )]
    pub async fn update_config(&self, guild_id: &Id, config: &Config) -> Result<Config, DbError> {
        let command_aliases = to_json_opt(&config.command_aliases)?;
        let automod = to_json_opt(&config.automod)?;

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
                   command_aliases = $10,
                   automod = $11,
                   automod_enabled = $12,
                   music_enabled = $13,
                   moderation_enabled = $14,
                   logging_enabled = $15
               WHERE id = $1"#,
        )
        .bind(guild_id.get() as i64)
        .bind(&config.prefix)
        .bind(config.mute_role.map(|v| v.get() as i64))
        .bind(config.default_warn_duration.map(|v| v as i64))
        .bind(config.log_channel.map(|v| v.get() as i64))
        .bind(config.prefer_embeds)
        .bind(config.inherit_discord_perms)
        .bind(config.alert_on_infraction)
        .bind(config.send_permission_denied)
        .bind(command_aliases)
        .bind(automod)
        .bind(config.automod_enabled)
        .bind(config.music_enabled)
        .bind(config.moderation_enabled)
        .bind(config.logging_enabled)
        .execute(&self.pool)
        .await?;

        self.sync_permission_groups(guild_id, &config.permission_groups)
            .await?;

        Ok(config.clone())
    }

    #[instrument(
        name = "db_create_config",
        skip(self, config),
        fields(
            guild_id = %config.id
        )
    )]
    pub async fn create_config(&self, config: &Config) -> Result<Config, DbError> {
        let command_aliases = to_json_opt(&config.command_aliases)?;
        let automod = to_json_opt(&config.automod)?;

        sqlx::query(
            r#"INSERT INTO configs (
                   id, prefix, mute_role, default_warn_duration, log_channel,
                   prefer_embeds, inherit_discord_perms, alert_on_infraction,
                   send_permission_denied, command_aliases, automod,
                   automod_enabled, music_enabled, moderation_enabled,
                   logging_enabled
               ) VALUES (
                   $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
               )"#,
        )
        .bind(config.id.get() as i64)
        .bind(&config.prefix)
        .bind(config.mute_role.map(|v| v.get() as i64))
        .bind(config.default_warn_duration.map(|v| v as i64))
        .bind(config.log_channel.map(|v| v.get() as i64))
        .bind(config.prefer_embeds)
        .bind(config.inherit_discord_perms)
        .bind(config.alert_on_infraction)
        .bind(config.send_permission_denied)
        .bind(command_aliases)
        .bind(automod)
        .bind(config.automod_enabled)
        .bind(config.music_enabled)
        .bind(config.moderation_enabled)
        .bind(config.logging_enabled)
        .execute(&self.pool)
        .await?;

        if let Some(groups) = &config.permission_groups {
            self.insert_permission_groups(&config.id, groups).await?;
        }

        Ok(config.clone())
    }

    async fn sync_permission_groups(
        &self,
        guild_id: &Id,
        groups: &Option<Vec<PermissionGroup>>,
    ) -> Result<(), DbError> {
        sqlx::query("DELETE FROM permissions WHERE guild_id = $1")
            .bind(guild_id.get() as i64)
            .execute(&self.pool)
            .await?;

        if let Some(groups) = groups {
            self.insert_permission_groups(guild_id, groups).await?;
        }

        Ok(())
    }

    pub async fn list_guilds_for_user(
        &self,
        user_id: &Id,
        user_guild_ids: &[Id],
        user_role_ids: &[Id],
        permission: Permission,
    ) -> Result<Vec<Id>, DbError> {
        let guild_ids: Vec<i64> = user_guild_ids.iter().map(|id| id.get() as i64).collect();
        let role_ids: Vec<i64> = user_role_ids.iter().map(|id| id.get() as i64).collect();

        // When we know the exact guilds the user is a member of, bound the scan to
        // those guilds so Postgres avoids a full permissions-table sweep.
        let rows = if guild_ids.is_empty() {
            // Fallback: no cached guild membership - only direct user-id matches.
            sqlx::query_scalar::<_, i64>(
                "SELECT DISTINCT guild_id FROM permissions \
                 WHERE $1 = ANY(users) \
                 AND (permissions & $2) != 0",
            )
            .bind(user_id.get() as i64)
            .bind(permission.bits() as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT DISTINCT guild_id FROM permissions \
                 WHERE guild_id = ANY($4) \
                 AND ($1 = ANY(users) OR roles && $2) \
                 AND (permissions & $3) != 0",
            )
            .bind(user_id.get() as i64)
            .bind(&role_ids)
            .bind(permission.bits() as i64)
            .bind(&guild_ids)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().map(|id| Id::new(id as u64)).collect())
    }

    async fn insert_permission_groups(
        &self,
        guild_id: &Id,
        groups: &[PermissionGroup],
    ) -> Result<(), DbError> {
        if groups.is_empty() {
            return Ok(());
        }

        let guild_id_i64 = guild_id.get() as i64;
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO permissions (guild_id, name, permissions, roles, users) ",
        );
        qb.push_values(groups, |mut b, group| {
            b.push_bind(guild_id_i64)
                .push_bind(&group.name)
                .push_bind(group.permissions.bits() as i64)
                .push_bind(
                    group
                        .roles
                        .iter()
                        .map(|id| id.get() as i64)
                        .collect::<Vec<_>>(),
                )
                .push_bind(
                    group
                        .users
                        .iter()
                        .map(|id| id.get() as i64)
                        .collect::<Vec<_>>(),
                );
        });
        qb.build().execute(&self.pool).await?;

        Ok(())
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for Config {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let automod = row
            .try_get::<Option<serde_json::Value>, _>("automod")?
            .map(|v| serde_json::from_value(v))
            .transpose()
            .map_err(|e: serde_json::Error| sqlx::Error::Decode(e.into()))?;

        Ok(Self {
            id: Id::new(row.try_get::<i64, _>("id")? as u64),
            prefix: row.try_get("prefix")?,
            mute_role: row
                .try_get::<Option<i64>, _>("mute_role")?
                .map(|v| Id::new(v as u64)),
            default_warn_duration: row
                .try_get::<Option<i64>, _>("default_warn_duration")?
                .map(|v| v as u64),
            log_channel: row
                .try_get::<Option<i64>, _>("log_channel")?
                .map(|v| Id::new(v as u64)),
            prefer_embeds: row.try_get("prefer_embeds")?,
            inherit_discord_perms: row.try_get("inherit_discord_perms")?,
            alert_on_infraction: row.try_get("alert_on_infraction")?,
            send_permission_denied: row.try_get("send_permission_denied")?,
            permission_groups: None,
            command_aliases: row
                .try_get::<Option<serde_json::Value>, _>("command_aliases")?
                .and_then(|v| serde_json::from_value(v).ok()),
            automod_enabled: row.try_get("automod_enabled")?,
            logging_enabled: row.try_get("logging_enabled")?,
            music_enabled: row.try_get("music_enabled")?,
            moderation_enabled: row.try_get("moderation_enabled")?,
            automod,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for PermissionGroup {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            name: row.try_get("name")?,
            permissions: Permission::from_bits_retain(
                row.try_get::<i64, _>("permissions")? as u64
            ),
            roles: row
                .try_get::<Vec<i64>, _>("roles")?
                .into_iter()
                .map(|id| Id::new(id as u64))
                .collect(),
            users: row
                .try_get::<Vec<i64>, _>("users")?
                .into_iter()
                .map(|id| Id::new(id as u64))
                .collect(),
        })
    }
}

fn to_json_opt<T: serde::Serialize>(
    value: &Option<T>,
) -> Result<Option<serde_json::Value>, DbError> {
    value
        .as_ref()
        .map(|value| {
            serde_json::to_value(value)
                .map_err(|e| DbError::Protocol(format!("Failed to serialize: {e}")))
        })
        .transpose()
}
