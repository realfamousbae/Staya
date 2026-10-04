//! Ограничение частоты запросов (задача 3.6). IP клиента используется только
//! здесь, только в памяти и только как ключ корзины: не пишется ни в журнал, ни в
//! базу, а корзина забывается, когда снова полна.
//!
//! Три уровня:
//! - любой запрос — по IP, щедро (защита базы от потока мусора);
//! - регистрация и вход — по IP, строго (перебор, массовая регистрация);
//! - запросы с токеном — по токену (сломанный или злой клиент).

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, RandomState};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Корзина токенов: `burst` запросов сразу, затем `per_second` в секунду.
#[derive(Clone, Copy, Debug)]
pub struct Rate {
    pub burst: f64,
    pub per_second: f64,
}

/// Любой запрос с одного IP.
pub const GLOBAL_PER_IP: Rate = Rate {
    burst: 300.0,
    per_second: 10.0,
};
/// Регистрация и вход с одного IP.
pub const AUTH_PER_IP: Rate = Rate {
    burst: 20.0,
    per_second: 0.2,
};
/// Запросы одной сессии.
pub const PER_SESSION: Rate = Rate {
    burst: 120.0,
    per_second: 2.0,
};

struct Bucket {
    tokens: f64,
    at: Instant,
}

/// Корзины по ключу. Ключ — хеш со случайной солью процесса: в памяти нет ни
/// IP, ни токенов в открытом виде.
pub struct Limiter {
    rate: Rate,
    salt: RandomState,
    buckets: Mutex<HashMap<u64, Bucket>>,
}

impl Limiter {
    pub fn new(rate: Rate) -> Self {
        Self {
            rate,
            salt: RandomState::new(),
            buckets: Mutex::default(),
        }
    }

    /// Списывает один токен; `false` — лимит исчерпан.
    pub fn check<K: Hash>(&self, key: &K, now: Instant) -> bool {
        let key = self.salt.hash_one(key);
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rate = self.rate;
        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: rate.burst,
            at: now,
        });
        let elapsed = now.saturating_duration_since(bucket.at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate.per_second).min(rate.burst);
        bucket.at = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Забывает корзины, которые успели снова наполниться.
    pub fn forget_full(&self, now: Instant) {
        let rate = self.rate;
        self.buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, b| {
                let elapsed = now.saturating_duration_since(b.at).as_secs_f64();
                b.tokens + elapsed * rate.per_second < rate.burst
            });
    }

    pub fn len(&self) -> usize {
        self.buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct Limits {
    /// Брать IP из последнего `X-Forwarded-For` (сервер за Caddy). Без этого —
    /// адрес соединения: заголовок от клиента подделывается.
    pub trust_proxy: bool,
    pub global: Limiter,
    pub auth: Limiter,
    pub session: Limiter,
}

impl Limits {
    pub fn new(trust_proxy: bool) -> Self {
        Self {
            trust_proxy,
            global: Limiter::new(GLOBAL_PER_IP),
            auth: Limiter::new(AUTH_PER_IP),
            session: Limiter::new(PER_SESSION),
        }
    }

    pub fn forget_full(&self) {
        let now = Instant::now();
        self.global.forget_full(now);
        self.auth.forget_full(now);
        self.session.forget_full(now);
    }
}

/// IP клиента: последний адрес `X-Forwarded-For` от доверенного прокси (Caddy
/// дописывает настоящий адрес в конец) или адрес соединения.
fn client_ip(limits: &Limits, headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    if limits.trust_proxy {
        return headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .next_back()
            .and_then(|ip| ip.trim().parse().ok());
    }
    peer.map(|p| p.ip())
}

fn too_many() -> Response {
    StatusCode::TOO_MANY_REQUESTS.into_response()
}

fn peer(req: &Request) -> Option<SocketAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0)
}

/// Слой для всех маршрутов: по IP и, если есть токен, по сессии.
pub async fn global(State(limits): State<Arc<Limits>>, req: Request, next: Next) -> Response {
    let now = Instant::now();
    let ip = client_ip(&limits, req.headers(), peer(&req));
    if ip.is_some_and(|ip| !limits.global.check(&ip, now)) {
        return too_many();
    }
    if let Some(auth) = req.headers().get(header::AUTHORIZATION)
        && !limits.session.check(&auth.as_bytes(), now)
    {
        return too_many();
    }
    next.run(req).await
}

/// Слой для регистрации и входа: строгий лимит по IP.
pub async fn auth(State(limits): State<Arc<Limits>>, req: Request, next: Next) -> Response {
    let ip = client_ip(&limits, req.headers(), peer(&req));
    if ip.is_some_and(|ip| !limits.auth.check(&ip, Instant::now())) {
        return too_many();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn bucket_allows_burst_then_refills() {
        let l = Limiter::new(Rate {
            burst: 3.0,
            per_second: 1.0,
        });
        let t0 = Instant::now();
        assert!((0..3).all(|_| l.check(&"a", t0)));
        assert!(!l.check(&"a", t0));
        assert!(l.check(&"b", t0), "keys are independent");
        assert!(!l.check(&"a", t0 + Duration::from_millis(500)));
        assert!(l.check(&"a", t0 + Duration::from_millis(1600)));
    }

    #[test]
    fn full_buckets_are_forgotten() {
        let l = Limiter::new(Rate {
            burst: 2.0,
            per_second: 1.0,
        });
        let t0 = Instant::now();
        l.check(&"a", t0);
        l.forget_full(t0);
        assert_eq!(l.len(), 1);
        l.forget_full(t0 + Duration::from_secs(2));
        assert!(l.is_empty());
    }

    #[test]
    fn forwarded_for_only_from_trusted_proxy() {
        let mut h = HeaderMap::new();
        h.append(
            "x-forwarded-for",
            "198.51.100.1, 203.0.113.7".parse().unwrap(),
        );
        let peer: SocketAddr = "192.0.2.5:4000".parse().unwrap();
        let proxied = Limits::new(true);
        assert_eq!(
            client_ip(&proxied, &h, Some(peer)),
            Some("203.0.113.7".parse().unwrap())
        );
        let direct = Limits::new(false);
        assert_eq!(client_ip(&direct, &h, Some(peer)), Some(peer.ip()));
    }
}
