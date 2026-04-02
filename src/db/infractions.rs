use super::Database;
use sqlx::Row;

use tracing::instrument;

use crate::model::automod::AutomodOffense;
use crate::model::InfractionType;
use crate::{
    discord::Id,
    model::{Infraction, Uuid},
};

pub use super::DbError;

impl Database {
    #[instrument(
        name = "db_create_infraction",
        skip(self, infraction),
        fields(
            guild_id = %infraction.guild_id,
            infraction_id = %infraction.uuid,
            infraction_type = %infraction.infraction_type
        )
    )]
    pub async fn create_infraction(&self, infraction: &Infraction) -> Result<(), DbError> {
        let automod_json = infraction
            .automod_offense
            .as_ref()
            .map(|o| serde_json::to_value(o).unwrap());

        sqlx::query(
            r#"INSERT INTO infractions
                (id, guild_id, user_id, moderator_id, infraction_type, reason,
                 last_edited, expires_at, mute_role_id, automod_offense, active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        )
        .bind(infraction.uuid.inner())
        .bind(infraction.guild_id.get() as i64)
        .bind(infraction.user_id.get() as i64)
        .bind(infraction.moderator_id.get() as i64)
        .bind(infraction.infraction_type.to_str())
        .bind(&infraction.reason)
        .bind(infraction.last_edited.map(|v| v as i64))
        .bind(infraction.expires_at.map(|v| v as i64))
        .bind(infraction.mute_role_id.map(|id| id.get() as i64))
        .bind(automod_json)
        .bind(infraction.active)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[instrument(
        name = "db_get_infraction",
        skip(self),
        fields(
            guild_id = %guild_id,
            infraction_id = %id
        )
    )]
    pub async fn get_infraction(
        &self,
        guild_id: &Id,
        id: &Uuid,
    ) -> Result<Option<Infraction>, DbError> {
        sqlx::query_as::<_, Infraction>("SELECT * FROM infractions WHERE id = $1 AND guild_id = $2")
            .bind(id.inner())
            .bind(guild_id.get() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    #[instrument(
        name = "db_delete_infraction",
        skip(self),
        fields(
            guild_id = %guild_id,
            infraction_id = %id
        )
    )]
    pub async fn delete_infraction(
        &self,
        guild_id: &Id,
        id: &Uuid,
    ) -> Result<Option<Infraction>, DbError> {
        sqlx::query_as::<_, Infraction>(
            "DELETE FROM infractions WHERE id = $1 AND guild_id = $2 RETURNING *",
        )
        .bind(id.inner())
        .bind(guild_id.get() as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[instrument(
        name = "db_get_infractions",
        skip(self),
        fields(
            guild_id = %guild_id,
            user_id = ?user_id,
            infraction_type = ?typ,
            active = ?active
        )
    )]
    pub async fn get_infractions(
        &self,
        guild_id: &Id,
        user_id: Option<&Id>,
        typ: Option<InfractionType>,
        active: Option<bool>,
    ) -> Result<Vec<Infraction>, DbError> {
        sqlx::query_as::<_, Infraction>(
            r#"SELECT * FROM infractions
               WHERE guild_id = $1
                 AND ($2::BIGINT IS NULL OR user_id = $2)
                 AND ($3::TEXT IS NULL OR infraction_type = $3)
                 AND ($4::BOOL IS NULL OR active = $4)"#,
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.map(|id| id.get() as i64))
        .bind(typ.map(|t| t.to_str().to_owned()))
        .bind(active)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[instrument(
        name = "db_get_active_infractions",
        skip(self),
        fields(
            guild_id = %guild_id,
            user_id = %user_id,
            infraction_type = ?typ
        )
    )]
    pub async fn get_active_infractions(
        &self,
        guild_id: &Id,
        user_id: &Id,
        typ: Option<InfractionType>,
    ) -> Result<Vec<Infraction>, DbError> {
        sqlx::query_as::<_, Infraction>(
            r#"SELECT * FROM infractions
               WHERE guild_id = $1
                 AND user_id = $2
                 AND active = true
                 AND ($3::TEXT IS NULL OR infraction_type = $3)"#,
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .bind(typ.map(|t| t.to_str().to_owned()))
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[instrument(
        name = "db_deactivate_infraction",
        skip(self, id),
        fields(
            infraction_id = %id
        )
    )]
    pub async fn deactivate_infraction(&self, id: &Uuid) -> Result<bool, DbError> {
        let result =
            sqlx::query("UPDATE infractions SET active = false WHERE id = $1 AND active = true")
                .bind(id.inner())
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_expired_infractions(&self) -> Result<Vec<Infraction>, DbError> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query_as::<_, Infraction>(
            "SELECT * FROM infractions WHERE expires_at <= $1 AND active = true",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for Infraction {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let automod_offense = row
            .try_get::<Option<serde_json::Value>, _>("automod_offense")?
            .and_then(|v| serde_json::from_value::<AutomodOffense>(v).ok());

        let infraction_type_str: String = row.try_get("infraction_type")?;
        let infraction_type = InfractionType::from_str(&infraction_type_str).ok_or_else(|| {
            sqlx::Error::Decode(format!("invalid infraction_type: {infraction_type_str}").into())
        })?;

        Ok(Self {
            uuid: Uuid::from(row.try_get::<uuid::Uuid, _>("id")?),
            guild_id: Id::new(row.try_get::<i64, _>("guild_id")? as u64),
            user_id: Id::new(row.try_get::<i64, _>("user_id")? as u64),
            moderator_id: Id::new(row.try_get::<i64, _>("moderator_id")? as u64),
            infraction_type,
            reason: row.try_get("reason")?,
            last_edited: row
                .try_get::<Option<i64>, _>("last_edited")?
                .map(|v| v as u64),
            expires_at: row
                .try_get::<Option<i64>, _>("expires_at")?
                .map(|v| v as u64),
            mute_role_id: row
                .try_get::<Option<i64>, _>("mute_role_id")?
                .map(|v| Id::new(v as u64)),
            automod_offense,
            active: row.try_get("active")?,
        })
    }
}
