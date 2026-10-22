use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use async_nats::HeaderMap;
use async_nats::jetstream::{self, stream};
use opentelemetry::metrics::Counter;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use serde::Deserialize;
use starlight_telemetry::OpenTelemetrySpanExt;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::event::CloudEvent;

const MESSAGE_ID_HEADER: &str = "Nats-Msg-Id";
const TRACEPARENT: &str = "traceparent";
const MAX_ERROR_LENGTH: usize = 500;

static PUBLISHED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    starlight_telemetry::meter("starlight-messaging")
        .u64_counter("starlight.outbox.published")
        .with_unit("{event}")
        .build()
});
static FAILED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    starlight_telemetry::meter("starlight-messaging")
        .u64_counter("starlight.outbox.failed")
        .with_unit("{event}")
        .build()
});

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    pub batch_size: u32,
    pub poll_interval_ms: u64,
    pub retention_hours: u64,
    /// Events failing this many times stay in the table with `last_error` for an operator to inspect.
    pub max_attempts: i32,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            poll_interval_ms: 500,
            retention_hours: 72,
            max_attempts: 20,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("database error: {0}")]
    Database(#[from] DbErr),
    #[error("failed to publish event {event_id}: {reason}")]
    Publish { event_id: Uuid, reason: String },
    #[error("invalid outbox payload for event {0}")]
    InvalidPayload(Uuid),
}

struct PendingEvent {
    id: Uuid,
    subject: String,
    payload: serde_json::Value,
}

/// Publishes committed outbox rows to JetStream; safe to run on every replica.
pub struct OutboxRelay {
    db: DatabaseConnection,
    jetstream: jetstream::Context,
    config: RelayConfig,
}

impl OutboxRelay {
    pub fn new(db: DatabaseConnection, jetstream: jetstream::Context, config: RelayConfig) -> Self {
        Self {
            db,
            jetstream,
            config,
        }
    }

    pub async fn run(self, shutdown: CancellationToken) {
        tracing::info!("outbox relay started");
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);
        let mut cleanup = tokio::time::interval(Duration::from_secs(3600));
        loop {
            let published = match self.publish_batch().await {
                Ok(count) => count,
                Err(error) => {
                    tracing::warn!(error.message = %error, "outbox batch failed");
                    0
                }
            };
            let has_backlog = published == self.config.batch_size as usize;
            if has_backlog {
                continue;
            }
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = cleanup.tick() => self.delete_published().await,
                () = tokio::time::sleep(poll_interval) => {}
            }
        }
        tracing::info!("outbox relay stopped");
    }

    /// Publishes one batch; rows stay locked so concurrent relays skip them.
    pub async fn publish_batch(&self) -> Result<usize, RelayError> {
        let transaction = self.db.begin().await?;
        let pending = lock_pending(
            &transaction,
            self.config.batch_size,
            self.config.max_attempts,
        )
        .await?;
        let mut published = 0;
        for event in pending {
            match self.publish(&event).await {
                Ok(()) => {
                    mark_published(&transaction, event.id).await?;
                    PUBLISHED.add(1, &[]);
                    published += 1;
                }
                Err(error) => {
                    mark_failed(&transaction, event.id, &error.to_string()).await?;
                    FAILED.add(1, &[]);
                    transaction.commit().await?;
                    return Err(error);
                }
            }
        }
        transaction.commit().await?;
        Ok(published)
    }

    async fn publish(&self, event: &PendingEvent) -> Result<(), RelayError> {
        let envelope: CloudEvent = serde_json::from_value(event.payload.clone())
            .map_err(|_| RelayError::InvalidPayload(event.id))?;
        let span = tracing::info_span!(
            "outbox.publish",
            otel.name = %format!("{} publish", event.subject),
            otel.kind = "producer",
            messaging.system = "nats",
            messaging.destination.name = %event.subject,
            messaging.message.id = %event.id,
        );
        if let Some(traceparent) = &envelope.traceparent {
            let carrier = HashMap::from([(TRACEPARENT.to_owned(), traceparent.clone())]);
            let _ = span.set_parent(starlight_telemetry::context_from_headers(&carrier));
        }

        let publish_error = |reason: String| RelayError::Publish {
            event_id: event.id,
            reason,
        };
        async {
            let mut headers = HeaderMap::new();
            headers.insert(MESSAGE_ID_HEADER, event.id.to_string().as_str());
            for (name, value) in starlight_telemetry::current_context_headers() {
                headers.insert(name.as_str(), value.as_str());
            }
            let body =
                serde_json::to_vec(&envelope).map_err(|_| RelayError::InvalidPayload(event.id))?;
            let acknowledgement = self
                .jetstream
                .publish_with_headers(event.subject.clone(), headers, body.into())
                .await
                .map_err(|error| publish_error(error.to_string()))?;
            acknowledgement
                .await
                .map_err(|error| publish_error(error.to_string()))?;
            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn delete_published(&self) {
        let statement = Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM outbox_events WHERE published_at < now() - make_interval(hours => $1)",
            [i32::try_from(self.config.retention_hours)
                .unwrap_or(i32::MAX)
                .into()],
        );
        match self.db.execute_raw(statement).await {
            Ok(result) => tracing::info!(
                deleted = result.rows_affected(),
                "published outbox events pruned"
            ),
            Err(error) => tracing::warn!(error.message = %error, "outbox pruning failed"),
        }
    }
}

async fn lock_pending(
    transaction: &DatabaseTransaction,
    limit: u32,
    max_attempts: i32,
) -> Result<Vec<PendingEvent>, DbErr> {
    let rows = transaction
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT id, subject, payload FROM outbox_events
             WHERE published_at IS NULL AND attempts < $2
             ORDER BY created_at
             LIMIT $1
             FOR UPDATE SKIP LOCKED",
            [i64::from(limit).into(), max_attempts.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(PendingEvent {
                id: row.try_get("", "id")?,
                subject: row.try_get("", "subject")?,
                payload: row.try_get("", "payload")?,
            })
        })
        .collect()
}

async fn mark_published(transaction: &DatabaseTransaction, id: Uuid) -> Result<(), DbErr> {
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE outbox_events SET published_at = now(), attempts = attempts + 1, last_error = NULL WHERE id = $1",
            [id.into()],
        ))
        .await?;
    Ok(())
}

async fn mark_failed(
    transaction: &DatabaseTransaction,
    id: Uuid,
    reason: &str,
) -> Result<(), DbErr> {
    let reason: String = reason.chars().take(MAX_ERROR_LENGTH).collect();
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE outbox_events SET attempts = attempts + 1, last_error = $2 WHERE id = $1",
            [id.into(), reason.into()],
        ))
        .await?;
    Ok(())
}

/// Creates the module's stream if missing; JetStream de-duplicates retries by `Nats-Msg-Id`.
pub async fn ensure_stream(
    jetstream: &jetstream::Context,
    name: &str,
    subjects: Vec<String>,
) -> Result<(), async_nats::Error> {
    jetstream
        .get_or_create_stream(stream::Config {
            name: name.to_owned(),
            subjects,
            storage: stream::StorageType::File,
            max_age: Duration::from_secs(7 * 24 * 3600),
            duplicate_window: Duration::from_secs(120),
            ..Default::default()
        })
        .await?;
    Ok(())
}
