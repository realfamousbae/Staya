//! Тестовый собеседник: ядро + HTTP к dev-серверу, как это делает платформа (§8.2).

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use staya_core::api::{CoreEvent, InviteMethod, StayaCore};
use staya_core::friends::Location;
use staya_proto::api::{B64, ClaimRequest, KeyCountResponse};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Peer {
    _dir: Option<tempfile::TempDir>,
    core: Arc<StayaCore>,
    base: String,
    pub id: String,
    agent: ureq::Agent,
    /// Токен сессии настоящего сервера; без него — dev-сервер (`Bearer <account_id>`).
    token: Option<String>,
}

impl Peer {
    /// Новый аккаунт во временной базе; `base` — адрес dev-сервера без `/` на конце.
    pub fn new(base: &str) -> Result<Self, Error> {
        let dir = tempfile::tempdir()?;
        let mut key = vec![0u8; 32];
        getrandom::fill(&mut key).map_err(|e| e.to_string())?;
        let path = dir.path().join("staya.db").to_string_lossy().into_owned();
        Self::with_core(base, Some(dir), StayaCore::open(path, key)?)
    }

    /// Аккаунт, переживающий перезапуск: база и ключ в `state` (тестовый друг на
    /// настоящем сервере). Ключ тестового аккаунта — файлом с правами 600: это
    /// инструмент разработки, не приложение.
    pub fn open(base: &str, state: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(state)?;
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700))?;
        let key_path = state.join("db-key");
        let key = match std::fs::read(&key_path) {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut key = vec![0u8; 32];
                getrandom::fill(&mut key).map_err(|e| e.to_string())?;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&key_path)?
                    .write_all(&key)?;
                key
            }
            Err(e) => return Err(e.into()),
        };
        let path = state.join("staya.db").to_string_lossy().into_owned();
        Self::with_core(base, None, StayaCore::open(path, key)?)
    }

    fn with_core(
        base: &str,
        dir: Option<tempfile::TempDir>,
        core: Arc<StayaCore>,
    ) -> Result<Self, Error> {
        let id = core.identity()?.account_id;
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();
        Ok(Self {
            _dir: dir,
            core,
            base: base.trim_end_matches('/').to_owned(),
            id,
            agent,
            token: None,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token.as_deref().unwrap_or(&self.id))
    }

    /// Настоящий сервер: регистрация и вход по подписи (protocol §4.1–4.2).
    /// `domain` — имя сервера (оно же — привязка аккаунта и имя в подписи
    /// входа); `invite_code` — код приглашения закрытого сервера.
    pub fn login(&mut self, domain: &str, invite_code: Option<String>) -> Result<(), Error> {
        self.core.set_server(domain.to_owned(), Vec::new())?;
        let register = self.core.register_request(invite_code)?;
        let r = self
            .agent
            .post(self.url("/v1/accounts"))
            .header("Content-Type", "application/json")
            .send(&register)?;
        Self::check(r, "/v1/accounts")?;
        let r = self
            .agent
            .post(self.url("/v1/auth/challenge"))
            .header("Content-Type", "application/json")
            .send(&self.core.auth_challenge_request()?)?;
        let challenge = Self::check(r, "/v1/auth/challenge")?;
        let r = self
            .agent
            .post(self.url("/v1/auth/verify"))
            .header("Content-Type", "application/json")
            .send(&self.core.auth_verify_request(challenge)?)?;
        self.core
            .complete_login(Self::check(r, "/v1/auth/verify")?)?;
        let token = self
            .core
            .session_token(now())?
            .ok_or("no session after login")?;
        self.token = Some(base64_std(&token));
        Ok(())
    }

    fn check(resp: ureq::http::Response<ureq::Body>, what: &str) -> Result<String, Error> {
        let status = resp.status();
        let body = resp.into_body().read_to_string()?;
        if status.is_success() {
            Ok(body)
        } else {
            Err(format!("{what}: HTTP {status}").into())
        }
    }

    fn get(&self, path: &str) -> Result<String, Error> {
        let r = self
            .agent
            .get(self.url(path))
            .header("Authorization", self.auth())
            .call()?;
        Self::check(r, path)
    }

    fn send_json(&self, method: &str, path: &str, json: &str) -> Result<String, Error> {
        let req = match method {
            "PUT" => self.agent.put(self.url(path)),
            _ => self.agent.post(self.url(path)),
        };
        let r = req
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json")
            .send(json)?;
        Self::check(r, path)
    }

    /// Пополняет одноразовые ключи на сервере (§4.3).
    pub fn publish_keys(&self) -> Result<(), Error> {
        let count: KeyCountResponse = serde_json::from_str(&self.get("/v1/keys/count")?)?;
        if let Some(json) = self.core.keys_to_publish(count.one_time_keys, now())? {
            self.send_json("PUT", "/v1/keys", &json)?;
            self.core.mark_keys_published()?;
        }
        Ok(())
    }

    /// Приглашение с сервером `server` (он же становится привязкой аккаунта).
    pub fn create_invite(&self, server: &str) -> Result<String, Error> {
        self.core.set_server(server.to_owned(), Vec::new())?;
        Ok(self.core.create_invite(InviteMethod::Qr, now())?)
    }

    /// Привязка к серверу по ссылке `staya://server?…` (с отпечатками ключа TLS).
    pub fn bind_server_link(&self, link: &str) -> Result<(), Error> {
        self.core.set_server_from_link(link.to_owned())?;
        Ok(())
    }

    /// Ссылка-приглашение (24 часа) на уже привязанный сервер.
    pub fn create_link_invite(&self) -> Result<String, Error> {
        Ok(self.core.create_invite(InviteMethod::Link, now())?)
    }

    /// Друзья для журнала тестового друга: ник и последняя позиция без координат.
    pub fn friend_summaries(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .core
            .list_friends()?
            .into_iter()
            .map(|f| {
                let nick = f.nick.unwrap_or_else(|| "?".into());
                match f.location {
                    Some(l) => format!(
                        "{nick}: {:?}, ±{} м, {} с назад",
                        l.kind,
                        l.accuracy_m,
                        now() - l.timestamp
                    ),
                    None if f.active => format!("{nick}: позиции нет"),
                    None => format!("{nick}: ждём ответа"),
                }
            })
            .collect())
    }

    /// Кладёт приглашение на `/dev/invite` для собеседника.
    pub fn post_invite(&self, uri: &str) -> Result<(), Error> {
        let r = self.agent.put(self.url("/dev/invite")).send(uri)?;
        Self::check(r, "/dev/invite").map(drop)
    }

    /// Ждёт приглашение на `/dev/invite`.
    pub fn fetch_invite(&self, deadline: Instant) -> Result<String, Error> {
        loop {
            let r = self.agent.get(self.url("/dev/invite")).call()?;
            if r.status().is_success() {
                return Ok(r.into_body().read_to_string()?);
            }
            if Instant::now() > deadline {
                return Err("no invite on /dev/invite".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn accept(&self, uri: &str) -> Result<(), Error> {
        // Новый аккаунт берёт сервер из приглашения (§5.3).
        self.core.set_server_from_link(uri.to_owned())?;
        let info = self.core.parse_invite(uri.to_owned())?;
        let claim = serde_json::to_string(&ClaimRequest {
            account_id: staya_proto::AccountId::from_b64(&info.account_id)?,
        })?;
        let claimed = self.send_json("POST", "/v1/keys/claim", &claim)?;
        self.core.accept_invite(uri.to_owned(), claimed, now())?;
        self.flush()
    }

    /// Отправка по §8.2: удаления слотов, затем один POST, затем `complete_send`.
    pub fn flush(&self) -> Result<(), Error> {
        let batch = self.core.pending_sends()?;
        for friend in &batch.delete_slots {
            let r = self
                .agent
                .delete(self.url(&format!("/v1/slots/{friend}")))
                .header("Authorization", self.auth())
                .call()?;
            Self::check(r, "/v1/slots")?;
            self.core.mark_slot_deleted(friend.clone())?;
        }
        if let Some(json) = batch.request_json {
            let response = self.send_json("POST", "/v1/envelopes", &json)?;
            self.core.complete_send(batch.ids, response)?;
        }
        Ok(())
    }

    /// Забирает ящик, обрабатывает, подтверждает и отправляет ответы.
    pub fn sync(&self) -> Result<Vec<CoreEvent>, Error> {
        let processed = self.core.process_mailbox(self.get("/v1/mailbox")?, now())?;
        if let Some(ack) = processed.ack_json {
            self.send_json("POST", "/v1/mailbox/ack", &ack)?;
        }
        self.flush()?;
        Ok(processed.events)
    }

    pub fn share(&self, lat_e7: i32, lon_e7: i32) -> Result<(), Error> {
        self.core.prepare_location_update(
            Some(Location {
                lat_e7,
                lon_e7,
                accuracy_m: 10,
                timestamp: now(),
            }),
            now(),
        )?;
        self.flush()
    }

    pub fn set_nick(&self, nick: &str) -> Result<(), Error> {
        self.set_profile(nick, Vec::new())
    }

    pub fn set_profile(&self, nick: &str, avatar: Vec<u8>) -> Result<(), Error> {
        self.core.set_profile(nick.to_owned(), avatar)?;
        self.flush()
    }

    pub fn has_friends(&self) -> Result<bool, Error> {
        Ok(self.core.list_friends()?.iter().any(|f| f.active))
    }
}

/// Сценарий обмена: дружба, затем позиции раз в секунду, пока не придёт ожидаемая
/// позиция друга; после неё ещё `linger` секунд шлём свою, чтобы друг тоже успел.
pub struct Exchange {
    pub mine: (i32, i32),
    pub expect: (i32, i32),
    pub timeout: Duration,
    pub linger: Duration,
}

impl Exchange {
    pub fn run(&self, peer: &Peer) -> Result<(), Error> {
        let deadline = Instant::now() + self.timeout;
        let mut got_at: Option<Instant> = None;
        loop {
            for event in peer.sync()? {
                if let CoreEvent::LocationUpdated { location, .. } = event
                    && (location.lat_e7, location.lon_e7) == self.expect
                    && got_at.is_none()
                {
                    eprintln!("PEER GOT LOCATION");
                    got_at = Some(Instant::now());
                }
            }
            if peer.has_friends()? {
                peer.share(self.mine.0, self.mine.1)?;
            }
            if got_at.is_some_and(|t| t.elapsed() >= self.linger) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err("timed out waiting for the friend's location".into());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn base64_std(bytes: &[u8]) -> String {
    // Тот же формат, что у B64 в JSON: стандартный алфавит с дополнением.
    serde_json::to_value(B64(bytes.to_vec()))
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
