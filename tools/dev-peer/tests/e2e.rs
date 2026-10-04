//! Сквозной тест (задача 3.7): два настоящих ядра дружат и обмениваются позициями
//! через настоящий сервер и PostgreSQL. Затем вся база и журнал сервера
//! проверяются на известные тестовые координаты во всех представлениях: строкой,
//! целым ×10⁷, f64/f32, в обоих порядках байтов. Найтись не должно ничего.
//!
//!
//! Задача 4.7: весь трафик между клиентами и сервером записывается прокси и
//! проверяется так же — сырые байты и содержимое каждой строки base64 в JSON
//! (шифротекст лежит в base64, и байты координат не совпали бы с его границами).
//! Ищутся координаты, ник, аватар и токен приглашения (он не должен попадать на
//! сервер вовсе, protocol §5). Положительный контроль — тот же сканер находит всё
//! это в нарочно «утёкшем» открытом пакете.
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

/// Узнаваемый «аватар»: в трафике его быть не должно (профиль шифруется Olm).
const AVATAR: &[u8] = b"\xff\xd8\xffSTAYA-TEST-AVATAR-7f3a9c\x00\x11\x22\x33";

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

/// Что ищем в трафике: представления координат (строки) и двоичные образцы.
struct Needles {
    text: Vec<String>,
    bytes: Vec<Vec<u8>>,
}

fn traffic_needles(token: &[u8]) -> Needles {
    let mut text = vec!["алиса".to_owned()];
    for e7 in [ALICE.0, ALICE.1, BOB.0, BOB.1] {
        text.extend(needles(e7));
    }
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    text.push(hex(token));
    text.push(hex(AVATAR));
    Needles {
        text,
        bytes: vec![token.to_vec(), AVATAR.to_vec(), "Алиса".as_bytes().to_vec()],
    }
}

/// Сырые байты (текстом и hex) и содержимое всех строк base64 в них.
fn scan(traffic: &[u8], n: &Needles) -> Vec<String> {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    let mut views = vec![traffic.to_vec()];
    // Строки base64 длиной от 16 символов: тела JSON, параметры.
    let is_b64 = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'=' | b'-' | b'_');
    for token in traffic.split(|c| !is_b64(*c)).filter(|t| t.len() >= 16) {
        if let Ok(d) = STANDARD.decode(token) {
            views.push(d);
        }
        if let Ok(d) = URL_SAFE_NO_PAD.decode(token.trim_ascii_end()) {
            views.push(d);
        }
    }
    let mut found = Vec::new();
    for v in &views {
        let lossy = String::from_utf8_lossy(v).to_lowercase();
        let hex: String = v.iter().map(|x| format!("{x:02x}")).collect();
        for t in &n.text {
            if lossy.contains(t.as_str()) || hex.contains(t.as_str()) {
                found.push(t.clone());
            }
        }
        for b in &n.bytes {
            if v.windows(b.len()).any(|w| w == b.as_slice()) {
                found.push(format!("bytes {}", hex_of(b)));
            }
        }
    }
    found
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Положительный контроль сканера: «утёкшие» в JSON открытый пакет позиции,
/// профиль и токен находятся — значит, ноль совпадений в трафике что-то значит.
#[test]
fn traffic_scanner_finds_leaked_plaintext() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use staya_proto::location::{LocationKind, LocationPayload};
    let token = [0x5a; 16];
    let n = traffic_needles(&token);
    let payload = LocationPayload {
        kind: LocationKind::Exact,
        lat_e7: BOB.0,
        lon_e7: BOB.1,
        accuracy_m: 10,
        timestamp: 1_700_000_000,
    }
    .encode()
    .unwrap();
    for leaked in [
        payload.to_vec(),
        AVATAR.to_vec(),
        token.to_vec(),
        "Алиса".as_bytes().to_vec(),
    ] {
        // Сдвиг на 1 и 2 байта: границы base64 не совпадают с началом образца.
        for pad in 0..3 {
            let mut data = vec![0u8; pad];
            data.extend_from_slice(&leaked);
            let body = format!(
                "POST /v1/envelopes HTTP/1.1\r\n\r\n{{\"data\":\"{}\"}}",
                STANDARD.encode(&data)
            );
            assert!(
                !scan(body.as_bytes(), &n).is_empty(),
                "scanner misses {} at offset {pad}",
                hex_of(&leaked)
            );
        }
    }
    assert!(scan(b"GET /v1/mailbox HTTP/1.1\r\n\r\n{}", &n).is_empty());
}

/// TCP-прокси, который пишет всё в обе стороны.
async fn recording_proxy(upstream: SocketAddr, record: Log) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (client, _) = listener.accept().await.unwrap();
            let server = tokio::net::TcpStream::connect(upstream).await.unwrap();
            let record = record.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let (mut cr, mut cw) = client.into_split();
                let (mut sr, mut sw) = server.into_split();
                let up = {
                    let mut record = record.clone();
                    async move {
                        let mut buf = vec![0u8; 16 * 1024];
                        while let Ok(n) = cr.read(&mut buf).await {
                            if n == 0 || sw.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                            record.write_all(&buf[..n]).unwrap();
                        }
                        let _ = sw.shutdown().await;
                    }
                };
                let down = {
                    let mut record = record.clone();
                    async move {
                        let mut buf = vec![0u8; 16 * 1024];
                        while let Ok(n) = sr.read(&mut buf).await {
                            if n == 0 || cw.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                            record.write_all(&buf[..n]).unwrap();
                        }
                        let _ = cw.shutdown().await;
                    }
                };
                tokio::join!(up, down);
            });
        }
    });
    addr
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
    let traffic = Log::default();
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
        // Клиенты ходят через записывающий прокси (4.7): весь трафик — на проверку.
        let proxy = recording_proxy(addr, traffic.clone()).await;
        (format!("http://{proxy}"), pool)
    });

    let mut alice = Peer::new(&base).unwrap();
    let mut bob = Peer::new(&base).unwrap();
    alice.login(DOMAIN, None).unwrap();
    bob.login(DOMAIN, None).unwrap();
    alice.publish_keys().unwrap();
    bob.publish_keys().unwrap();
    alice.set_profile("Алиса", AVATAR.to_vec()).unwrap();
    let invite = alice.create_invite(DOMAIN).unwrap();
    let token = {
        use base64::Engine;
        let t = invite
            .split(['?', '&'])
            .find_map(|kv| kv.strip_prefix("t="))
            .unwrap();
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(t)
            .unwrap()
    };
    bob.accept(&invite).unwrap();

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

    // Трафик: позиции и профиль прошли через прокси, а найтись не должно ничего.
    let traffic = traffic.0.lock().unwrap().clone();
    assert!(
        traffic.len() > 20_000,
        "traffic looks empty: {} bytes",
        traffic.len()
    );
    let text = String::from_utf8_lossy(&traffic);
    assert!(
        text.contains("POST /v1/envelopes"),
        "envelopes went through the proxy"
    );
    let leaks = scan(&traffic, &traffic_needles(&token));
    assert!(leaks.is_empty(), "traffic leaks: {leaks:?}");

    rt.block_on(async {
        pool.close();
        admin(&admin_url)
            .await
            .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .await
            .unwrap();
    });
}
