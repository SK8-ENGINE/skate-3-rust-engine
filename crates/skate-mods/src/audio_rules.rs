//! Declarative mute / replace / layer rules on the game's audio event sites (capability
//! `audio_events` = 2; doc 16 "Rules"). Lua cannot run inside the audio pass, so a mod declares
//! what should happen and the engine applies it at the post site, the same frame:
//!
//! ```json
//! { "match": { "tag": "pop" }, "action": "replace", "play": { "path": "audio/pop.wav", "volume": 0.8 } }
//! ```
//!
//! - `match`: every given field must hold (at least one): `tag` (the event tags: `pop`, `land`,
//!   `grind_start`, `grind_end`, `footstep`, `horn`, `alarm`, `tazer`, `body_fall`, `emitter`),
//!   `kind` (`post`, `splice`, `emitter_start`), `source` (`player`, `world`, `npc`, `emitter`),
//!   `class` (a retail class for posts, a bank for Splice starts and emitters), `slot` (the poster's
//!   slot name: `grind`, `wind`, `footstep`, `horn`, …) and `id` (the slot index for posts, the
//!   sound id for Splice starts, the patch for emitters).
//! - `action`: `mute` (the request is dropped: a post is not made, so its later updates and its
//!   release do nothing; a Splice sound does not start; an emitter keeps its state but posts
//!   nothing), `replace` (mute + `play`), `layer` (the game's sound and `play`).
//! - `play`: the mod's own WAV through the native mixer, `volume` 0..1, `pitch` 0.25..4, `reverb`
//!   (default true), `group` `player` (default) or `world`; at most once per `min_interval`
//!   seconds (default 0.05) per rule. Where it plays (`at`): `owner` (default: at the owner of the
//!   game's sound, following it, plus an `offset`), `world` (a fixed `position`) or `centre`
//!   (non-positional, the retail non-positional emitter outputs); `falloff` = the reach of a
//!   positional one (default: the owner's retail reach).
//! - Rules apply while the mod runs (runtime `sdk.audio.rule(key, rule|nil)`, or `audio.json`
//!   `rules`). The first matching rule decides (mods in mod-id order, then rule keys). Event rows
//!   are the game's requests: a muted request is still reported to subscribers.
use serde::{Deserialize, Serialize};

/// Rules one mod may hold (runtime and `audio.json` together) and in all.
pub const MAX_RULES_PER_MOD: usize = 32;
pub const MAX_RULES: usize = 64;
/// The tags a rule can match (the engine's event tags without `zone_change` / `speech`, which
/// are not post sites).
pub const TAGS: [&str; 10] = ["pop", "land", "grind_start", "grind_end", "footstep", "horn", "alarm", "tazer", "body_fall", "emitter"];
pub const KINDS: [&str; 3] = ["post", "splice", "emitter_start"];
pub const SOURCES: [&str; 4] = ["player", "world", "npc", "emitter"];

fn min_interval() -> f32 {
    0.05
}
fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Mute,
    Replace,
    Layer,
}

/// Where a rule's sound plays (user decision 2026-10-04: positional, the mod chooses where).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAt {
    /// At the owner of the game's sound (the local skater, the car / ped, the NPC skater, the
    /// emitter), following it while it plays; `offset` is added (world axes).
    #[default]
    Owner,
    /// At the fixed world `position`.
    World,
    /// Non-positional, centered (the retail non-positional emitter outputs).
    #[serde(alias = "center")]
    Centre,
}

/// The axes a rule sound's `offset` is in (audio events extension 3, doc 16 "offset frame").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleFrame {
    /// World axes (x, y up, z): the default.
    #[default]
    World,
    /// The owner's own axes, turning with it: x = its right, y = up, z = its facing (the local
    /// skater's and an NPC skater's board, a car's direction, a ped's walking direction, an
    /// emitter's forward).
    Owner,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulePlay {
    /// Mod-relative PCM16 WAV (the `sdk.audio.play` rules: 30 s, 8 MiB).
    pub path: String,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default = "one")]
    pub pitch: f32,
    #[serde(default)]
    pub reverb: Option<bool>,
    #[serde(default)]
    pub group: Option<String>,
    /// Where it plays (default: at the owner).
    #[serde(default)]
    pub at: RuleAt,
    /// `owner` only: meters added to the owner's position (world axes, y up), −100..100.
    #[serde(default)]
    pub offset: Option<[f32; 3]>,
    /// `owner` only: the axes of `offset` (default `world`; `owner` = the owner's right, up, facing).
    #[serde(default)]
    pub frame: Option<RuleFrame>,
    /// `world` only (required there): the world position.
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    /// Positional only: the reach (default: the owner's retail reach, `game_audio::mod_rules`).
    #[serde(default)]
    pub falloff: Option<crate::audio::NativeFalloff>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub on: RuleMatch,
    pub action: RuleAction,
    #[serde(default)]
    pub play: Option<RulePlay>,
    #[serde(default = "min_interval")]
    pub min_interval: f32,
}

fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') && !s.contains("..")
}

impl RuleMatch {
    pub fn validate(&self) -> bool {
        let any = self.tag.is_some() || self.kind.is_some() || self.source.is_some() || self.class.is_some() || self.slot.is_some() || self.id.is_some();
        any && self.tag.as_deref().is_none_or(|t| TAGS.contains(&t))
            && self.kind.as_deref().is_none_or(|k| KINDS.contains(&k))
            && self.source.as_deref().is_none_or(|s| SOURCES.contains(&s))
            && self.class.as_deref().is_none_or(word)
            && self.slot.as_deref().is_none_or(word)
    }
}

impl RulePlay {
    pub fn validate(&self) -> bool {
        crate::audio::valid_audio_path(&self.path)
            && self.volume.is_finite() && (0.0..=1.0).contains(&self.volume)
            && self.pitch.is_finite() && (0.25..=4.0).contains(&self.pitch)
            && self.group.as_deref().is_none_or(|g| crate::audio::NATIVE_GROUPS.contains(&g))
            && self.validate_placement()
    }

    /// `offset` only with `owner`, `position` exactly with `world`, `falloff` not with `centre`;
    /// the ranges of `sdk.audio.play` (offset ±100 m, position ±100 km, the reach's).
    fn validate_placement(&self) -> bool {
        let fields = match self.at {
            RuleAt::Owner => self.position.is_none(),
            RuleAt::World => self.offset.is_none() && self.position.is_some() && self.frame.is_none(),
            RuleAt::Centre => self.offset.is_none() && self.position.is_none() && self.falloff.is_none() && self.frame.is_none(),
        };
        fields
            && self.offset.as_ref().is_none_or(crate::audio::offset)
            && self.position.as_ref().is_none_or(crate::audio::point)
            && self.falloff.as_ref().is_none_or(crate::audio::NativeFalloff::validate)
    }
}

impl Rule {
    /// The shape: a non-empty match of known names; `play` exactly for `replace` / `layer`.
    pub fn validate(&self) -> bool {
        self.on.validate()
            && self.min_interval.is_finite()
            && (0.0..=10.0).contains(&self.min_interval)
            && match self.action {
                RuleAction::Mute => self.play.is_none(),
                RuleAction::Replace | RuleAction::Layer => self.play.as_ref().is_some_and(RulePlay::validate),
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rules_parse_and_validate() {
        for ok in [
            json!({"match": {"tag": "pop"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "replace", "play": {"path": "audio/pop.wav", "volume": 0.8}}),
            json!({"match": {"source": "world", "class": "TRAFFIC_HORN"}, "action": "layer", "play": {"path": "a.wav", "pitch": 1.5, "reverb": false, "group": "world"}, "min_interval": 0.5}),
            json!({"match": {"kind": "splice", "class": "Skate_Collisions", "id": 12}, "action": "mute"}),
            json!({"match": {"kind": "emitter_start", "class": "Baby_Cry_1"}, "action": "mute"}),
        ] {
            let r: Rule = serde_json::from_value(ok.clone()).unwrap();
            assert!(r.validate(), "{ok}");
        }
        for bad in [
            json!({"match": {}, "action": "mute"}),
            json!({"match": {"tag": "zone_change"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "mute", "play": {"path": "a.wav"}}),
            json!({"match": {"tag": "pop"}, "action": "replace"}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "../a.wav"}}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "a.wav", "volume": 2}}),
            json!({"match": {"source": "skater"}, "action": "mute"}),
            json!({"match": {"class": "a b"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "mute", "min_interval": 20}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "a.wav", "group": "music"}}),
        ] {
            let r: Rule = serde_json::from_value(bad.clone()).unwrap();
            assert!(!r.validate(), "accepted {bad}");
        }
        for typo in [json!({"match": {"tag": "pop"}, "action": "silence"}), json!({"match": {"tags": "pop"}, "action": "mute"}), json!({"match": {"tag": "pop"}, "action": "mute", "when": 1})] {
            assert!(serde_json::from_value::<Rule>(typo.clone()).is_err(), "{typo}");
        }
    }

    /// Where the rule's sound plays (user decision 2026-10-04): at the owner by default, with an
    /// offset; a fixed world position; centered; the reach. Each combination is checked.
    #[test]
    fn rule_placement_options_validate() {
        let play = |p: serde_json::Value| -> Rule {
            let mut play = json!({"path": "a.wav"});
            play.as_object_mut().unwrap().extend(p.as_object().unwrap().clone());
            serde_json::from_value(json!({"match": {"tag": "horn"}, "action": "replace", "play": play})).unwrap()
        };
        let default = play(json!({}));
        assert_eq!(default.play.as_ref().unwrap().at, RuleAt::Owner, "at the owner by default");
        assert!(default.validate());
        for ok in [
            json!({"at": "owner"}),
            json!({"offset": [0, 1.5, 0]}),
            json!({"at": "owner", "offset": [-100, 0, 100], "falloff": {"radius": 20, "curve": "linear"}}),
            json!({"at": "world", "position": [10, 0, -4]}),
            json!({"at": "world", "position": [1e5, 0, -1e5], "falloff": {"radius": 30, "core": 0.2}}),
            json!({"at": "centre"}),
            json!({"at": "center"}),
            json!({"offset": [0, 0, 2], "frame": "owner"}),
            json!({"at": "owner", "offset": [1, 0, 0], "frame": "world"}),
        ] {
            assert!(play(ok.clone()).validate(), "{ok}");
        }
        assert_eq!(default.play.as_ref().unwrap().frame, None, "world axes by default");
        assert_eq!(play(json!({"frame": "owner"})).play.unwrap().frame, Some(RuleFrame::Owner));
        assert!(serde_json::from_value::<Rule>(json!({"match": {"tag": "horn"}, "action": "layer", "play": {"path": "a.wav", "frame": "car"}})).is_err());
        assert_eq!(play(json!({"at": "center"})).play.unwrap().at, RuleAt::Centre);
        for bad in [
            json!({"position": [1, 2, 3]}),
            json!({"at": "world"}),
            json!({"at": "world", "position": [1, 2, 3], "offset": [0, 1, 0]}),
            json!({"at": "centre", "offset": [0, 1, 0]}),
            json!({"at": "centre", "position": [0, 1, 0]}),
            json!({"at": "centre", "falloff": {"radius": 10}}),
            json!({"at": "world", "position": [1, 2, 3], "frame": "owner"}),
            json!({"at": "centre", "frame": "owner"}),
            json!({"offset": [0, 101, 0]}),
            json!({"at": "world", "position": [0, 1e6, 0]}),
            json!({"falloff": {"radius": 0}}),
            json!({"falloff": {"radius": 10, "core": 2}}),
        ] {
            assert!(!play(bad.clone()).validate(), "accepted {bad}");
        }
        for typo in [json!({"at": "listener"}), json!({"falloff": {"radius": 10, "shape": "box"}}), json!({"offset": [0, 1]})] {
            let mut p = json!({"path": "a.wav"});
            p.as_object_mut().unwrap().extend(typo.as_object().unwrap().clone());
            assert!(serde_json::from_value::<Rule>(json!({"match": {"tag": "horn"}, "action": "layer", "play": p})).is_err(), "{typo}");
        }
    }
}
