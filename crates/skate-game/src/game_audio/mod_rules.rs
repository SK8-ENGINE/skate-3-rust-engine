//! Declarative mute / replace / layer rules on the event sites (doc 16 "Rules"; capability
//! `audio_events` = 2; the format: `skate_mods::audio_rules`). Lua cannot run inside the audio
//! pass, so a mod declares rules (`sdk.audio.rule(key, rule)` or `audio.json` `rules`) and the
//! sites apply them as they post, the same frame:
//!
//! - the local player's component posts and Splice starts, the world host's and the NPC host's
//!   posts and Splice starts (`PlayerAudio::apply`, `world_sources::apply`, `NpcHost::apply`, the
//!   `mod_audio::Observed` Splice wrapper) and the world emitters' `c_emitter` starts
//!   (`emitters::update`);
//! - `mute` drops the request (a post is not made: its later redeliveries and its release find no
//!   node and do nothing; a Splice sound does not start; an emitter keeps its state and posts
//!   nothing); `replace` drops it and plays the rule's WAV; `layer` keeps it and plays the WAV;
//! - a rule's WAV plays through the native mixer (`mod_voices`), opened in the same pass for the
//!   sites before `mod_voices::frame` (the local player, the world / NPC owners' process) and in
//!   the next for the later ones (their update, the emitters). It is positional (user decision
//!   2026-10-04): by default at the owner of the game's sound (the request's source and owner
//!   travel with it, [`Origin`]; `mod_voices` follows the owner every frame), or where the rule
//!   says (`play.at`: a fixed world position, or centred); the reach defaults to the owner's
//!   retail one (`mod_voices::locate`).
//!
//! The compiled [`RuleSet`] is shared (`Arc`) with the sites; with no rules every site holds
//! `None` and checks one branch, so without rules the game sounds exactly as before. The first
//! matching rule decides (mods in mod-id order; per mod the `audio.json` rules, then the runtime
//! ones, by key). Event rows are the game's requests: a muted request is still reported.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use skate_mods::audio_rules::{MAX_RULES, MAX_RULES_PER_MOD, Rule, RuleAction};

use super::mod_audio::{EventKind, EventRow, Source, Tags};
use super::mod_voices::{Anchor, Follow, ModVoices, Reach, VoiceSpec};

/// Concurrent voices of one rule's sound (a newer play replaces the oldest).
const VOICES_PER_RULE: usize = 4;

/// Where a rule's sound plays (`skate_mods::audio_rules::RuleAt`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Placement {
    /// At the owner plus this offset, followed; the offset in the owner's axes when the flag is set
    /// (`play.frame = 'owner'`), else world axes.
    Owner(Vec3, bool),
    /// At a fixed world position.
    World(Vec3),
    /// Non-positional, centred.
    Centre,
}

/// A rule's sound: the voice (no position yet) and where it plays.
struct Play {
    voice: VoiceSpec,
    at: Placement,
}

/// The request a rule's sound answers: its source and owner (the event row's), and for an emitter
/// start the record's position and reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Origin {
    source: Source,
    owner: u64,
    site: Option<(Vec3, Reach)>,
    /// A published emitter's entity (`WorldEmitter`): the sound follows it (it can move).
    entity: Option<u64>,
}

impl Origin {
    /// The owner to place the sound at (None: an emitter request without its record).
    fn anchor(&self) -> Option<Anchor> {
        Some(match self.source {
            Source::Player => Anchor::Player,
            Source::World => Anchor::World(self.owner),
            Source::Npc => Anchor::Npc(self.owner),
            Source::Emitter => {
                let (p, r) = self.site?;
                match self.entity {
                    Some(e) => Anchor::Emitter(e, p, r),
                    None => Anchor::Fixed(p, r),
                }
            }
            // Not rule sites (zone changes, speech rows).
            Source::Ambience | Source::Speech => return None,
        })
    }
}

impl Play {
    /// The voice for one request: positioned at its owner (followed), at the rule's fixed
    /// position, or centred; an owner that cannot be named (no emitter record) plays it centred.
    fn spec(&self, origin: &Origin) -> VoiceSpec {
        let mut v = self.voice.clone();
        let anchor = origin.anchor();
        match (self.at, anchor) {
            (Placement::Owner(offset, local), Some(anchor)) => {
                // The position comes from the owner at the voices' frame, before any word is read.
                v.position = Some(Vec3::ZERO);
                v.follow = Some(Follow { anchor, offset, track: true, local });
            }
            (Placement::World(p), anchor) => {
                v.position = Some(p);
                v.follow = anchor.map(|anchor| Follow { anchor, offset: Vec3::ZERO, track: false, local: false });
            }
            (Placement::Centre, _) | (Placement::Owner(..), None) => {
                v.position = None;
                v.reach = None;
            }
        }
        v
    }
}

/// A rule ready for the sites.
struct Compiled {
    owner: String,
    key: String,
    tag: Option<String>,
    kind: Option<EventKind>,
    source: Option<Source>,
    class: Option<String>,
    slot: Option<String>,
    id: Option<i32>,
    action: RuleAction,
    play: Option<Play>,
    min_interval: f64,
}

impl Compiled {
    fn matches(&self, r: &EventRow, tag: Option<&'static str>) -> bool {
        self.tag.as_deref().is_none_or(|t| tag == Some(t))
            && self.kind.is_none_or(|k| k == r.kind)
            && self.source.is_none_or(|s| s == r.source)
            && self.class.as_deref().is_none_or(|c| c == r.class)
            && self.slot.as_deref().is_none_or(|s| s == r.slot)
            && self.id.is_none_or(|i| i == r.id)
    }
}

#[derive(Default)]
struct Clock {
    now: f64,
    last: Vec<f64>,
    /// Rules whose sound is to play (in order) and the request each answers.
    plays: Vec<(usize, Origin)>,
}

/// The compiled rules, shared with the sites.
pub(crate) struct RuleSet {
    rules: Vec<Compiled>,
    tags: Tags,
    clock: Mutex<Clock>,
    pending: AtomicBool,
}

impl RuleSet {
    /// One request at a site: whether to drop it (`mute`, `replace`); a `replace` / `layer`
    /// queues its sound (at most once per `min_interval`), placed at the request's owner.
    pub(crate) fn mutes(&self, row: &EventRow) -> bool {
        self.mutes_at(row, None)
    }

    /// [`Self::mutes`] with the site's own position and reach (an emitter start: its record).
    pub(crate) fn mutes_at(&self, row: &EventRow, site: Option<(Vec3, Reach)>) -> bool {
        self.mutes_at_published(row, site, None)
    }

    /// [`Self::mutes_at`] for a published emitter (`WorldEmitter` entity): its sound follows it.
    pub(crate) fn mutes_at_published(&self, row: &EventRow, site: Option<(Vec3, Reach)>, entity: Option<u64>) -> bool {
        let tag = self.tags.tag(row);
        let Some((i, r)) = self.rules.iter().enumerate().find(|(_, r)| r.matches(row, tag)) else { return false };
        if r.play.is_some() {
            if let Ok(mut c) = self.clock.lock() {
                let now = c.now;
                if let Some(last) = c.last.get_mut(i) {
                    if now - *last >= r.min_interval {
                        *last = now;
                        c.plays.push((i, Origin { source: row.source, owner: row.owner, site, entity }));
                        self.pending.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
        r.action != RuleAction::Layer
    }

    /// Whether sounds wait to be played (`mod_voices::frame`).
    pub(crate) fn has_plays(&self) -> bool {
        self.pending.load(Ordering::Relaxed)
    }

    /// The queued sounds as (mod, voice key, spec), each placed for its request: each rule plays
    /// through a ring of [`VOICES_PER_RULE`] keys.
    pub(crate) fn take_plays(&self, counter: &mut u64) -> Vec<(String, String, VoiceSpec)> {
        self.pending.store(false, Ordering::Relaxed);
        let plays = self.clock.lock().map(|mut c| std::mem::take(&mut c.plays)).unwrap_or_default();
        plays
            .into_iter()
            .filter_map(|(i, origin)| {
                let r = self.rules.get(i)?;
                let spec = r.play.as_ref()?.spec(&origin);
                *counter += 1;
                Some((r.owner.clone(), format!("__rule.{}.{}", r.key, *counter as usize % VOICES_PER_RULE), spec))
            })
            .collect()
    }

    fn set_now(&self, now: f64) {
        if let Ok(mut c) = self.clock.lock() {
            c.now = now;
        }
    }
}

/// Rules by owner: the runtime ones (`sdk.audio.rule`), and the compiled set.
#[derive(Resource, Default)]
pub(crate) struct AudioRules {
    runtime: BTreeMap<(String, String), Rule>,
    dirty: bool,
    /// `AudioContent::rules_generation` compiled last.
    content: u64,
    /// The pop / landing ids the set was compiled with (the Contacts tuning).
    tags: Option<Tags>,
    pub(crate) set: Option<Arc<RuleSet>>,
    /// Rules left out at the last compile (their WAV did not load), for the log.
    pub(crate) failed: Vec<String>,
}

impl AudioRules {
    /// Set (`Some`) or remove (`None`) `owner`'s rule `key`.
    pub(crate) fn set_rule(&mut self, owner: &str, key: &str, rule: Option<Rule>) -> Result<(), String> {
        let k = (owner.to_owned(), key.to_owned());
        match rule {
            None => {
                self.dirty |= self.runtime.remove(&k).is_some();
            }
            Some(rule) => {
                if !rule.validate() {
                    return Err("audio rule: a rule needs a known match, an action and (replace / layer) a play".into());
                }
                if !self.runtime.contains_key(&k) {
                    if self.runtime.keys().filter(|(o, _)| o == owner).count() >= MAX_RULES_PER_MOD {
                        return Err(format!("audio rule: {MAX_RULES_PER_MOD} rules per mod maximum"));
                    }
                    if self.runtime.len() >= MAX_RULES {
                        return Err(format!("audio rule: {MAX_RULES} rules in all maximum"));
                    }
                }
                self.runtime.insert(k, rule);
                self.dirty = true;
            }
        }
        Ok(())
    }

    /// A mod stopped, failed or was reloaded: its runtime rules go.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        let before = self.runtime.len();
        self.runtime.retain(|(o, _), _| o != owner);
        self.dirty |= self.runtime.len() != before;
    }

    pub(crate) fn count(&self) -> usize {
        self.set.as_ref().map_or(0, |s| s.rules.len())
    }
}

/// A rule's `play` as a one-shot voice (no position yet) and its placement.
fn play(p: &skate_mods::audio_rules::RulePlay, group: u8) -> Play {
    use skate_mods::audio_rules::RuleAt;
    let v3 = |a: Option<[f32; 3]>| a.map_or(Vec3::ZERO, Vec3::from_array);
    let at = match p.at {
        RuleAt::Owner => Placement::Owner(v3(p.offset), p.frame == Some(skate_mods::audio_rules::RuleFrame::Owner)),
        RuleAt::World => Placement::World(v3(p.position)),
        RuleAt::Centre => Placement::Centre,
    };
    let voice = VoiceSpec { path: p.path.clone(), looping: false, volume: p.volume, pitch: p.pitch, paused: false, fade_in: 0.0, position: None, reach: p.falloff.map(Reach::from_falloff), follow: None, reverb: p.reverb.unwrap_or(true), group };
    Play { voice, at }
}

fn kind(k: &str) -> Option<EventKind> {
    Some(match k {
        "post" => EventKind::Post,
        "splice" => EventKind::Splice,
        "emitter_start" => EventKind::EmitterStart,
        _ => return None,
    })
}

fn source(s: &str) -> Option<Source> {
    Some(match s {
        "player" => Source::Player,
        "world" => Source::World,
        "npc" => Source::Npc,
        "emitter" => Source::Emitter,
        _ => return None,
    })
}

/// Compile every running rule: `audio.json` rules (their WAVs read from the mod root now) and
/// runtime ones (their WAVs loaded by the command), each sound a slot of its mod's bank.
fn compile(rules: &mut AudioRules, content: &super::AudioContent, voices: &mut ModVoices, tags: Tags) -> Option<Arc<RuleSet>> {
    let mut all: Vec<(String, u8, String, Rule, Option<std::path::PathBuf>)> = Vec::new();
    for (id, reg) in &content.overlays {
        for (key, rule) in &reg.overlay.rules {
            all.push((id.clone(), 0, key.clone(), rule.clone(), Some(reg.root.clone())));
        }
    }
    for ((owner, key), rule) in &rules.runtime {
        all.push((owner.clone(), 1, key.clone(), rule.clone(), None));
    }
    all.sort_by(|a, b| (&a.0, a.1, &a.2).cmp(&(&b.0, b.1, &b.2)));
    all.truncate(MAX_RULES);
    rules.failed.clear();
    let mut compiled = Vec::new();
    for (owner, _, key, rule, root) in all {
        let play = match &rule.play {
            None => None,
            Some(p) => {
                if let Some(root) = root.filter(|_| !voices.has_clip(&owner, &p.path)) {
                    let loaded = skate_mods::read_bounded(&root, &p.path, skate_mods::audio::MAX_WAV_BYTES)
                        .and_then(|b| skate_mods::audio::canonical_pcm_wav(&b).map(|(w, _)| w))
                        .and_then(|w| voices.add_clip(&owner, &p.path, &w));
                    if let Err(e) = loaded {
                        rules.failed.push(format!("{owner}: rule {key}: {e}"));
                        continue;
                    }
                }
                let group = u8::from(p.group.as_deref() != Some("world"));
                if let Err(e) = voices.slot(&owner, &p.path, false, group) {
                    rules.failed.push(format!("{owner}: rule {key}: {e}"));
                    continue;
                }
                Some(play(p, group))
            }
        };
        let on = &rule.on;
        compiled.push(Compiled {
            owner,
            key,
            tag: on.tag.clone(),
            kind: on.kind.as_deref().and_then(kind),
            source: on.source.as_deref().and_then(source),
            class: on.class.clone(),
            slot: on.slot.clone(),
            id: on.id,
            action: rule.action,
            play,
            min_interval: f64::from(rule.min_interval),
        });
    }
    for f in &rules.failed {
        warn!("Game audio: {f}");
    }
    if compiled.is_empty() {
        return None;
    }
    let n = compiled.len();
    info!("Game audio: {n} audio rules");
    Some(Arc::new(RuleSet { rules: compiled, tags, clock: Mutex::new(Clock { now: 0.0, last: vec![f64::NEG_INFINITY; n], plays: Vec::new() }), pending: AtomicBool::new(false) }))
}

/// The start of the audio pass (after the event frame): compile the rules when they changed and
/// hand the set to the sites (they hold `None` without rules); the rules' clock.
#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    mut rules: ResMut<AudioRules>,
    content: Res<super::AudioContent>,
    mut voices: ResMut<ModVoices>,
    mut native: Option<ResMut<super::Native>>,
    mut world: Option<ResMut<super::world_sources::WorldHost>>,
    mut npc: Option<ResMut<super::npc_skaters::NpcHost>>,
    time: Res<Time<Real>>,
) {
    let player_tags = native.as_deref().and_then(|n| n.player.as_ref()).map(|p| Tags::from_tuning(&p.contact_tuning));
    let tag_key = player_tags;
    let changed = rules.dirty || rules.content != content.rules_generation || (rules.set.is_some() && rules.tags != tag_key);
    if changed {
        rules.dirty = false;
        rules.content = content.rules_generation;
        rules.tags = tag_key;
        let set = compile(&mut rules, &content, &mut voices, player_tags.unwrap_or_default());
        rules.set = set;
    }
    let set = rules.set.clone();
    let same = |held: &Option<Arc<RuleSet>>| match (held, &set) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    };
    if let Some(p) = native.as_deref_mut().and_then(|n| n.player.as_mut()).filter(|p| !same(&p.rules)) {
        p.rules = set.clone();
    }
    if let Some(w) = world.as_deref_mut().filter(|w| !same(&w.rules)) {
        w.rules = set.clone();
    }
    if let Some(n) = npc.as_deref_mut().filter(|n| !same(&n.rules)) {
        n.rules = set.clone();
    }
    if let Some(s) = &set {
        s.set_now(time.elapsed_secs_f64());
    }
}

#[cfg(test)]
impl RuleSet {
    /// A set from (owner, key, rule) in the given order, without loading sounds (tests).
    pub(crate) fn for_test(rules: &[(&str, &str, Rule)], tags: Tags) -> Arc<Self> {
        let compiled: Vec<Compiled> = rules
            .iter()
            .map(|(owner, key, rule)| Compiled {
                owner: (*owner).to_owned(),
                key: (*key).to_owned(),
                tag: rule.on.tag.clone(),
                kind: rule.on.kind.as_deref().and_then(kind),
                source: rule.on.source.as_deref().and_then(source),
                class: rule.on.class.clone(),
                slot: rule.on.slot.clone(),
                id: rule.on.id,
                action: rule.action,
                play: rule.play.as_ref().map(|p| play(p, 1)),
                min_interval: f64::from(rule.min_interval),
            })
            .collect();
        let n = compiled.len();
        Arc::new(Self { rules: compiled, tags, clock: Mutex::new(Clock { now: 0.0, last: vec![f64::NEG_INFINITY; n], plays: Vec::new() }), pending: AtomicBool::new(false) })
    }

    pub(crate) fn advance(&self, now: f64) {
        self.set_now(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(v: serde_json::Value) -> Rule {
        serde_json::from_value(v).unwrap()
    }

    fn row(kind: EventKind, source: Source, class: &'static str, slot: &'static str, id: i32) -> EventRow {
        EventRow { kind, source, class, slot, id, owner: 0, hit: None }
    }

    /// Matching: every given field must hold; the first matching rule decides; mute / replace drop
    /// the request, layer keeps it; replace / layer queue their sound at most once per
    /// `min_interval`, through a ring of keys.
    #[test]
    fn rules_match_decide_and_queue() {
        let tags = Tags { pop: [12, 13, 0, 0, 0, 0], land: 20 };
        let set = RuleSet::for_test(&[
            ("dev.a", "quiet_land", rule(json!({"match": {"tag": "land"}, "action": "mute"}))),
            ("dev.a", "my_pop", rule(json!({"match": {"tag": "pop"}, "action": "replace", "play": {"path": "pop.wav"}, "min_interval": 0.1}))),
            ("dev.b", "pop_too", rule(json!({"match": {"tag": "pop"}, "action": "mute"}))),
            ("dev.b", "horns", rule(json!({"match": {"source": "world", "class": "TRAFFIC_HORN"}, "action": "layer", "play": {"path": "honk.wav"}, "min_interval": 0}))),
            ("dev.b", "grind2", rule(json!({"match": {"kind": "post", "slot": "grind", "id": 2}, "action": "mute"}))),
        ], tags);
        let bank = skate_audio::player::contacts::BANK;
        let pop = row(EventKind::Splice, Source::Player, bank, "", 12);
        assert!(set.mutes(&row(EventKind::Splice, Source::Player, bank, "", 20)), "land muted");
        assert!(set.mutes(&pop), "pop replaced (dropped)");
        assert!(set.has_plays());
        assert!(set.mutes(&pop), "again: dropped, but within min_interval no second sound");
        let mut n = 0;
        let plays = set.take_plays(&mut n);
        assert_eq!(plays.len(), 1);
        assert_eq!((plays[0].0.as_str(), plays[0].1.as_str(), plays[0].2.path.as_str()), ("dev.a", "__rule.my_pop.1", "pop.wav"));
        assert!(!set.has_plays());
        set.advance(0.2);
        assert!(set.mutes(&pop));
        assert_eq!(set.take_plays(&mut n)[0].1, "__rule.my_pop.2", "the next key of the ring");
        // Layer: kept, the sound queued; another source / class does not match.
        assert!(!set.mutes(&row(EventKind::Post, Source::World, "TRAFFIC_HORN", "horn", 0)));
        assert!(!set.mutes(&row(EventKind::Post, Source::Npc, "TRAFFIC_HORN", "horn", 0)));
        assert_eq!(set.take_plays(&mut n).len(), 1);
        // Slot and index both must hold.
        assert!(set.mutes(&row(EventKind::Post, Source::Player, "Class_grind", "grind", 2)));
        assert!(!set.mutes(&row(EventKind::Post, Source::Player, "Class_grind", "grind", 0)));
        assert!(!set.mutes(&row(EventKind::Release, Source::Player, "", "grind", 2)), "releases never match");
    }

    /// Where the rules' sounds play (user decision 2026-10-04): at the request's owner by default
    /// (the local skater, a world owner, an NPC skater, an emitter record with its own reach),
    /// followed, plus an offset; at a fixed world position (the owner still gives the default
    /// reach); centred; a mod's `falloff` replaces the default reach.
    #[test]
    fn rule_sounds_are_placed_at_their_owner() {
        let tags = Tags { pop: [12, 0, 0, 0, 0, 0], land: 20 };
        let set = RuleSet::for_test(&[
            ("dev.a", "pop", rule(json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "a.wav"}, "min_interval": 0}))),
            ("dev.a", "horn", rule(json!({"match": {"tag": "horn"}, "action": "replace", "play": {"path": "a.wav", "offset": [0, 1.5, 0], "falloff": {"radius": 12, "curve": "linear"}}, "min_interval": 0}))),
            ("dev.a", "npc", rule(json!({"match": {"source": "npc"}, "action": "replace", "play": {"path": "a.wav", "at": "world", "position": [10, 0, -4]}, "min_interval": 0}))),
            ("dev.a", "land", rule(json!({"match": {"tag": "land"}, "action": "replace", "play": {"path": "a.wav", "at": "centre"}, "min_interval": 0}))),
            ("dev.a", "emit", rule(json!({"match": {"kind": "emitter_start"}, "action": "replace", "play": {"path": "a.wav"}, "min_interval": 0}))),
        ], tags);
        let bank = skate_audio::player::contacts::BANK;
        let mut n = 0;
        let mut one = |row: EventRow, site: Option<(Vec3, Reach)>| {
            set.mutes_at(&row, site);
            let mut plays = set.take_plays(&mut n);
            assert_eq!(plays.len(), 1, "{row:?}");
            plays.remove(0).2
        };
        // The local player's pop: at the skater, followed, its default reach found at play time.
        let pop = one(row(EventKind::Splice, Source::Player, bank, "", 12), None);
        assert_eq!(pop.follow, Some(Follow { anchor: Anchor::Player, offset: Vec3::ZERO, track: true, local: false }));
        assert!(pop.position.is_some() && pop.reach.is_none());
        // A car's horn: at the car (owner 7) 1.5 m up, the mod's reach.
        let horn = one(EventRow { kind: EventKind::Post, source: Source::World, class: skate_audio::world::traffic::HORN_CLASS, slot: "horn", id: 0, owner: 7, hit: None }, None);
        assert_eq!(horn.follow, Some(Follow { anchor: Anchor::World(7), offset: Vec3::new(0.0, 1.5, 0.0), track: true, local: false }));
        assert_eq!(horn.reach, Some(Reach::sphere(12.0, 0.0, 1)));
        // An NPC skater's post at a fixed world position: not followed; the skater gives the reach.
        let npc = one(EventRow { kind: EventKind::Post, source: Source::Npc, class: "Class_grind", slot: "grind", id: 0, owner: 9, hit: None }, None);
        assert_eq!(npc.position, Some(Vec3::new(10.0, 0.0, -4.0)));
        assert_eq!(npc.follow, Some(Follow { anchor: Anchor::Npc(9), offset: Vec3::ZERO, track: false, local: false }));
        // Centred: no position, no follow.
        let land = one(row(EventKind::Splice, Source::Player, bank, "", 20), None);
        assert_eq!((land.position, land.follow, land.reach), (None, None, None));
        // An emitter start: at the record with the record's reach.
        let reach = Reach { extent: Vec3::new(20.0, 5.0, 8.0), forward: Vec3::Z, core: 0.2, curve: 1 };
        let at = Vec3::new(3.0, 1.0, -7.0);
        let emit = one(EventRow { kind: EventKind::EmitterStart, source: Source::Emitter, class: "Baby_Cry_1", slot: "", id: 22, owner: 4, hit: None }, Some((at, reach)));
        assert_eq!(emit.follow, Some(Follow { anchor: Anchor::Fixed(at, reach), offset: Vec3::ZERO, track: true, local: false }));
        // A published emitter's start: its entity travels with the sound (it may move).
        set.mutes_at_published(&EventRow { kind: EventKind::EmitterStart, source: Source::Emitter, class: "Baby_Cry_1", slot: "", id: 22, owner: 77, hit: None }, Some((at, reach)), Some(77));
        let moving = set.take_plays(&mut 100).remove(0).2;
        assert_eq!(moving.follow, Some(Follow { anchor: Anchor::Emitter(77, at, reach), offset: Vec3::ZERO, track: true, local: false }));
        // An emitter request without its record cannot be placed: centred.
        let lost = one(EventRow { kind: EventKind::EmitterStart, source: Source::Emitter, class: "Baby_Cry_1", slot: "", id: 22, owner: 4, hit: None }, None);
        assert_eq!((lost.position, lost.follow), (None, None));
    }

    /// The owners' positions and retail reaches (`mod_voices::locate`): the skater's centre of
    /// mass (30 m), a car (40 m) or ped (50 m) by id, an NPC skater (30 m), a fixed record; unknown
    /// ids are not found.
    #[test]
    fn owners_are_located_with_their_retail_reach() {
        use super::super::mod_voices::locate;
        let mut cues = super::super::skate_events::Cues::default();
        cues.riding.audio.com_position = [1.0, 2.0, 3.0];
        let mut owners = super::super::world_sources::WorldOwners::default();
        owners.vehicles.insert(7, skate_audio::world::traffic::VehicleState { position: [5.0, 0.0, 0.0], ..Default::default() });
        let ped = skate_audio::world::peds::PedState { position: [0.0, 0.0, 9.0], ..Default::default() };
        owners.peds.insert(8, ped);
        let player = locate(&Anchor::Player, &cues, None, None).unwrap();
        assert_eq!(player, (Vec3::new(1.0, 2.0, 3.0), Reach::sphere(30.0, 0.0, 0)));
        assert_eq!(locate(&Anchor::World(7), &cues, Some(&owners), None), Some((Vec3::new(5.0, 0.0, 0.0), Reach::sphere(40.0, 0.0, 0))));
        assert_eq!(locate(&Anchor::World(8), &cues, Some(&owners), None), Some((Vec3::new(0.0, 0.0, 9.0), Reach::sphere(50.0, 0.0, 0))));
        assert_eq!(locate(&Anchor::World(99), &cues, Some(&owners), None), None);
        assert_eq!(locate(&Anchor::Npc(3), &cues, None, None), None);
        let r = Reach::sphere(7.0, 0.1, 2);
        assert_eq!(locate(&Anchor::Fixed(Vec3::X, r), &cues, None, None), Some((Vec3::X, r)));
    }

    /// Runtime rules: limits, removal, the owner's cleanup; `audio.json` rules come with the
    /// running overlay without an audio restart (rules only) and compile with their WAV in the
    /// mod's bank; a rule whose WAV is missing is left out.
    #[test]
    fn runtime_and_overlay_rules_compile() {
        let mut r = AudioRules::default();
        let mute = rule(json!({"match": {"tag": "pop"}, "action": "mute"}));
        for i in 0..MAX_RULES_PER_MOD {
            r.set_rule("dev.a", &format!("r{i}"), Some(mute.clone())).unwrap();
        }
        assert!(r.set_rule("dev.a", "one_more", Some(mute.clone())).unwrap_err().contains("per mod"));
        r.set_rule("dev.a", "r0", None).unwrap();
        r.clear_owner("dev.a");
        assert!(r.runtime.is_empty() && r.dirty);
        // An overlay of rules only.
        let dir = std::env::temp_dir().join(format!("skate-audio-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join("audio/pop.wav"), super::super::mod_voices::tests::tone_wav(800.0, 0.1, 48000, 0.5)).unwrap();
        std::fs::write(dir.join("audio.json"), json!({"version": 1, "rules": {"my_pop": {"match": {"tag": "pop"}, "action": "replace", "play": {"path": "audio/pop.wav"}}}}).to_string()).unwrap();
        let mut content = super::super::AudioContent::default();
        content.sync([("dev.a", dir.as_path(), 1)].into_iter());
        assert_eq!(content.rules_generation, 1);
        assert!(!content.restart_pending(), "rules alone do not restart the sound");
        let mut voices = ModVoices::default();
        r.set_rule("dev.b", "quiet", Some(mute.clone())).unwrap();
        let set = compile(&mut r, &content, &mut voices, Tags { pop: [5, 0, 0, 0, 0, 0], land: 6 }).expect("two rules");
        assert_eq!(set.rules.iter().map(|c| (c.owner.as_str(), c.key.as_str())).collect::<Vec<_>>(), [("dev.a", "my_pop"), ("dev.b", "quiet")], "mod-id order");
        assert!(voices.has_clip("dev.a", "audio/pop.wav"), "the overlay rule's WAV is in the mod's bank");
        // The mod stops: its overlay rules go (a recompile), still no restart.
        content.sync(std::iter::empty());
        assert_eq!(content.rules_generation, 2);
        assert!(!content.restart_pending());
        // A missing WAV leaves the rule out (logged), the others stay.
        std::fs::remove_file(dir.join("audio/pop.wav")).unwrap();
        let mut c2 = super::super::AudioContent::default();
        std::fs::write(dir.join("audio.json"), json!({"version": 1, "rules": {"q": {"match": {"tag": "land"}, "action": "mute"}}}).to_string()).unwrap();
        c2.sync([("dev.c", dir.as_path(), 1)].into_iter());
        let set = compile(&mut r, &c2, &mut ModVoices::default(), Tags::default()).unwrap();
        assert_eq!(set.rules.len(), 2);
        r.clear_owner("dev.b");
        assert!(compile(&mut r, &super::super::AudioContent::default(), &mut ModVoices::default(), Tags::default()).is_none(), "no rules: no set");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rule tags the mod side accepts are event tags of the engine (the ones on post sites).
    #[test]
    fn rule_tags_are_engine_tags() {
        for t in skate_mods::audio_rules::TAGS {
            assert!(super::super::mod_audio::TAGS.contains(&t), "{t}");
        }
        for k in skate_mods::audio_rules::KINDS {
            assert!(kind(k).is_some(), "{k}");
        }
        for s in skate_mods::audio_rules::SOURCES {
            assert!(source(s).is_some(), "{s}");
        }
    }
}
