//! PostgreSQL: пул соединений и встроенные миграции.
//!
//! Миграции — SQL-файлы в `server/migrations`, вшитые в бинарник. Каждая
//! применяется в своей транзакции под advisory-блокировкой, так что два экземпляра
//! сервера не применят её дважды. Сервер со схемой новее своей не стартует.

use deadpool_postgres::{Config, Pool, PoolConfig, Runtime};
use tokio_postgres::NoTls;

/// Миграция: номер (по возрастанию, без пропусков), имя и SQL.
pub struct Migration {
    pub version: i32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Миграции сервера по порядку. Каждая задача этапа 3 добавляет свою.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "accounts",
    sql: include_str!("../migrations/0001_accounts.sql"),
}];

/// Ключ advisory-блокировки миграций (произвольная константа).
const MIGRATION_LOCK: i64 = 0x5354_4159_415f_4442; // "STAYA_DB"

#[derive(Debug)]
pub enum DbError {
    Pool(String),
    Postgres(tokio_postgres::Error),
    /// В базе применена миграция, которой этот бинарник не знает.
    SchemaTooNew {
        applied: i32,
        known: i32,
    },
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pool(e) => write!(f, "database pool: {e}"),
            Self::Postgres(e) => write!(f, "database: {e}"),
            Self::SchemaTooNew { applied, known } => write!(
                f,
                "database schema version {applied} is newer than this server knows ({known})"
            ),
        }
    }
}

impl std::error::Error for DbError {}

impl From<tokio_postgres::Error> for DbError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Postgres(e)
    }
}

impl From<deadpool_postgres::PoolError> for DbError {
    fn from(e: deadpool_postgres::PoolError) -> Self {
        Self::Pool(e.to_string())
    }
}

/// Пул по URL вида `postgres://user:password@host:port/db`. Без TLS: Postgres в
/// той же сети Docker Compose, наружу не открыт.
pub fn pool(url: &str, max_size: usize) -> Result<Pool, DbError> {
    let mut cfg = Config::new();
    cfg.url = Some(url.to_owned());
    cfg.application_name = Some("staya-server".into());
    cfg.pool = Some(PoolConfig::new(max_size));
    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| DbError::Pool(e.to_string()))
}

/// Применяет недостающие миграции. Возвращает номер версии схемы.
///
/// Всё — одной транзакцией под advisory-блокировкой: два экземпляра сервера,
/// стартующие одновременно, не столкнутся, а сбой любой миграции откатывает весь
/// запуск (DDL в PostgreSQL транзакционный).
pub async fn migrate(pool: &Pool, migrations: &[Migration]) -> Result<i32, DbError> {
    let mut client = pool.get().await?;
    let tx = client.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK])
        .await?;
    tx.batch_execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    integer PRIMARY KEY,
            name       text NOT NULL,
            applied_at timestamptz NOT NULL DEFAULT now()
        )",
    )
    .await?;
    let applied: i32 = tx
        .query_one(
            "SELECT COALESCE(max(version), 0) FROM schema_migrations",
            &[],
        )
        .await?
        .get(0);
    let known = migrations.last().map_or(0, |m| m.version);
    if applied > known {
        return Err(DbError::SchemaTooNew { applied, known });
    }
    for m in migrations.iter().filter(|m| m.version > applied) {
        tx.batch_execute(m.sql).await?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name) VALUES ($1, $2)",
            &[&m.version, &m.name],
        )
        .await?;
        tracing::info!(version = m.version, name = m.name, "migration applied");
    }
    tx.commit().await?;
    Ok(known.max(applied))
}

/// База отвечает. Для `/health`.
pub async fn ping(pool: &Pool) -> bool {
    match pool.get().await {
        Ok(client) => client.simple_query("SELECT 1").await.is_ok(),
        Err(_) => false,
    }
}
