use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};

use crate::event::{CloudEvent, NewEvent};

/// Outbox table shared by every service; include it in the service's first migration.
pub const OUTBOX_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS outbox_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    subject text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz,
    attempts integer NOT NULL DEFAULT 0,
    last_error text
);
CREATE INDEX IF NOT EXISTS idx_outbox_events__pending ON outbox_events (created_at) WHERE published_at IS NULL;";

/// Stores `event` in the caller's transaction so it is published only if the transaction commits.
pub async fn enqueue<C: ConnectionTrait>(
    connection: &C,
    source: &str,
    event: &NewEvent,
) -> Result<(), DbErr> {
    let envelope = CloudEvent::from_new(source, event);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| DbErr::Custom(error.to_string()))?;
    connection
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO outbox_events (id, tenant_id, subject, payload) VALUES ($1, $2, $3, $4)",
            [
                envelope.id.into(),
                envelope.tenantid.into(),
                envelope.event_type.clone().into(),
                payload.into(),
            ],
        ))
        .await?;
    Ok(())
}
