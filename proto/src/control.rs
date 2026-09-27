//! Открытый текст управляющих сообщений Olm (§6.1).

use std::fmt;

use crate::bytes::{Reader, put_lp8, put_lp16};
use crate::consts::{AVATAR_MAX_LEN, NICK_MAX_LEN, control_bucket_for};
use crate::{AccountId, ProtoError};

const VERSION: u8 = 1;

/// Профиль, который видят только друзья.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Profile {
    pub nick: String,
    /// JPEG; пустой — аватара нет.
    pub avatar: Vec<u8>,
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profile")
            .field("nick", &self.nick)
            .field("avatar_len", &self.avatar.len())
            .finish()
    }
}

/// Байты `megolm::SessionKey` — секрет, поэтому `Debug` их не печатает.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionKeyBytes(pub Vec<u8>);

impl fmt::Debug for SessionKeyBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionKeyBytes(<{} bytes redacted>)", self.0.len())
    }
}

/// Управляющее сообщение, передаваемое через Olm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlMessage {
    FriendRequest {
        token: [u8; 16],
        account_id: AccountId,
        sk: [u8; 32],
        session_key: SessionKeyBytes,
        profile: Profile,
    },
    FriendAccept {
        session_key: SessionKeyBytes,
        profile: Profile,
    },
    SessionShare {
        session_key: SessionKeyBytes,
    },
    Profile(Profile),
    Unfriend,
}

impl ControlMessage {
    fn type_byte(&self) -> u8 {
        match self {
            Self::FriendRequest { .. } => 1,
            Self::FriendAccept { .. } => 2,
            Self::SessionShare { .. } => 3,
            Self::Profile(_) => 4,
            Self::Unfriend => 5,
        }
    }

    /// Кодирует и дополняет нулями до корзины (256, 1024 или 9216 байт).
    pub fn encode(&self) -> Result<Vec<u8>, ProtoError> {
        let mut out = vec![VERSION, self.type_byte()];
        match self {
            Self::FriendRequest {
                token,
                account_id,
                sk,
                session_key,
                profile,
            } => {
                out.extend_from_slice(token);
                out.extend_from_slice(&account_id.0);
                out.extend_from_slice(sk);
                put_lp16(&mut out, &session_key.0, "session key")?;
                put_profile(&mut out, profile)?;
            }
            Self::FriendAccept {
                session_key,
                profile,
            } => {
                put_lp16(&mut out, &session_key.0, "session key")?;
                put_profile(&mut out, profile)?;
            }
            Self::SessionShare { session_key } => {
                put_lp16(&mut out, &session_key.0, "session key")?;
            }
            Self::Profile(profile) => put_profile(&mut out, profile)?,
            Self::Unfriend => {}
        }
        let bucket =
            control_bucket_for(out.len()).ok_or(ProtoError::TooLarge("control message"))?;
        out.resize(bucket.plaintext, 0);
        Ok(out)
    }

    /// Разбирает открытый текст; паддинг после тела игнорируется.
    pub fn decode(buf: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(buf);
        let version = r.u8()?;
        if version != VERSION {
            return Err(ProtoError::Version(version));
        }
        let msg = match r.u8()? {
            1 => Self::FriendRequest {
                token: r.array()?,
                account_id: AccountId(r.array()?),
                sk: r.array()?,
                session_key: SessionKeyBytes(r.lp16()?.to_vec()),
                profile: read_profile(&mut r)?,
            },
            2 => Self::FriendAccept {
                session_key: SessionKeyBytes(r.lp16()?.to_vec()),
                profile: read_profile(&mut r)?,
            },
            3 => Self::SessionShare {
                session_key: SessionKeyBytes(r.lp16()?.to_vec()),
            },
            4 => Self::Profile(read_profile(&mut r)?),
            5 => Self::Unfriend,
            other => return Err(ProtoError::UnknownType(other)),
        };
        Ok(msg)
    }
}

fn put_profile(out: &mut Vec<u8>, p: &Profile) -> Result<(), ProtoError> {
    if p.nick.len() > NICK_MAX_LEN {
        return Err(ProtoError::TooLarge("nick"));
    }
    if p.avatar.len() > AVATAR_MAX_LEN {
        return Err(ProtoError::TooLarge("avatar"));
    }
    put_lp8(out, p.nick.as_bytes(), "nick")?;
    put_lp16(out, &p.avatar, "avatar")
}

fn read_profile(r: &mut Reader<'_>) -> Result<Profile, ProtoError> {
    let nick = r.lp8()?;
    if nick.len() > NICK_MAX_LEN {
        return Err(ProtoError::TooLarge("nick"));
    }
    let nick = std::str::from_utf8(nick).map_err(|_| ProtoError::Invalid("nick is not UTF-8"))?;
    let avatar = r.lp16()?;
    if avatar.len() > AVATAR_MAX_LEN {
        return Err(ProtoError::TooLarge("avatar"));
    }
    Ok(Profile {
        nick: nick.to_owned(),
        avatar: avatar.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{CONTROL_BUCKETS, MEGOLM_SESSION_KEY_LEN};
    use proptest::prelude::*;

    fn session_key() -> impl Strategy<Value = SessionKeyBytes> {
        proptest::collection::vec(any::<u8>(), MEGOLM_SESSION_KEY_LEN).prop_map(SessionKeyBytes)
    }

    fn profile() -> impl Strategy<Value = Profile> {
        (
            "[a-zA-Zа-яА-Я0-9 ]{0,20}",
            proptest::collection::vec(any::<u8>(), 0..=AVATAR_MAX_LEN),
        )
            .prop_map(|(nick, avatar)| Profile { nick, avatar })
    }

    fn message() -> impl Strategy<Value = ControlMessage> {
        prop_oneof![
            (
                any::<[u8; 16]>(),
                any::<[u8; 16]>(),
                any::<[u8; 32]>(),
                session_key(),
                profile()
            )
                .prop_map(|(token, id, sk, session_key, profile)| {
                    ControlMessage::FriendRequest {
                        token,
                        account_id: AccountId(id),
                        sk,
                        session_key,
                        profile,
                    }
                }),
            (session_key(), profile()).prop_map(|(session_key, profile)| {
                ControlMessage::FriendAccept {
                    session_key,
                    profile,
                }
            }),
            session_key().prop_map(|session_key| ControlMessage::SessionShare { session_key }),
            profile().prop_map(ControlMessage::Profile),
            Just(ControlMessage::Unfriend),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn roundtrip_and_bucketed(msg in message()) {
            let bytes = msg.encode().unwrap();
            prop_assert!(CONTROL_BUCKETS.iter().any(|b| b.plaintext == bytes.len()));
            prop_assert_eq!(ControlMessage::decode(&bytes).unwrap(), msg);
        }

        #[test]
        fn decode_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..600)) {
            let _ = ControlMessage::decode(&buf);
        }
    }

    #[test]
    fn small_messages_use_smallest_bucket() {
        let share = ControlMessage::SessionShare {
            session_key: SessionKeyBytes(vec![1; MEGOLM_SESSION_KEY_LEN]),
        };
        assert_eq!(share.encode().unwrap().len(), 256);
        assert_eq!(ControlMessage::Unfriend.encode().unwrap().len(), 256);
    }

    #[test]
    fn max_friend_request_fits_largest_bucket() {
        let msg = ControlMessage::FriendRequest {
            token: [0; 16],
            account_id: AccountId([0; 16]),
            sk: [0; 32],
            session_key: SessionKeyBytes(vec![0; MEGOLM_SESSION_KEY_LEN]),
            profile: Profile {
                nick: "x".repeat(NICK_MAX_LEN),
                avatar: vec![0; AVATAR_MAX_LEN],
            },
        };
        assert_eq!(msg.encode().unwrap().len(), 9216);
    }

    #[test]
    fn oversized_fields_are_rejected() {
        let long_nick = Profile {
            nick: "x".repeat(NICK_MAX_LEN + 1),
            avatar: vec![],
        };
        assert_eq!(
            ControlMessage::Profile(long_nick).encode(),
            Err(ProtoError::TooLarge("nick"))
        );
        let big_avatar = Profile {
            nick: String::new(),
            avatar: vec![0; AVATAR_MAX_LEN + 1],
        };
        assert_eq!(
            ControlMessage::Profile(big_avatar).encode(),
            Err(ProtoError::TooLarge("avatar"))
        );
    }

    #[test]
    fn debug_hides_session_key() {
        let msg = ControlMessage::SessionShare {
            session_key: SessionKeyBytes(vec![0xAB; 4]),
        };
        assert!(!format!("{msg:?}").contains("171"));
    }
}
