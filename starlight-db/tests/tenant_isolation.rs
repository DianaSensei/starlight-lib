//! Requires a PostgreSQL database owned by a non-superuser role (superusers bypass RLS):
//! `STARLIGHT_TEST_DATABASE_URL=postgres://... cargo test -- --include-ignored`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use secrecy::SecretString;
use starlight_core::TenantId;
use starlight_db::migration::enable_tenant_isolation;
use starlight_db::{
    DatabaseConfig, begin_tenant, connect, is_unique_violation, violated_unique_constraint,
};

async fn database() -> DatabaseConnection {
    let url =
        std::env::var("STARLIGHT_TEST_DATABASE_URL").expect("STARLIGHT_TEST_DATABASE_URL is set");
    let config = DatabaseConfig {
        url: SecretString::from(url),
        max_connections: 4,
        min_connections: 1,
        connect_timeout_secs: 5,
        acquire_timeout_secs: 5,
        idle_timeout_secs: 60,
        max_lifetime_secs: 600,
        sqlx_logging: false,
    };
    connect(&config).await.unwrap()
}

async fn create_table(db: &DatabaseConnection) -> String {
    let table = format!("notes_{}", uuid::Uuid::now_v7().simple());
    db.execute_unprepared(&format!(
        "CREATE TABLE {table} (id uuid PRIMARY KEY, tenant_id uuid NOT NULL, body text NOT NULL,
             CONSTRAINT uq_{table}__tenant_id_body UNIQUE (tenant_id, body));
         {isolation}",
        isolation = enable_tenant_isolation(&table)
    ))
    .await
    .unwrap();
    table
}

async fn insert(
    db: &DatabaseConnection,
    table: &str,
    tenant: TenantId,
    body: &str,
) -> Result<(), sea_orm::DbErr> {
    let transaction = begin_tenant(db, tenant).await?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("INSERT INTO {table} (id, tenant_id, body) VALUES ($1, $2, $3)"),
            [
                uuid::Uuid::now_v7().into(),
                tenant.as_uuid().into(),
                body.into(),
            ],
        ))
        .await?;
    transaction.commit().await
}

async fn count_visible(db: &DatabaseConnection, table: &str, tenant: TenantId) -> i64 {
    let transaction = begin_tenant(db, tenant).await.unwrap();
    let row = transaction
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!("SELECT count(*) AS total FROM {table}"),
        ))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "total").unwrap()
}

#[tokio::test]
#[ignore = "requires STARLIGHT_TEST_DATABASE_URL"]
async fn tenants_only_see_their_own_rows() {
    let db = database().await;
    let table = create_table(&db).await;
    let (tenant_a, tenant_b) = (TenantId::generate(), TenantId::generate());

    insert(&db, &table, tenant_a, "a-1").await.unwrap();
    insert(&db, &table, tenant_a, "a-2").await.unwrap();
    insert(&db, &table, tenant_b, "b-1").await.unwrap();

    assert_eq!(count_visible(&db, &table, tenant_a).await, 2);
    assert_eq!(count_visible(&db, &table, tenant_b).await, 1);
}

#[tokio::test]
#[ignore = "requires STARLIGHT_TEST_DATABASE_URL"]
async fn rows_cannot_be_written_for_another_tenant() {
    let db = database().await;
    let table = create_table(&db).await;
    let (caller, victim) = (TenantId::generate(), TenantId::generate());

    let transaction = begin_tenant(&db, caller).await.unwrap();
    let result = transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("INSERT INTO {table} (id, tenant_id, body) VALUES ($1, $2, 'x')"),
            [uuid::Uuid::now_v7().into(), victim.as_uuid().into()],
        ))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requires STARLIGHT_TEST_DATABASE_URL"]
async fn queries_without_tenant_context_see_nothing() {
    let db = database().await;
    let table = create_table(&db).await;
    insert(&db, &table, TenantId::generate(), "hidden")
        .await
        .unwrap();

    let transaction = db.begin().await.unwrap();
    let row = transaction
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!("SELECT count(*) AS total FROM {table}"),
        ))
        .await
        .unwrap()
        .unwrap();
    let total: i64 = row.try_get("", "total").unwrap();

    assert_eq!(total, 0);
}

#[tokio::test]
#[ignore = "requires STARLIGHT_TEST_DATABASE_URL"]
async fn unique_violation_is_classified_with_constraint_name() {
    let db = database().await;
    let table = create_table(&db).await;
    let tenant = TenantId::generate();
    insert(&db, &table, tenant, "same").await.unwrap();

    let error = insert(&db, &table, tenant, "same").await.unwrap_err();

    assert!(is_unique_violation(&error));
    assert_eq!(
        violated_unique_constraint(&error),
        Some(format!("uq_{table}__tenant_id_body"))
    );
}
