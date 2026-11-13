//! Optional FCM: opaque addresses and fixed wake-up signals, never mailbox contents.
use crate::{
    auth::Session,
    http::{AppState, JsonBody, internal},
    live::Hub,
};
use axum::{Json, extract::State, http::StatusCode};
use deadpool_postgres::Pool;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use staya_proto::AccountId;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use zeroize::Zeroizing;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const MAX_RESPONSE: usize = 65_536;

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "type")]
    kind: String,
    project_id: String,
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
}

/// Fields are deliberately private and have no Debug implementation.
pub struct FcmClient {
    email: String,
    key: EncodingKey,
    key_id: Option<String>,
    http: reqwest::Client,
    token_url: String,
    send_url: String,
    access: Mutex<Option<(String, Instant)>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushError {
    Authentication,
    Transport,
    Rejected(u16),
    InvalidResponse,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Accepted,
    Unregistered,
}

impl FcmClient {
    pub fn from_file(path: &Path) -> Result<Self, &'static str> {
        let size = std::fs::metadata(path)
            .map_err(|_| "credentials unavailable")?
            .len();
        if size > MAX_RESPONSE as u64 {
            return Err("credentials too large");
        }
        let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| "credentials unavailable")?);
        let credentials: Credentials =
            serde_json::from_slice(&bytes).map_err(|_| "invalid credential format")?;
        if credentials.kind != "service_account"
            || credentials.project_id.is_empty()
            || credentials.project_id.len() > 128
            || !credentials
                .project_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !credentials
                .client_email
                .ends_with(".iam.gserviceaccount.com")
        {
            return Err("invalid credential identity");
        }
        let pem = Zeroizing::new(credentials.private_key);
        let key =
            EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(|_| "invalid credential key")?;
        let http = build_transport(true).map_err(|_| "HTTP client unavailable")?;
        Ok(Self {
            email: credentials.client_email,
            key,
            key_id: credentials.private_key_id,
            http,
            token_url: TOKEN_URL.into(),
            send_url: format!(
                "https://fcm.googleapis.com/v1/projects/{}/messages:send",
                credentials.project_id
            ),
            access: Mutex::new(None),
        })
    }

    async fn access_token(&self) -> Result<String, PushError> {
        let mut cache = self.access.lock().await;
        if let Some((token, until)) = &*cache
            && *until > Instant::now()
        {
            return Ok(token.clone());
        }
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            scope: &'a str,
            aud: &'a str,
            iat: u64,
            exp: u64,
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PushError::Authentication)?
            .as_secs();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = self.key_id.clone();
        let assertion = Zeroizing::new(
            jsonwebtoken::encode(
                &header,
                &Claims {
                    iss: &self.email,
                    scope: SCOPE,
                    aud: TOKEN_URL,
                    iat: now,
                    exp: now.saturating_add(3600),
                },
                &self.key,
            )
            .map_err(|_| PushError::Authentication)?,
        );
        let response = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|_| PushError::Transport)?;
        if !response.status().is_success() {
            return Err(PushError::Authentication);
        }
        #[derive(Deserialize)]
        struct Access {
            access_token: String,
            expires_in: u64,
        }
        let response: Access = serde_json::from_slice(&read_small(response).await?)
            .map_err(|_| PushError::InvalidResponse)?;
        if response.access_token.is_empty()
            || response.access_token.len() > 16_384
            || response.expires_in == 0
        {
            return Err(PushError::InvalidResponse);
        }
        let until =
            Instant::now() + Duration::from_secs(response.expires_in.min(3600).saturating_sub(60));
        *cache = Some((response.access_token.clone(), until));
        Ok(response.access_token)
    }

    pub async fn send(&self, token: &str) -> Result<Delivery, PushError> {
        self.submit(wake_message(Some(token)), false).await
    }

    /// Google validation mode: checks credentials/IAM without sending to a device.
    pub async fn validate_configuration(&self) -> Result<(), PushError> {
        match self.submit(wake_message(None), true).await? {
            Delivery::Accepted => Ok(()),
            Delivery::Unregistered => Err(PushError::InvalidResponse),
        }
    }

    async fn submit(&self, message: Value, validate: bool) -> Result<Delivery, PushError> {
        let access = self.access_token().await?;
        let mut payload = json!({ "message": message });
        if validate {
            payload["validate_only"] = json!(true);
        }
        let response = self
            .http
            .post(&self.send_url)
            .bearer_auth(access)
            .json(&payload)
            .send()
            .await
            .map_err(|_| PushError::Transport)?;
        if response.status().is_success() {
            return Ok(Delivery::Accepted);
        }
        let status = response.status().as_u16();
        let body = read_small(response).await?;
        let error: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if error["error"]["details"]
            .as_array()
            .is_some_and(|details| details.iter().any(|d| d["errorCode"] == "UNREGISTERED"))
        {
            return Ok(Delivery::Unregistered);
        }
        if status == 401 {
            *self.access.lock().await = None;
        }
        Err(PushError::Rejected(status))
    }
}

fn build_transport(https_only: bool) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(https_only)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
}

fn wake_message(token: Option<&str>) -> Value {
    let mut message = json!({"data":{"wake":"1"}, "android":{"priority":"NORMAL", "ttl":"3600s", "collapse_key":"staya-mailbox"}});
    if let Some(token) = token {
        message["token"] = json!(token);
    } else {
        message["topic"] = json!("staya-configuration-check");
    }
    message
}

async fn read_small(mut response: reqwest::Response) -> Result<Vec<u8>, PushError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| PushError::Transport)? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err(PushError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Clone)]
pub struct PushQueue {
    tx: mpsc::Sender<AccountId>,
}
impl PushQueue {
    pub fn spawn(pool: Pool, client: FcmClient, hub: Arc<Hub>) -> Self {
        let (tx, mut rx) = mpsc::channel::<AccountId>(1024);
        tokio::spawn(async move {
            let client = Arc::new(client);
            let mut pending = HashSet::new();
            let mut last: HashMap<AccountId, Instant> = HashMap::new();
            let mut running = tokio::task::JoinSet::new();
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    recipient = rx.recv() => {
                        let Some(recipient) = recipient else { break; };
                        if pending.len() < 2048 { pending.insert(recipient); }
                    }
                    _ = running.join_next(), if !running.is_empty() => {}
                    _ = tick.tick() => {
                        last.retain(|_, at| at.elapsed() < Duration::from_secs(30));
                        pending.retain(|recipient| hub.connections(recipient) == 0);
                        let ready: Vec<_> = pending.iter().filter(|recipient| !last.contains_key(recipient))
                            .take(4usize.saturating_sub(running.len())).copied().collect();
                        for recipient in ready {
                            pending.remove(&recipient);
                            last.insert(recipient, Instant::now());
                            let (pool, client) = (pool.clone(), client.clone());
                            running.spawn(async move { wake_one(pool, client, recipient).await; });
                        }
                    }
                }
            }
        });
        Self { tx }
    }
    pub fn wake(&self, recipient: AccountId) {
        let _ = self.tx.try_send(recipient);
    }
}

async fn wake_one(pool: Pool, client: Arc<FcmClient>, recipient: AccountId) {
    let row = {
        let Ok(connection) = pool.get().await else {
            return;
        };
        match connection.query_opt("SELECT token, token_hash FROM push_devices WHERE account_id=$1 AND updated_at > now()-interval '30 days'", &[&recipient.0.as_slice()]).await {
            Ok(value) => value, Err(_) => return,
        }
    };
    let Some(row) = row else {
        return;
    };
    let token: String = row.get(0);
    let digest: Vec<u8> = row.get(1);
    match client.send(&token).await {
        Ok(Delivery::Accepted) => {}
        Ok(Delivery::Unregistered) => {
            if let Ok(connection) = pool.get().await {
                let _ = connection
                    .execute(
                        "DELETE FROM push_devices WHERE account_id=$1 AND token_hash=$2",
                        &[&recipient.0.as_slice(), &digest],
                    )
                    .await;
            }
        }
        Err(error) => {
            tracing::warn!(category = ?error, "FCM wake failed; encrypted mailbox remains queued")
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterPush {
    token: String,
}

pub async fn capabilities(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"fcm": state.push.is_some(), "sealed_delivery": false}))
}
pub async fn register(
    State(state): State<AppState>,
    Session(me): Session,
    JsonBody(request): JsonBody<RegisterPush>,
) -> Result<StatusCode, StatusCode> {
    if state.push.is_none() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if !(16..=4096).contains(&request.token.len())
        || !request.token.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let digest = Sha256::digest(request.token.as_bytes()).to_vec();
    let mut connection = state.pool.get().await.map_err(internal)?;
    let tx = connection.transaction().await.map_err(internal)?;
    tx.query_one(
        "SELECT account_id FROM accounts WHERE account_id=$1 FOR UPDATE",
        &[&me.0.as_slice()],
    )
    .await
    .map_err(internal)?;
    tx.execute(
        "DELETE FROM push_devices WHERE account_id=$1",
        &[&me.0.as_slice()],
    )
    .await
    .map_err(internal)?;
    tx.execute("INSERT INTO push_devices (account_id, token, token_hash) VALUES ($1,$2,$3) ON CONFLICT(token_hash) DO UPDATE SET account_id=EXCLUDED.account_id, token=EXCLUDED.token, updated_at=now()", &[&me.0.as_slice(), &request.token, &digest]).await.map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn unregister(
    State(state): State<AppState>,
    Session(me): Session,
) -> Result<StatusCode, StatusCode> {
    state
        .pool
        .get()
        .await
        .map_err(internal)?
        .execute(
            "DELETE FROM push_devices WHERE account_id=$1",
            &[&me.0.as_slice()],
        )
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Opaque device addresses are forgotten after 30 days without registration refresh.
pub async fn purge_expired(pool: &Pool) -> Result<u64, crate::db::DbError> {
    Ok(pool
        .get()
        .await?
        .execute(
            "DELETE FROM push_devices WHERE updated_at <= now()-interval '30 days'",
            &[],
        )
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderMap, header},
        response::{IntoResponse, Response},
        routing::post,
    };
    use base64::Engine;
    use std::io::Write;
    use std::sync::{
        Mutex as StdMutex, OnceLock,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Mock {
        oauth: AtomicUsize,
        messages: StdMutex<Vec<Value>>,
        status: AtomicU16,
        redirect: AtomicBool,
        followed: AtomicUsize,
    }
    fn credentials() -> tempfile::NamedTempFile {
        static KEY: OnceLock<String> = OnceLock::new();
        let key = KEY.get_or_init(|| {
            let output = std::process::Command::new("openssl")
                .args([
                    "genpkey",
                    "-algorithm",
                    "RSA",
                    "-pkeyopt",
                    "rsa_keygen_bits:2048",
                ])
                .output()
                .expect("openssl test fixture generator");
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        });
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(serde_json::to_string(&json!({"type":"service_account", "project_id":"staya-test", "client_email":"sender@staya-test.iam.gserviceaccount.com", "private_key":key, "private_key_id":"test-key", "token_uri":"http://untrusted.invalid/ignored"})).unwrap().as_bytes()).unwrap();
        file
    }
    async fn oauth(State(state): State<Arc<Mock>>, body: String) -> Response {
        state.oauth.fetch_add(1, Ordering::SeqCst);
        if state.redirect.load(Ordering::SeqCst) {
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [(header::LOCATION, "/never")],
            )
                .into_response();
        }
        let assertion = body
            .split('&')
            .find_map(|part| part.strip_prefix("assertion="))
            .unwrap();
        let segments: Vec<_> = assertion.split('.').collect();
        let header: Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(segments[0])
                .unwrap(),
        )
        .unwrap();
        let claims: Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(segments[1])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["kid"], "test-key");
        assert_eq!(claims["aud"], TOKEN_URL);
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["iss"], "sender@staya-test.iam.gserviceaccount.com");
        assert_eq!(
            claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(),
            3600
        );
        Json(json!({"access_token":"fake-access", "expires_in":3600})).into_response()
    }
    async fn message(
        State(state): State<Arc<Mock>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        assert_eq!(
            headers.get(header::AUTHORIZATION).unwrap(),
            "Bearer fake-access"
        );
        assert_eq!(body["message"]["data"], json!({"wake":"1"}));
        assert_eq!(body["message"]["android"]["priority"], "NORMAL");
        assert!(body["message"].get("notification").is_none());
        state.messages.lock().unwrap().push(body);
        let status = state.status.load(Ordering::SeqCst);
        if status == 200 {
            Json(json!({"name":"mock-message"})).into_response()
        } else {
            let details = if status == 404 {
                json!([{"errorCode":"UNREGISTERED"}])
            } else {
                json!([])
            };
            (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"error":{"message":"do-not-log-token", "details":details}})),
            )
                .into_response()
        }
    }
    async fn never(State(state): State<Arc<Mock>>) -> StatusCode {
        state.followed.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK
    }
    async fn mock() -> (FcmClient, Arc<Mock>, tokio::task::JoinHandle<()>) {
        let file = credentials();
        let mut client = FcmClient::from_file(file.path()).unwrap();
        assert_eq!(client.token_url, TOKEN_URL); // Credential-file URLs cannot redirect assertions.
        let state = Arc::new(Mock::default());
        state.status.store(200, Ordering::SeqCst);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        client.http = build_transport(false).unwrap(); // Loopback only; production always requires HTTPS.
        client.token_url = format!("http://{addr}/token");
        client.send_url = format!("http://{addr}/send");
        let app = Router::new()
            .route("/token", post(oauth))
            .route("/send", post(message))
            .route("/never", post(never))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (client, state, server)
    }
    #[tokio::test]
    async fn oauth_is_cached_and_only_fixed_wake_data_is_sent() {
        let (client, state, server) = mock().await;
        let (a, b) = tokio::join!(
            client.send("test-device-token"),
            client.send("test-device-token")
        );
        assert_eq!(a.unwrap(), Delivery::Accepted);
        assert_eq!(b.unwrap(), Delivery::Accepted);
        assert_eq!(state.oauth.load(Ordering::SeqCst), 1);
        let messages = state.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["message"]["token"], "test-device-token");
        server.abort();
    }
    #[tokio::test]
    async fn dry_run_never_targets_a_device_and_provider_errors_do_not_leak_payloads() {
        let (client, state, server) = mock().await;
        client.validate_configuration().await.unwrap();
        {
            let messages = state.messages.lock().unwrap();
            assert_eq!(messages[0]["validate_only"], true);
            assert!(messages[0]["message"].get("token").is_none());
            assert_eq!(messages[0]["message"]["topic"], "staya-configuration-check");
        }
        state.status.store(404, Ordering::SeqCst);
        assert_eq!(
            client.send("test-device-token").await.unwrap(),
            Delivery::Unregistered
        );
        state.status.store(403, Ordering::SeqCst);
        let error = client.send("test-device-token").await.unwrap_err();
        assert_eq!(error, PushError::Rejected(403));
        assert!(!format!("{error:?}").contains("do-not-log-token"));
        server.abort();
    }
    #[tokio::test]
    async fn credential_assertions_never_follow_redirects_and_unauthorized_tokens_refresh() {
        let (client, state, server) = mock().await;
        state.redirect.store(true, Ordering::SeqCst);
        assert_eq!(
            client.send("test-device-token").await.unwrap_err(),
            PushError::Authentication
        );
        assert_eq!(state.followed.load(Ordering::SeqCst), 0);
        state.redirect.store(false, Ordering::SeqCst);
        client.send("test-device-token").await.unwrap();
        state.status.store(401, Ordering::SeqCst);
        assert_eq!(
            client.send("test-device-token").await.unwrap_err(),
            PushError::Rejected(401)
        );
        state.status.store(200, Ordering::SeqCst);
        client.send("test-device-token").await.unwrap();
        assert_eq!(state.oauth.load(Ordering::SeqCst), 3); // Redirect attempt, success, refresh.
        server.abort();
    }
}
