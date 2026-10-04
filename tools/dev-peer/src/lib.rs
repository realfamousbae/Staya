//! Тестовый собеседник: ядро + HTTP к dev-серверу, как это делает платформа (§8.2).

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use staya_core::api::{CoreEvent, InviteMethod, StayaCore};
use staya_core::friends::Location;
use staya_proto::api::{ClaimRequest, KeyCountResponse};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Peer {
    _dir: tempfile::TempDir,
    core: Arc<StayaCore>,
    base: String,
    pub id: String,
    agent: ureq::Agent,
}

impl Peer {
    /// Новый аккаунт во временной базе; `base` — адрес dev-сервера без `/` на конце.
    pub fn new(base: &str) -> Result<Self, Error> {
        let dir = tempfile::tempdir()?;
        let mut key = vec![0u8; 32];
        getrandom::fill(&mut key).map_err(|e| e.to_string())?;
        let path = dir.path().join("staya.db").to_string_lossy().into_owned();
        let core = StayaCore::open(path, key)?;
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
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.id)
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

    pub fn create_invite(&self, server: &str) -> Result<String, Error> {
        Ok(self
            .core
            .create_invite(server.to_owned(), None, InviteMethod::Qr, now())?)
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
