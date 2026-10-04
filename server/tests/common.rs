//! Отдельная база PostgreSQL на каждый тест.
//!
//! URL сервера — `STAYA_TEST_DATABASE_URL` (например,
//! `postgres://staya:staya@127.0.0.1:54329/staya`). Без него тесты с базой
//! пропускаются локально, но падают в CI — чтобы не «пройти», ничего не проверив.

#![allow(dead_code)]

use deadpool_postgres::Pool;
use staya_server::db;
use tokio_postgres::NoTls;

pub struct TestDb {
    admin_url: String,
    name: String,
    pub url: String,
    pub pool: Pool,
}

fn base_url() -> Option<String> {
    match std::env::var("STAYA_TEST_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ if std::env::var_os("CI").is_some() => {
            panic!("STAYA_TEST_DATABASE_URL must be set in CI")
        }
        _ => {
            eprintln!("STAYA_TEST_DATABASE_URL is not set: skipping database test");
            None
        }
    }
}

/// URL той же базы-сервера, но с другим именем базы.
fn with_db(url: &str, name: &str) -> String {
    let (prefix, _) = url.rsplit_once('/').expect("database URL with /dbname");
    format!("{prefix}/{name}")
}

async fn admin(url: &str) -> tokio_postgres::Client {
    let (client, conn) = tokio_postgres::connect(url, NoTls).await.expect("connect");
    tokio::spawn(conn);
    client
}

impl TestDb {
    pub async fn new() -> Option<Self> {
        let admin_url = base_url()?;
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).unwrap();
        let name = format!(
            "staya_test_{}",
            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        admin(&admin_url)
            .await
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .expect("create test database");
        let url = with_db(&admin_url, &name);
        let pool = db::pool(&url, 4).unwrap();
        Some(Self {
            admin_url,
            name,
            url,
            pool,
        })
    }

    pub async fn drop(self) {
        self.pool.close();
        admin(&self.admin_url)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("drop test database");
    }
}
