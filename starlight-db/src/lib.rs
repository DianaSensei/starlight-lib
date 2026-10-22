//! PostgreSQL-compatible database access (ADR-0006, ADR-0017): pooling, tenant isolation, error classification.

pub mod migration;

use std::time::Duration;

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbBackend,
    DbErr, SqlErr, Statement, TransactionTrait,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use starlight_core::TenantId;
use starlight_core::health::{HealthCheck, HealthFuture};

pub use sea_orm;

/// Session setting read by row-level security policies.
pub const TENANT_SETTING: &str = "app.tenant_id";

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: SecretString,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_timeout_secs")]
    pub acquire_timeout_secs: u64,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_max_lifetime_secs")]
    pub max_lifetime_secs: u64,
    /// Logs every SQL statement; leave off outside local debugging.
    #[serde(default)]
    pub sqlx_logging: bool,
}

fn default_max_connections() -> u32 {
    10
}

fn default_min_connections() -> u32 {
    1
}

fn default_timeout_secs() -> u64 {
    5
}

fn default_idle_timeout_secs() -> u64 {
    300
}

fn default_max_lifetime_secs() -> u64 {
    1800
}

pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(config.url.expose_secret().to_owned());
    options
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .max_lifetime(Duration::from_secs(config.max_lifetime_secs))
        .sqlx_logging(config.sqlx_logging);
    Database::connect(options).await
}

/// Transaction whose statements only see rows of `tenant_id` under row-level security.
pub async fn begin_tenant(
    db: &DatabaseConnection,
    tenant_id: TenantId,
) -> Result<DatabaseTransaction, DbErr> {
    let transaction = db.begin().await?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT set_config($1, $2, true)",
            [TENANT_SETTING.into(), tenant_id.to_string().into()],
        ))
        .await?;
    Ok(transaction)
}

pub fn is_unique_violation(error: &DbErr) -> bool {
    matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

pub fn is_foreign_key_violation(error: &DbErr) -> bool {
    matches!(
        error.sql_err(),
        Some(SqlErr::ForeignKeyConstraintViolation(_))
    )
}

/// Name of the violated unique constraint, so callers can map it to a precise error code.
pub fn violated_unique_constraint(error: &DbErr) -> Option<String> {
    let Some(SqlErr::UniqueConstraintViolation(message)) = error.sql_err() else {
        return None;
    };
    message.split('"').nth(1).map(str::to_owned)
}

/// Readiness probe that pings the pool.
pub struct DatabaseHealth(pub DatabaseConnection);

impl HealthCheck for DatabaseHealth {
    fn name(&self) -> &'static str {
        "database"
    }

    fn check(&self) -> HealthFuture<'_> {
        Box::pin(async move { self.0.ping().await.map_err(|error| error.to_string()) })
    }
}
