mod config;
mod infractions;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::instrument;

pub use sqlx::Error as DbError;

pub struct Database {
    pool: PgPool,
}

impl Database {
    #[instrument(name = "db_connect", skip(connection_string))]
    pub async fn connect(connection_string: String) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .min_connections(2)
            .max_connections(10)
            .connect(&connection_string)
            .await?;

        Ok(Self { pool })
    }

    /// Run the embedded migrations on startup.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!().run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
