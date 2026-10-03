//! World audio commands (API 2, world audio extension 1): mods publish traffic vehicles,
//! pedestrians and skaters to the game's retail world audio (the same components an engine
//! system adds, `skate-game` `world_audio.rs`), fire their one-shots (horn, alarm, speech) and read
//! back whether an object is audible. Keys belong to the calling mod; the host enforces 16
//! objects per mod and 64 in all, parks an object that has not been updated for 0.5 s, and removes
//! everything a mod published when it is disabled or reloaded.
use serde::Deserialize;

/// Objects per mod / in all.
pub const MAX_OBJECTS_PER_MOD: usize = 16;
pub const MAX_OBJECTS_TOTAL: usize = 64;
/// An object not updated for this long is parked (speed 0, feet up, horn off).
pub const PARK_SECONDS: f64 = 0.5;

/// What kind of object a key publishes.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Traffic,
    Ped,
    Skater,
}

/// Every field a spawn or an update may carry; each kind reads its own and ignores none silently
/// (a field of another kind is rejected by [`WorldAudioOptions::validate_for`]).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldAudioOptions {
    // ---- all kinds
    /// World position (m). Ignored while `body` is set.
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    /// World velocity (m/s); without it the host derives it from the position change.
    #[serde(default)]
    pub velocity: Option<[f32; 3]>,
    /// Heading (rad about +Y; 0 = +Z).
    #[serde(default)]
    pub heading: Option<f32>,
    /// Follow one of this mod's physics bodies (position, rotation and velocity from it).
    #[serde(default)]
    pub body: Option<String>,
    // ---- traffic
    /// The `aud_traffic_engine` record (`c01_family01`, `c03_sports01`, `c04_taxi01`,
    /// `c05_truck01`, …): a mod car opts in to the retail traffic engine sound (not retail).
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    /// The driver's signed acceleration (m/s²); without it, from the speed change.
    #[serde(default)]
    pub load: Option<f32>,
    /// 0 none, 1..=5 a horn kind, 6 the alarm (prefer the `horn` / `alarm` events).
    #[serde(default)]
    pub horn: Option<i32>,
    #[serde(default)]
    pub skidding: Option<bool>,
    // ---- peds
    /// Speech voice id 41..=96 (0 = none).
    #[serde(default)]
    pub voice: Option<u32>,
    #[serde(default)]
    pub shoe_class: Option<u8>,
    #[serde(default)]
    pub weight: Option<u8>,
    #[serde(default)]
    pub close_range: Option<bool>,
    /// Foot plants (A, B).
    #[serde(default)]
    pub feet: Option<[bool; 2]>,
    /// Audio surface materials under the feet (A, B).
    #[serde(default)]
    pub materials: Option<[u32; 2]>,
    /// Footsteps on (default: retail's 3-nearest rule).
    #[serde(default)]
    pub footsteps: Option<bool>,
    // ---- skaters
    /// `lite` (the default: a rolling-only state from these fields) or `state_log:<name>` (a
    /// ghost replaying a recorded audio state log, `logs/<name>.tsv` in the mod or
    /// `SKATE_AUDIO_STATE_LOGS`).
    #[serde(default)]
    pub source: Option<String>,
    /// Ghost window: start (s) and length (s) of the log to loop.
    #[serde(default)]
    pub from: Option<f32>,
    #[serde(default)]
    pub seconds: Option<f32>,
    /// Lite skater: wheels down (FL, FR, RL, RR), the material under the board (default: the
    /// ground's), grinding and its material, in the air.
    #[serde(default)]
    pub wheels: Option<[bool; 4]>,
    #[serde(default)]
    pub material: Option<u32>,
    #[serde(default)]
    pub grinding: Option<bool>,
    #[serde(default)]
    pub grind_material: Option<u32>,
    #[serde(default)]
    pub air: Option<bool>,
}

/// One-shot options (`horn`: kind, seconds; `speech`: value).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldAudioEventOptions {
    #[serde(default)]
    pub kind: Option<u8>,
    #[serde(default)]
    pub seconds: Option<f32>,
    /// A speech value: a number or a name (`warn`, `cheer`, `slam`, `flee`, `DoWarning`, …).
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}
fn point(p: &[f32; 3]) -> bool {
    finite(p) && p.iter().all(|x| x.abs() <= 100_000.0)
}
fn velocity(v: &[f32; 3]) -> bool {
    finite(v) && v.iter().all(|x| x.abs() <= 200.0)
}
fn name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') && !s.contains("..")
}

/// `lite`, or `state_log:<name>` with a safe file name.
pub fn valid_source(s: &str) -> bool {
    s == "lite" || s.strip_prefix("state_log:").is_some_and(name)
}

impl WorldAudioOptions {
    /// Bounds shared by every kind.
    pub fn validate(&self) -> bool {
        self.position.as_ref().is_none_or(point)
            && self.velocity.as_ref().is_none_or(velocity)
            && self.heading.is_none_or(|h| h.is_finite() && h.abs() <= 1000.0)
            && self.body.as_deref().is_none_or(crate::schema::valid_id)
            && self.engine.as_deref().is_none_or(name)
            && self.speed.is_none_or(|v| v.is_finite() && (0.0..=200.0).contains(&v))
            && self.load.is_none_or(|v| v.is_finite() && v.abs() <= 100.0)
            && self.horn.is_none_or(|v| (0..=6).contains(&v))
            && self.voice.is_none_or(|v| v == 0 || (41..=96).contains(&v))
            && self.shoe_class.is_none_or(|v| (1..=5).contains(&v))
            && self.weight.is_none_or(|v| (1..=5).contains(&v))
            && self.materials.is_none_or(|m| m.iter().all(|x| *x <= 143))
            && self.source.as_deref().is_none_or(valid_source)
            && self.from.is_none_or(|v| v.is_finite() && (0.0..=36_000.0).contains(&v))
            && self.seconds.is_none_or(|v| v.is_finite() && (1.0..=600.0).contains(&v))
            && self.material.is_none_or(|v| v <= 143)
            && self.grind_material.is_none_or(|v| v <= 143)
    }

    /// The fields of other kinds are rejected (so a typo'd kind is an error, not silence).
    pub fn validate_for(&self, kind: ObjectKind) -> bool {
        let traffic = self.engine.is_some() || self.speed.is_some() || self.load.is_some() || self.horn.is_some() || self.skidding.is_some();
        let ped = self.voice.is_some() || self.shoe_class.is_some() || self.weight.is_some() || self.close_range.is_some() || self.feet.is_some() || self.materials.is_some() || self.footsteps.is_some();
        let skater = self.source.is_some() || self.from.is_some() || self.seconds.is_some() || self.wheels.is_some() || self.material.is_some() || self.grinding.is_some() || self.grind_material.is_some() || self.air.is_some();
        self.validate()
            && match kind {
                ObjectKind::Traffic => !ped && !skater,
                ObjectKind::Ped => !traffic && !skater,
                // A lite skater takes its speed from `speed` too.
                ObjectKind::Skater => !ped && self.engine.is_none() && self.load.is_none() && self.horn.is_none() && self.skidding.is_none(),
            }
    }
}

impl WorldAudioEventOptions {
    pub fn validate(&self, event: &str) -> bool {
        match event {
            "horn" => self.kind.is_none_or(|k| (1..=5).contains(&k)) && self.seconds.is_none_or(|s| s.is_finite() && (0.0..=30.0).contains(&s)) && self.value.is_none(),
            "alarm" => self.kind.is_none() && self.seconds.is_none() && self.value.is_none(),
            "speech" => {
                self.kind.is_none()
                    && self.seconds.is_none()
                    && match &self.value {
                        Some(serde_json::Value::Number(n)) => n.as_i64().is_some_and(|v| (0..=127).contains(&v)),
                        Some(serde_json::Value::String(s)) => !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ' || c == '/'),
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn options_validate_per_kind() {
        let car: WorldAudioOptions = serde_json::from_value(json!({"engine":"c04_taxi01","position":[1,2,3],"speed":12,"load":-3.5,"skidding":true})).unwrap();
        assert!(car.validate_for(ObjectKind::Traffic));
        assert!(!car.validate_for(ObjectKind::Ped));
        let ped: WorldAudioOptions = serde_json::from_value(json!({"voice":59,"shoe_class":3,"feet":[true,false],"materials":[0,0]})).unwrap();
        assert!(ped.validate_for(ObjectKind::Ped) && !ped.validate_for(ObjectKind::Traffic));
        let ghost: WorldAudioOptions = serde_json::from_value(json!({"source":"state_log:state_20261003_143434","from":30,"seconds":20,"position":[0,0,0]})).unwrap();
        assert!(ghost.validate_for(ObjectKind::Skater));
        let lite: WorldAudioOptions = serde_json::from_value(json!({"source":"lite","speed":5,"wheels":[true,true,true,true],"air":false})).unwrap();
        assert!(lite.validate_for(ObjectKind::Skater));
        for bad in [json!({"voice":12}), json!({"shoe_class":0}), json!({"horn":7}), json!({"source":"state_log:../x"}), json!({"source":"record"}),
            json!({"engine":"a b"}), json!({"position":[0,1e9,0]}), json!({"velocity":[1000,0,0]}), json!({"seconds":0})] {
            let o: WorldAudioOptions = serde_json::from_value(bad.clone()).unwrap();
            assert!(!o.validate(), "accepted {bad}");
        }
        assert!(serde_json::from_value::<WorldAudioOptions>(json!({"typo":1})).is_err());
    }

    #[test]
    fn events_validate() {
        let horn: WorldAudioEventOptions = serde_json::from_value(json!({"kind":3,"seconds":1.2})).unwrap();
        assert!(horn.validate("horn") && !horn.validate("alarm") && !horn.validate("bark"));
        assert!(WorldAudioEventOptions::default().validate("alarm"));
        let speech: WorldAudioEventOptions = serde_json::from_value(json!({"value":"warn"})).unwrap();
        assert!(speech.validate("speech"));
        let speech: WorldAudioEventOptions = serde_json::from_value(json!({"value":23})).unwrap();
        assert!(speech.validate("speech"));
        let bad: WorldAudioEventOptions = serde_json::from_value(json!({"value":500})).unwrap();
        assert!(!bad.validate("speech"));
    }
}
