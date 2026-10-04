//! Сквозной тест (задача 3.7): два настоящих ядра дружат и обмениваются позициями
//! через настоящий сервер и PostgreSQL. Затем вся база и журнал сервера
//! проверяются на известные тестовые координаты во всех представлениях: строкой,
//! целым ×10⁷, f64/f32, в обоих порядках байтов. Найтись не должно ничего.
//!
//! Нужен `STAYA_TEST_DATABASE_URL` (см. CLAUDE.md); без него тест пропускается
//! локально и падает в CI.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dev_peer::{Exchange, Peer};
use staya_server::db;
use staya_server::http::{AppState, Config, app};
use tokio_postgres::NoTls;

const DOMAIN: &str = "staya.test";
// Москва и Санкт-Петербург, градусы × 10⁷.
const ALICE: (i32, i32) = (557_558_000, 376_173_000);
const BOB: (i32, i32) = (599_386_000, 303_141_000);

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn base_url() -> Option<String> {
    match std::env::var("STAYA_TEST_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ if std::env::var_os("CI").is_some() => {
            panic!("STAYA_TEST_DATABASE_URL must be set in CI")
        }
        _ => None,
    }
}

async fn admin(url: &str) -> tokio_postgres::Client {
    let (client, conn) = tokio_postgres::connect(url, NoTls).await.unwrap();
    tokio::spawn(conn);
    client
}

/// Все представления координаты, которые могли бы утечь.
fn needles(e7: i32) -> Vec<String> {
    let deg = f64::from(e7) / 1e7;
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    vec![
        e7.to_string(),
        format!("{deg:.4}"),
        format!("{deg:.4}").replace('.', ","),
        hex(&e7.to_be_bytes()),
        hex(&e7.to_le_bytes()),
        hex(&deg.to_be_bytes()),
        hex(&deg.to_le_bytes()),
        hex(&(deg as f32).to_be_bytes()),
        hex(&(deg as f32).to_le_bytes()),
    ]
}

/// Поиск не пустой: образцы находят координаты в настоящем открытом тексте пакета.
#[test]
fn needles_match_real_plaintext() {
    use staya_proto::location::{LocationKind, LocationPayload};
    let plain = LocationPayload {
        kind: LocationKind::Exact,
        lat_e7: ALICE.0,
        lon_e7: ALICE.1,
        accuracy_m: 10,
        timestamp: 1_700_000_000,
    }
    .encode()
    .unwrap();
    let hex: String = plain.iter().map(|x| format!("{x:02x}")).collect();
    for e7 in [ALICE.0, ALICE.1] {
        assert!(
            needles(e7).iter().any(|n| hex.contains(n)),
            "{e7} not found in {hex}"
        );
    }
}

#[test]
fn two_cores_through_real_server_leave_no_coordinates() {
    let Some(admin_url) = base_url() else {
        eprintln!("STAYA_TEST_DATABASE_URL is not set: skipping");
        return;
    };
    let log = Log::default();
    let writer = log.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish(),
    )
    .unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let name = format!("staya_e2e_{}", std::process::id());
    let (prefix, _) = admin_url.rsplit_once('/').unwrap();
    let url = format!("{prefix}/{name}");
    let (base, pool) = rt.block_on(async {
        let a = admin(&admin_url).await;
        a.batch_execute(&format!("DROP DATABASE IF EXISTS {name}"))
            .await
            .unwrap();
        a.batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        let pool = db::pool(&url, 8).unwrap();
        db::migrate(&pool, db::MIGRATIONS).await.unwrap();
        let state = AppState::new(
            pool.clone(),
            Config {
                domain: DOMAIN.into(),
                invite_code: None,
                trust_proxy: false,
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (format!("http://{addr}"), pool)
    });

    let mut alice = Peer::new(&base).unwrap();
    let mut bob = Peer::new(&base).unwrap();
    alice.login(DOMAIN).unwrap();
    bob.login(DOMAIN).unwrap();
    alice.publish_keys().unwrap();
    bob.publish_keys().unwrap();
    alice.set_nick("Алиса").unwrap();
    bob.accept(&alice.create_invite(DOMAIN).unwrap()).unwrap();

    let run = |peer: Peer, mine, expect| {
        std::thread::spawn(move || {
            Exchange {
                mine,
                expect,
                timeout: Duration::from_secs(60),
                linger: Duration::from_secs(3),
            }
            .run(&peer)
            .map(|()| peer)
            .map_err(|e| e.to_string())
        })
    };
    let a = run(alice, ALICE, BOB);
    let b = run(bob, BOB, ALICE);
    let alice = a.join().unwrap().expect("alice got bob's location");
    let bob = b.join().unwrap().expect("bob got alice's location");
    assert!(alice.has_friends().unwrap() && bob.has_friends().unwrap());

    // Вся база текстом: каждая строка каждой таблицы, bytea — в hex.
    let dump = rt.block_on(async {
        let client = pool.get().await.unwrap();
        let tables: Vec<String> = client
            .query(
                "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
                &[],
            )
            .await
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect();
        assert!(tables.contains(&"location_slots".to_owned()), "{tables:?}");
        let slots: i64 = client
            .query_one("SELECT count(*) FROM location_slots", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(slots, 2, "both positions went through the database");
        let mut dump = String::new();
        for t in &tables {
            for row in client
                .query(&format!("SELECT t::text FROM {t} t"), &[])
                .await
                .unwrap()
            {
                dump.push_str(&row.get::<_, String>(0));
                dump.push('\n');
            }
        }
        dump.to_lowercase()
    });
    assert!(
        dump.len() > 10_000,
        "dump looks empty: {} bytes",
        dump.len()
    );
    let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    for e7 in [ALICE.0, ALICE.1, BOB.0, BOB.1] {
        for needle in needles(e7) {
            assert!(!dump.contains(&needle), "database leaks {needle}");
            assert!(!log.contains(&needle), "server log leaks {needle}");
        }
    }
    assert!(!dump.contains("алиса"), "nick must be end-to-end encrypted");

    rt.block_on(async {
        pool.close();
        admin(&admin_url)
            .await
            .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .await
            .unwrap();
    });
}
