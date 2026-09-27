//! Константы протокола (§10).

use std::time::Duration;

/// OTK, которые клиент держит на сервере.
pub const OTK_TARGET: usize = 50;
/// Порог, ниже которого клиент пополняет OTK.
pub const OTK_REFILL_BELOW: usize = 20;
/// Ротация fallback-ключа.
pub const FALLBACK_ROTATION: Duration = Duration::from_secs(7 * 24 * 3600);

/// Срок жизни токена приглашения через QR.
pub const INVITE_TTL_QR: Duration = Duration::from_secs(10 * 60);
/// Срок жизни токена приглашения через ссылку.
pub const INVITE_TTL_LINK: Duration = Duration::from_secs(24 * 3600);

/// Срок жизни challenge для входа.
pub const AUTH_CHALLENGE_TTL: Duration = Duration::from_secs(60);
/// Срок жизни токена сессии.
pub const AUTH_TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 3600);

/// Ротация исходящей Megolm-сессии к другу: по времени…
pub const MEGOLM_ROTATION_AGE: Duration = Duration::from_secs(24 * 3600);
/// …или по числу пакетов.
pub const MEGOLM_ROTATION_MESSAGES: u32 = 1000;

/// Хранение слота позиции на сервере.
pub const LOCATION_SLOT_TTL: Duration = Duration::from_secs(72 * 3600);
/// Хранение управляющего сообщения в очереди.
pub const CONTROL_QUEUE_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Максимум управляющих сообщений в очереди одного получателя.
pub const CONTROL_QUEUE_LIMIT: usize = 200;

/// Olm-сессий, хранимых на одного друга.
pub const OLM_SESSIONS_PER_FRIEND: usize = 3;

/// Частота пакетов в состоянии покоя (призрак, заморозка).
pub const STATIONARY_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Итерации SHA-512 для отпечатка кода безопасности.
pub const FINGERPRINT_ITERATIONS: u32 = 5200;

/// Размер открытого текста пакета позиции.
pub const LOCATION_PLAINTEXT_LEN: usize = 48;
/// Размер конверта позиции.
pub const LOCATION_ENVELOPE_LEN: usize = 160;

/// Корзины открытого текста управляющих сообщений и размеры их конвертов.
pub const CONTROL_BUCKETS: [ControlBucket; 3] = [
    ControlBucket {
        plaintext: 256,
        envelope: 512,
    },
    ControlBucket {
        plaintext: 1024,
        envelope: 1280,
    },
    ControlBucket {
        plaintext: 9216,
        envelope: 9472,
    },
];

/// Максимальная длина ника в байтах UTF-8.
pub const NICK_MAX_LEN: usize = 64;
/// Максимальный размер аватара (JPEG) в байтах.
pub const AVATAR_MAX_LEN: usize = 8192;

/// Длина `megolm::SessionKey::to_bytes()` для Megolm version 1.
pub const MEGOLM_SESSION_KEY_LEN: usize = 229;

/// Пара «корзина открытого текста — размер конверта».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlBucket {
    pub plaintext: usize,
    pub envelope: usize,
}

/// Корзина для открытого текста длиной `len`, если он вообще помещается.
pub fn control_bucket_for(len: usize) -> Option<ControlBucket> {
    CONTROL_BUCKETS.iter().copied().find(|b| len <= b.plaintext)
}

/// Допустим ли размер конверта управляющего сообщения.
pub fn is_control_envelope_len(len: usize) -> bool {
    CONTROL_BUCKETS.iter().any(|b| b.envelope == len)
}
