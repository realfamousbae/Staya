//! Recipient-only named places and current membership; never a movement history.
use crate::api::{CoreEvent, FriendLocation, LocationKind};
use crate::{CoreError, store::Store};
use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use zeroize::Zeroizing;

const RECORD: &str = "local_places";

#[derive(Clone, Serialize, Deserialize, uniffi::Record)]
pub struct PlaceView {
    pub id: String,
    pub friend: String,
    pub name: String,
    pub lat_e7: i32,
    pub lon_e7: i32,
    pub radius_m: u16,
}
impl std::fmt::Debug for PlaceView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaceView")
            .field("coordinates", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    view: PlaceView,
    inside: Option<bool>,
    candidate: Option<(bool, i64)>,
    last_at: Option<i64>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Places {
    entries: Vec<Entry>,
}

impl Places {
    pub fn load(store: &Store) -> Result<Self, CoreError> {
        store
            .get_secret(RECORD)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| CoreError::Corrupted))
            .transpose()
            .map(|value| value.unwrap_or_default())
    }
    pub fn save(&self, store: &Store) -> Result<(), CoreError> {
        let bytes = Zeroizing::new(serde_json::to_vec(self).map_err(|_| CoreError::Corrupted)?);
        store.put_secret(RECORD, &bytes)
    }
    pub fn list(&self) -> Vec<PlaceView> {
        self.entries.iter().map(|e| e.view.clone()).collect()
    }
    pub fn add(
        &mut self,
        friend: String,
        name: String,
        lat_e7: i32,
        lon_e7: i32,
        radius_m: u16,
    ) -> Result<PlaceView, CoreError> {
        if name.trim().is_empty()
            || name.len() > 64
            || !(100..=5000).contains(&radius_m)
            || !(-900_000_000..=900_000_000).contains(&lat_e7)
            || !(-1_800_000_000..=1_800_000_000).contains(&lon_e7)
            || self.entries.len() >= 64
        {
            return Err(CoreError::Invalid("place"));
        }
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| CoreError::Random)?;
        let view = PlaceView {
            id: AccountId(bytes).to_b64(),
            friend,
            name: name.trim().into(),
            lat_e7,
            lon_e7,
            radius_m,
        };
        self.entries.push(Entry {
            view: view.clone(),
            inside: None,
            candidate: None,
            last_at: None,
        });
        Ok(view)
    }
    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.view.id != id);
    }
    pub fn remove_friend(&mut self, friend: &str) {
        self.entries.retain(|e| e.view.friend != friend);
    }
    pub fn apply(&mut self, events: &[CoreEvent], now: i64) -> Vec<CoreEvent> {
        let mut result = Vec::new();
        for event in events {
            match event {
                CoreEvent::FriendRemoved { friend } => self.remove_friend(friend),
                CoreEvent::LocationUpdated { friend, location } => {
                    for entry in self.entries.iter_mut().filter(|e| &e.view.friend == friend) {
                        if let Some(entered) = entry.observe(*location, now) {
                            result.push(CoreEvent::PlaceTransition {
                                friend: friend.clone(),
                                place: entry.view.id.clone(),
                                name: entry.view.name.clone(),
                                entered,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        result
    }
}

impl Entry {
    fn observe(&mut self, location: FriendLocation, now: i64) -> Option<bool> {
        if matches!(location.kind, LocationKind::Hidden | LocationKind::Frozen) {
            self.inside = None;
            self.candidate = None;
            return None;
        }
        let at = location.timestamp;
        if now.saturating_sub(at) > 600
            || at > now.saturating_add(300)
            || self.last_at.is_some_and(|last| at <= last)
        {
            return None;
        }
        self.last_at = Some(at);
        let distance = distance_m(
            self.view.lat_e7,
            self.view.lon_e7,
            location.lat_e7,
            location.lon_e7,
        );
        let accuracy = f64::from(location.accuracy_m);
        let radius = f64::from(self.view.radius_m);
        let observed = if distance + accuracy <= radius {
            Some(true)
        } else if distance - accuracy >= radius + 50.0 {
            Some(false)
        } else {
            None
        };
        let Some(observed) = observed else {
            self.candidate = None;
            return None;
        };
        let Some(inside) = self.inside else {
            self.inside = Some(observed);
            return None;
        };
        if observed == inside {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((candidate, since)) if candidate == observed && at.saturating_sub(since) >= 30 => {
                self.inside = Some(observed);
                self.candidate = None;
                Some(observed)
            }
            Some((candidate, _)) if candidate == observed => None,
            _ => {
                self.candidate = Some((observed, at));
                None
            }
        }
    }
}

fn distance_m(a_lat: i32, a_lon: i32, b_lat: i32, b_lon: i32) -> f64 {
    let radians = |value: i32| (f64::from(value) / 1e7).to_radians();
    let (a, b) = (radians(a_lat), radians(b_lat));
    let delta_lon = radians(b_lon) - radians(a_lon);
    let h = (((b - a) / 2.0).sin().powi(2) + a.cos() * b.cos() * (delta_lon / 2.0).sin().powi(2))
        .clamp(0.0, 1.0);
    6_371_000.0 * 2.0 * h.sqrt().atan2((1.0 - h).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> Entry {
        Entry {
            view: PlaceView {
                id: "place".into(),
                friend: "friend".into(),
                name: "private-place".into(),
                lat_e7: 0,
                lon_e7: 0,
                radius_m: 250,
            },
            inside: None,
            candidate: None,
            last_at: None,
        }
    }
    fn fix(lat: i32, timestamp: i64) -> FriendLocation {
        FriendLocation {
            kind: LocationKind::Exact,
            lat_e7: lat,
            lon_e7: 0,
            accuracy_m: 10,
            timestamp,
            battery_percent: None,
            charging: false,
        }
    }
    #[test]
    fn baseline_debounce_jitter_and_replay_do_not_create_false_transitions() {
        let mut e = entry();
        assert_eq!(e.observe(fix(100_000, 100), 100), None); // Outside baseline, no initial alert.
        assert_eq!(e.observe(fix(0, 130), 130), None);
        assert_eq!(e.observe(fix(0, 140), 140), None); // Too soon.
        assert_eq!(e.observe(fix(25_000, 150), 150), None); // Border jitter resets candidate.
        assert_eq!(e.observe(fix(0, 160), 160), None);
        assert_eq!(e.observe(fix(0, 190), 190), Some(true));
        assert_eq!(e.observe(fix(100_000, 180), 190), None); // Reordered packet.
        assert_eq!(e.observe(fix(100_000, 220), 220), None);
        assert_eq!(e.observe(fix(100_000, 250), 250), Some(false));
        assert_eq!(e.observe(fix(100_000, 250), 250), None); // Duplicate.
    }
    #[test]
    fn stale_future_imprecise_and_hidden_positions_do_not_trigger() {
        let mut e = entry();
        e.observe(fix(100_000, 1), 1000);
        e.observe(fix(100_000, 1500), 1000);
        assert!(e.inside.is_none());
        let mut coarse = fix(0, 1000);
        coarse.kind = LocationKind::Approx;
        coarse.accuracy_m = 1500;
        e.observe(coarse, 1000);
        assert!(e.inside.is_none());
        e.observe(fix(0, 1010), 1010);
        let mut hidden = fix(0, 1020);
        hidden.kind = LocationKind::Hidden;
        e.observe(hidden, 1020);
        assert!(e.inside.is_none());
        assert_eq!(e.observe(fix(100_000, 1040), 1040), None); // New baseline.
        hidden.kind = LocationKind::Frozen;
        e.observe(hidden, 1050);
        assert!(e.inside.is_none());
    }
    #[test]
    fn storage_validation_deletion_and_debug_keep_places_private() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(
            &dir.path().join("places.db"),
            &crate::store::DbKey::from_slice(&[4; 32]).unwrap(),
        )
        .unwrap();
        let mut places = Places::default();
        assert!(places.add("friend".into(), "".into(), 0, 0, 250).is_err());
        assert!(
            places
                .add("friend".into(), "home".into(), 910_000_000, 0, 250)
                .is_err()
        );
        let view = places
            .add(
                "friend".into(),
                "private-place".into(),
                557_558_000,
                376_173_000,
                250,
            )
            .unwrap();
        assert!(!format!("{view:?}").contains("557558000"));
        assert!(!format!("{view:?}").contains("private-place"));
        places.save(&store).unwrap();
        let mut restored = Places::load(&store).unwrap();
        assert_eq!(restored.list()[0].name, "private-place");
        restored.remove_friend("friend");
        assert!(restored.list().is_empty());
        assert!(distance_m(0, 1_799_990_000, 0, -1_799_990_000) < 250.0); // Dateline.
    }
}
