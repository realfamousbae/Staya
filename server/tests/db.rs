//! Миграции на настоящем PostgreSQL.

mod common;

use common::TestDb;
use staya_server::db::{self, DbError, Migration};

const M1: Migration = Migration {
    version: 1,
    name: "first",
    sql: "CREATE TABLE t1 (id integer PRIMARY KEY);",
};
const M2: Migration = Migration {
    version: 2,
    name: "second",
    sql: "ALTER TABLE t1 ADD COLUMN note text;",
};

#[tokio::test]
async fn migrations_apply_once_and_in_order() {
    let Some(t) = TestDb::new().await else { return };
    assert_eq!(db::migrate(&t.pool, &[M1]).await.unwrap(), 1);
    // Повторный запуск ничего не делает, следующая миграция докатывается.
    assert_eq!(db::migrate(&t.pool, &[M1]).await.unwrap(), 1);
    assert_eq!(db::migrate(&t.pool, &[M1, M2]).await.unwrap(), 2);
    let client = t.pool.get().await.unwrap();
    client
        .execute("INSERT INTO t1 (id, note) VALUES (1, 'x')", &[])
        .await
        .unwrap();
    drop(client);
    t.drop().await;
}

#[tokio::test]
async fn refuses_newer_schema() {
    let Some(t) = TestDb::new().await else { return };
    db::migrate(&t.pool, &[M1, M2]).await.unwrap();
    // Старый бинарник против новой схемы — отказ, а не тихая работа.
    match db::migrate(&t.pool, &[M1]).await {
        Err(DbError::SchemaTooNew {
            applied: 2,
            known: 1,
        }) => {}
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
    t.drop().await;
}

#[tokio::test]
async fn failed_migration_rolls_back() {
    let Some(t) = TestDb::new().await else { return };
    let broken = Migration {
        version: 2,
        name: "broken",
        sql: "CREATE TABLE t2 (id integer); SELECT no_such_function();",
    };
    assert!(db::migrate(&t.pool, &[M1, broken]).await.is_err());
    let client = t.pool.get().await.unwrap();
    let t2: i64 = client
        .query_one(
            "SELECT count(*) FROM information_schema.tables WHERE table_name = 't2'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(t2, 0, "half-applied migration must be rolled back");
    let journal: bool = client
        .query_one("SELECT to_regclass('schema_migrations') IS NULL", &[])
        .await
        .unwrap()
        .get(0);
    assert!(journal, "the whole run is rolled back, M1 included");
    drop(client);
    assert_eq!(db::migrate(&t.pool, &[M1]).await.unwrap(), 1);
    t.drop().await;
}

#[tokio::test]
async fn concurrent_servers_migrate_safely() {
    let Some(t) = TestDb::new().await else { return };
    let (a, b) = tokio::join!(
        db::migrate(&t.pool, &[M1, M2]),
        db::migrate(&t.pool, &[M1, M2])
    );
    assert_eq!((a.unwrap(), b.unwrap()), (2, 2));
    t.drop().await;
}

#[tokio::test]
async fn ping_reports_database_state() {
    let Some(t) = TestDb::new().await else { return };
    assert!(db::ping(&t.pool).await);
    let pool = t.pool.clone();
    t.drop().await;
    assert!(!db::ping(&pool).await);
}
