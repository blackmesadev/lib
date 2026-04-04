use super::Database;
use sqlx::Row;
use tracing::instrument;

use crate::{discord::Id, model::LogConfig};

pub use super::DbError;

impl Database {
    #[instrument(name = "db_get_log_configs", skip(self), fields(guild_id = %guild_id))]
    pub async fn get_log_configs(&self, guild_id: &Id) -> Result<Vec<LogConfig>, DbError> {
        let rows = sqlx::query_as::<_, LogConfig>(
            "SELECT * FROM log_configs WHERE guild_id = $1 ORDER BY event",
        )
        .bind(guild_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    #[instrument(name = "db_get_log_config_for_event", skip(self), fields(guild_id = %guild_id, event = %event))]
    pub async fn get_log_config_for_event(
        &self,
        guild_id: &Id,
        event: &str,
    ) -> Result<Option<LogConfig>, DbError> {
        let row = sqlx::query_as::<_, LogConfig>(
            "SELECT * FROM log_configs WHERE guild_id = $1 AND event = $2 AND enabled = true",
        )
        .bind(guild_id.get() as i64)
        .bind(event)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    #[instrument(name = "db_upsert_log_config", skip(self, config), fields(guild_id = %config.guild_id, event = %config.event))]
    pub async fn upsert_log_config(&self, config: &LogConfig) -> Result<LogConfig, DbError> {
        let row = sqlx::query_as::<_, LogConfig>(
            r#"INSERT INTO log_configs
                (guild_id, event, enabled, channel_id, embed, text_content,
                 embed_title, embed_body, embed_color, embed_footer)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (guild_id, event)
            DO UPDATE SET
                enabled = EXCLUDED.enabled,
                channel_id = EXCLUDED.channel_id,
                embed = EXCLUDED.embed,
                text_content = EXCLUDED.text_content,
                embed_title = EXCLUDED.embed_title,
                embed_body = EXCLUDED.embed_body,
                embed_color = EXCLUDED.embed_color,
                embed_footer = EXCLUDED.embed_footer
            RETURNING *"#,
        )
        .bind(config.guild_id.get() as i64)
        .bind(&config.event)
        .bind(config.enabled)
        .bind(config.channel_id.map(|v| v.get() as i64))
        .bind(config.embed)
        .bind(&config.text_content)
        .bind(&config.embed_title)
        .bind(&config.embed_body)
        .bind(config.embed_color.map(|v| v as i32))
        .bind(&config.embed_footer)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    #[instrument(name = "db_delete_log_config", skip(self), fields(guild_id = %guild_id, event = %event))]
    pub async fn delete_log_config(
        &self,
        guild_id: &Id,
        event: &str,
    ) -> Result<bool, DbError> {
        let result =
            sqlx::query("DELETE FROM log_configs WHERE guild_id = $1 AND event = $2")
                .bind(guild_id.get() as i64)
                .bind(event)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected() > 0)
    }

    #[instrument(name = "db_bulk_upsert_log_configs", skip(self, configs), fields(guild_id = %guild_id))]
    pub async fn bulk_upsert_log_configs(
        &self,
        guild_id: &Id,
        configs: &[LogConfig],
    ) -> Result<Vec<LogConfig>, DbError> {
        let mut results = Vec::with_capacity(configs.len());
        for config in configs {
            results.push(self.upsert_log_config(config).await?);
        }
        Ok(results)
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LogConfig {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get::<i64, _>("id").ok(),
            guild_id: Id::new(row.try_get::<i64, _>("guild_id")? as u64),
            event: row.try_get("event")?,
            enabled: row.try_get("enabled")?,
            channel_id: row
                .try_get::<Option<i64>, _>("channel_id")?
                .map(|v| Id::new(v as u64)),
            embed: row.try_get("embed")?,
            text_content: row.try_get("text_content")?,
            embed_title: row.try_get("embed_title")?,
            embed_body: row.try_get("embed_body")?,
            embed_color: row
                .try_get::<Option<i32>, _>("embed_color")?
                .map(|v| v as u32),
            embed_footer: row.try_get("embed_footer")?,
        })
    }
}
