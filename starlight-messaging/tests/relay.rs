//! Requires PostgreSQL (non-superuser owner) and a NATS server with JetStream:
//! `STARLIGHT_TEST_DATABASE_URL=... STARLIGHT_TEST_NATS_URL=nats://127.0.0.1:4222 cargo test -- --include-ignored`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use futures_util::StreamExt;
use sea_orm::{ConnectionTrait, TransactionTrait};
use secrecy::SecretString;
use starlight_core::TenantId;
use starlight_db::{DatabaseConfig, connect};
use starlight_messaging::async_nats::jetstream;
use starlight_messaging::{
    CloudEvent, NewEvent, OUTBOX_TABLE_SQL, OutboxRelay, RelayConfig, enqueue, ensure_stream,
};

#[tokio::test]
#[ignore = "requires STARLIGHT_TEST_DATABASE_URL and STARLIGHT_TEST_NATS_URL"]
async fn committed_events_are_published_once_and_rolled_back_events_never() {
    let database_url = std::env::var("STARLIGHT_TEST_DATABASE_URL").unwrap();
    let nats_url = std::env::var("STARLIGHT_TEST_NATS_URL").unwrap();
    let db = connect(&DatabaseConfig {
        url: SecretString::from(database_url),
        max_connections: 2,
        min_connections: 1,
        connect_timeout_secs: 5,
        acquire_timeout_secs: 5,
        idle_timeout_secs: 60,
        max_lifetime_secs: 600,
        sqlx_logging: false,
    })
    .await
    .unwrap();
    db.execute_unprepared(OUTBOX_TABLE_SQL).await.unwrap();
    db.execute_unprepared("DELETE FROM outbox_events")
        .await
        .unwrap();

    let client = starlight_messaging::async_nats::connect(&nats_url)
        .await
        .unwrap();
    let context = jetstream::new(client.clone());
    let module = format!("relaytest{}", uuid::Uuid::now_v7().simple());
    ensure_stream(
        &context,
        &module.to_uppercase(),
        vec![format!("starlight.{module}.>")],
    )
    .await
    .unwrap();
    let subject: &'static str =
        Box::leak(format!("starlight.{module}.thing.created.v1").into_boxed_str());
    let mut subscription = client.subscribe(subject).await.unwrap();
    client.flush().await.unwrap();

    let tenant = TenantId::generate();
    let committed = db.begin().await.unwrap();
    enqueue(
        &committed,
        "/starlight/test",
        &NewEvent::new(subject, "t-1", tenant, serde_json::json!({ "n": 1 })),
    )
    .await
    .unwrap();
    committed.commit().await.unwrap();
    let rolled_back = db.begin().await.unwrap();
    enqueue(
        &rolled_back,
        "/starlight/test",
        &NewEvent::new(subject, "t-2", tenant, serde_json::json!({ "n": 2 })),
    )
    .await
    .unwrap();
    rolled_back.rollback().await.unwrap();

    let relay = OutboxRelay::new(db.clone(), context, RelayConfig::default());
    let first = relay.publish_batch().await.unwrap();
    let second = relay.publish_batch().await.unwrap();

    let message = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .unwrap()
        .unwrap();
    let event: CloudEvent = serde_json::from_slice(&message.payload).unwrap();
    let message_id = message
        .headers
        .as_ref()
        .and_then(|headers| headers.get("Nats-Msg-Id"))
        .unwrap();

    assert_eq!(first, 1);
    assert_eq!(second, 0);
    assert_eq!(event.subject, "t-1");
    assert_eq!(event.tenantid, tenant.as_uuid());
    assert_eq!(message_id.as_str(), event.id.to_string());
    assert!(
        tokio::time::timeout(Duration::from_millis(300), subscription.next())
            .await
            .is_err()
    );
}
