//! The runtime audio API (doc 16 "Audio modding", R3(a) and R5), the same for mods
//! (`sdk.audio.post / redeliver / release / set_global / watch / subscribe / events`) and engine
//! systems ([`AudioApi`]):
//! - retail posts by class name with key-scoped handles (a key's post replaces its last one),
//!   queued and applied at one fixed point, the start of the audio pass ([`drain`], before the
//!   local player's process), so their order against retail's posts is always the same;
//! - a handle is dead after a map change or an audio restart (`Native::map_epoch`, the content
//!   generation): it reads `live = false` and is never released into another runtime;
//! - globals: the first owner wins, the value seen before its first write comes back on `nil`,
//!   when the owner stops and at a map change; overrides are applied again after a restart;
//! - a read-only watch of MixMap outputs and globals, copied after the pass's ticks ([`readback`]);
//! - audio events, observe only: the post sites (the local player's components and Splice
//!   starts, the world and NPC hosts, the world emitters, the zone ambience, speech) append
//!   compact rows while some mod subscribes; nothing is recorded without a subscriber.
//!
//! Parity note: a mod post runs the bank's program, which draws from the evaluator's one random
//! generator as every retail post does, so with a mod posting the retail random sequence differs
//! from a run without it. Without mod posts nothing changes.
use std::collections::BTreeMap;

use bevy::prelude::*;
use serde_json::{Value, json};
use skate_audio::eval::NodeId;

use super::native::Native;

pub(crate) const MAX_HANDLES_PER_MOD: usize = 32;
pub(crate) const MAX_HANDLES: usize = 128;
/// Posts queued per mod and frame.
pub(crate) const MAX_POSTS_PER_FRAME: usize = 16;
/// Globals / MixMap outputs one mod may watch (each list).
pub(crate) const MAX_WATCH: usize = 16;
/// Event rows kept per frame (all sources), and delivered per mod and frame.
pub(crate) const MAX_EVENTS: usize = 256;

#[derive(Clone, Debug)]
enum Op {
    Post { owner: String, key: String, class: usize, name: String, words: Vec<i32> },
    Redeliver { owner: String, key: String, words: Vec<i32> },
    Release { owner: String, key: String },
    Global { owner: String, id: usize, name: String, value: Option<i32> },
}

#[derive(Clone, Debug)]
struct Handle {
    node: Option<NodeId>,
    epoch: u64,
    generation: u64,
    class: String,
}

#[derive(Clone, Debug)]
struct GlobalOverride {
    owner: String,
    id: usize,
    original: i32,
    value: i32,
}

/// One MixMap output a mod watches.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MixMapWatch {
    pub slot: String,
    pub object: u32,
    pub instance: u32,
    pub output: u32,
    key: u32,
}

#[derive(Clone, Debug, Default)]
struct Watch {
    globals: Vec<(String, usize)>,
    mixmap: Vec<MixMapWatch>,
}

/// The MixMap slots by name (`skate_audio::mixmap::keys::slot`).
pub(crate) fn slot_number(name: &str) -> Option<u32> {
    use skate_audio::mixmap::keys::slot;
    Some(match name {
        "global" => slot::GLOBAL,
        "player" => slot::PLAYER,
        "ambience" => slot::AMBIENCE,
        "collision" => slot::COLLISION,
        "traffic" => slot::TRAFFIC,
        "pedestrian" => slot::PEDESTRIAN,
        "emitter" => slot::EMITTER,
        _ => return None,
    })
}

// ---- events (R5) ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventKind {
    Post,
    Release,
    Splice,
    EmitterStart,
    EmitterStop,
    Zone,
    Speech,
    /// A body region's hit posted by the body poster (`sub_824BC188`), with its data in
    /// [`EventRow::hit`]: observe only, no sound of its own (the hit's contacts are posts).
    BodyImpact,
}

impl EventKind {
    fn name(self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Release => "release",
            Self::Splice => "splice",
            Self::EmitterStart => "emitter_start",
            Self::EmitterStop => "emitter_stop",
            Self::Zone => "zone",
            Self::Speech => "speech",
            Self::BodyImpact => "body_impact",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Player,
    World,
    Npc,
    Emitter,
    Ambience,
    Speech,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::World => "world",
            Self::Npc => "npc",
            Self::Emitter => "emitter",
            Self::Ambience => "ambience",
            Self::Speech => "speech",
        }
    }
}

/// One audio event: plain data, no allocation (`class`: a retail class / bank / slot name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EventRow {
    pub kind: EventKind,
    pub source: Source,
    /// The class (posts), bank (Splice starts, emitters), or "".
    pub class: &'static str,
    /// The poster's slot (`grind`, `footstep`, `horn`, …) for posts and releases, or "".
    pub slot: &'static str,
    /// Splice sound id, emitter patch, slot index, speech event; 0 otherwise.
    pub id: i32,
    /// The world / NPC object, the zone key, the speaker; 0 for the local player.
    pub owner: u64,
    /// A body impact's region, impact, tiers, materials and position ([`EventKind::BodyImpact`]).
    pub hit: Option<skate_audio::player::contacts::BodyHit>,
}

/// The event row of a body hit (`Contacts::body_hits`) of the local player (owner 0) or an NPC.
pub(crate) fn body_impact_row(source: Source, owner: u64, hit: skate_audio::player::contacts::BodyHit) -> EventRow {
    EventRow { kind: EventKind::BodyImpact, source, class: "", slot: "body", id: hit.region as i32, owner, hit: Some(hit) }
}

/// A post site's buffer: Some while a mod subscribes (`AudioApi::events_on`).
pub(crate) type EventBuf = Option<Vec<EventRow>>;

/// Append a row when recording (one branch otherwise).
#[inline]
pub(crate) fn record(buf: &mut EventBuf, row: EventRow) {
    if let Some(rows) = buf {
        if rows.len() < MAX_EVENTS {
            rows.push(row);
        }
    }
}

/// The named tags on retail identities (each tested against a real post):
/// - `pop` / `land`: the board contacts' Splice starts of the pop ids and the landing id (the
///   Contacts tuning of the install);
/// - `grind_start` / `grind_end`: the grind slot's post / release;
/// - `footstep`: a footstep Splice start (`fstep_*` banks) or a `playercharacter_footstep` /
///   `livingword_footstep` post;
/// - `horn` / `alarm`: a traffic horn / car alarm post;
/// - `tazer`: a ped's `c_tazer` post; `body_fall`: a ped's body-fall Splice start;
/// - `emitter`: a world emitter start; `zone_change`: a zone ambience change; `speech`: a line;
/// - `body_impact`: a body region's hit of the local player or an NPC skater (`sub_824BC188`), with
///   the region, impact, tiers, materials and position (observe only: not a rule tag).
///
/// The pop / land ids are the running player's (`events_frame` copies them every frame a mod
/// subscribes, so they follow a native start, restart or tuning change); 0 is "unset" and never
/// tags a row (`Tags::default` tags no pop or landing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tags {
    pub pop: [i32; 6],
    pub land: i32,
}

pub(crate) const TAGS: [&str; 13] = ["pop", "land", "grind_start", "grind_end", "footstep", "horn", "alarm", "tazer", "body_fall", "emitter", "zone_change", "speech", "body_impact"];

impl Tags {
    pub(crate) fn from_tuning(c: &skate_audio::player::contacts::ContactsTuning) -> Self {
        let mut pop = [0; 6];
        for (p, &i) in pop.iter_mut().zip(c.pop_ids.iter().chain(&c.pop_ids_hollow)) {
            *p = i as i32;
        }
        Self { pop, land: c.landing_id as i32 }
    }

    pub(crate) fn tag(&self, r: &EventRow) -> Option<&'static str> {
        use skate_audio::player::contacts::BANK;
        use skate_audio::world::traffic::{ALARM_CLASS, HORN_CLASS};
        match (r.kind, r.source) {
            (EventKind::Splice, Source::Player) if r.class == BANK && r.id != 0 && self.pop.contains(&r.id) => Some("pop"),
            (EventKind::Splice, Source::Player) if r.class == BANK && r.id != 0 && r.id == self.land => Some("land"),
            (EventKind::Splice, _) if r.class.starts_with("fstep_") => Some("footstep"),
            (EventKind::Post, Source::Player) if r.slot == "grind" && r.id == 0 => Some("grind_start"),
            (EventKind::Release, Source::Player) if r.slot == "grind" && r.id == 0 => Some("grind_end"),
            (EventKind::Post, _) if r.class == skate_audio::player::footsteps::CLASS || r.class == skate_audio::world::peds::FOOTSTEP_CLASS => Some("footstep"),
            (EventKind::Post, Source::World) if r.class == HORN_CLASS => Some("horn"),
            (EventKind::Post, Source::World) if r.class == ALARM_CLASS => Some("alarm"),
            (EventKind::Post, Source::World) if r.class == skate_audio::world::peds::TAZER_CLASS => Some("tazer"),
            (EventKind::Splice, Source::World) if r.slot == "body_fall" => Some("body_fall"),
            (EventKind::EmitterStart, _) => Some("emitter"),
            (EventKind::Zone, _) => Some("zone_change"),
            (EventKind::Speech, _) => Some("speech"),
            (EventKind::BodyImpact, _) => Some("body_impact"),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Events {
    /// Mod → the tags it wants (empty = every row).
    subscribers: BTreeMap<String, Vec<String>>,
    /// This frame's rows (the sites append), last frame's (what mods read), and a counter of
    /// swaps (`serial`): a snapshot delivers `last` once per serial.
    pub(crate) current: Vec<EventRow>,
    last: Vec<EventRow>,
    truncated: bool,
    serial: u64,
    tags: Tags,
}

impl Events {
    /// Whether any mod subscribes (the sites record only then).
    pub(crate) fn on(&self) -> bool {
        !self.subscribers.is_empty()
    }

    pub(crate) fn push(&mut self, row: EventRow) {
        if !self.on() {
            return;
        }
        if self.current.len() < MAX_EVENTS {
            self.current.push(row);
        } else {
            self.truncated = true;
        }
    }

    /// The frame boundary: this frame's rows become what mods read.
    fn swap(&mut self) {
        std::mem::swap(&mut self.current, &mut self.last);
        self.current.clear();
        self.serial += 1;
    }

    /// One mod's rows of the last frame (its tags), as Lua sees them.
    fn rows_for(&self, owner: &str) -> Option<Value> {
        let tags = self.subscribers.get(owner)?;
        let mut rows = Vec::new();
        let mut truncated = self.truncated;
        for r in &self.last {
            let tag = self.tags.tag(r);
            if !tags.is_empty() && !tag.is_some_and(|t| tags.iter().any(|x| x == t)) {
                continue;
            }
            if rows.len() >= MAX_EVENTS {
                truncated = true;
                break;
            }
            let mut row = json!({"kind": r.kind.name(), "source": r.source.name(), "class": r.class, "slot": r.slot, "id": r.id,
                "owner": r.owner.to_string(), "tag": tag});
            if let Some(h) = r.hit {
                let m = h.message;
                row["region"] = json!(h.region);
                row["impact"] = json!(h.impact);
                row["tier"] = json!(m.tier);
                row["material"] = json!(m.material);
                row["position"] = json!(m.position);
            }
            rows.push(row);
        }
        Some(json!({"serial": self.serial, "rows": rows, "truncated": truncated}))
    }
}

#[derive(Resource, Default)]
pub(crate) struct AudioApi {
    queue: Vec<Op>,
    posts: BTreeMap<String, usize>,
    handles: BTreeMap<(String, String), Handle>,
    globals: BTreeMap<String, GlobalOverride>,
    watches: BTreeMap<String, Watch>,
    readback: BTreeMap<String, Value>,
    pub(crate) events: Events,
}

fn symbol(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl AudioApi {
    fn owned(&self, owner: &str) -> usize {
        self.handles.keys().filter(|(o, _)| o == owner).count()
    }

    /// Queue a post to a retail class for `owner`'s `key` (replacing the key's last post).
    pub(crate) fn post(&mut self, native: Option<&Native>, owner: &str, key: &str, class: &str, words: &[i32]) -> Result<(), String> {
        let native = native.ok_or("native audio is not running")?;
        if !symbol(class) || words.len() > skate_mods::audio_content::MAX_POST_WORDS {
            return Err(format!("audio post: bad class name or more than {} words", skate_mods::audio_content::MAX_POST_WORDS));
        }
        let id = native.shared.lock().map_err(|_| "audio lock poisoned")?.eval.class_id(class).ok_or_else(|| format!("audio post: no class {class}"))?;
        let slot = (owner.to_owned(), key.to_owned());
        if !self.handles.contains_key(&slot) {
            if self.owned(owner) >= MAX_HANDLES_PER_MOD {
                return Err(format!("audio post: {MAX_HANDLES_PER_MOD} handles per mod maximum"));
            }
            if self.handles.len() >= MAX_HANDLES {
                return Err(format!("audio post: {MAX_HANDLES} handles in all maximum"));
            }
        }
        let n = self.posts.entry(owner.to_owned()).or_default();
        if *n >= MAX_POSTS_PER_FRAME {
            return Err(format!("audio post: {MAX_POSTS_PER_FRAME} posts per frame maximum"));
        }
        *n += 1;
        // Reserved now (counts against the limits), posted at the drain.
        self.handles.entry(slot).or_insert(Handle { node: None, epoch: u64::MAX, generation: u64::MAX, class: class.to_owned() });
        self.queue.push(Op::Post { owner: owner.to_owned(), key: key.to_owned(), class: id, name: class.to_owned(), words: words.to_vec() });
        Ok(())
    }

    pub(crate) fn redeliver(&mut self, owner: &str, key: &str, words: &[i32]) -> Result<(), String> {
        if words.len() > skate_mods::audio_content::MAX_POST_WORDS {
            return Err("audio redeliver: too many words".into());
        }
        if !self.handles.contains_key(&(owner.to_owned(), key.to_owned())) {
            return Err(format!("audio redeliver: no post {key}"));
        }
        self.queue.push(Op::Redeliver { owner: owner.to_owned(), key: key.to_owned(), words: words.to_vec() });
        Ok(())
    }

    pub(crate) fn release(&mut self, owner: &str, key: &str) {
        if self.handles.contains_key(&(owner.to_owned(), key.to_owned())) {
            self.queue.push(Op::Release { owner: owner.to_owned(), key: key.to_owned() });
        }
    }

    /// Set (Some) or restore (None) a global; the first owner wins.
    pub(crate) fn set_global(&mut self, native: Option<&Native>, owner: &str, name: &str, value: Option<i32>) -> Result<(), String> {
        let native = native.ok_or("native audio is not running")?;
        if !symbol(name) {
            return Err("audio global: bad name".into());
        }
        if let Some(o) = self.globals.get(name).filter(|g| g.owner != owner) {
            return Err(format!("audio global {name} is owned by another mod ({})", o.owner));
        }
        let id = native.shared.lock().map_err(|_| "audio lock poisoned")?.eval.global_id(name).ok_or_else(|| format!("audio global: no global {name}"))?;
        self.queue.push(Op::Global { owner: owner.to_owned(), id, name: name.to_owned(), value });
        Ok(())
    }

    /// Replace `owner`'s watch lists.
    pub(crate) fn watch(&mut self, native: Option<&Native>, owner: &str, globals: &[String], mixmap: &[(String, u32, u32, u32)]) -> Result<(), String> {
        if globals.len() > MAX_WATCH || mixmap.len() > MAX_WATCH {
            return Err(format!("audio watch: {MAX_WATCH} globals and {MAX_WATCH} MixMap outputs maximum"));
        }
        let mut w = Watch::default();
        if !globals.is_empty() {
            let native = native.ok_or("native audio is not running")?;
            let rt = native.shared.lock().map_err(|_| "audio lock poisoned")?;
            for g in globals {
                let id = rt.eval.global_id(g).ok_or_else(|| format!("audio watch: no global {g}"))?;
                w.globals.push((g.clone(), id));
            }
        }
        for (slot, object, instance, output) in mixmap {
            let s = slot_number(slot).ok_or_else(|| format!("audio watch: no MixMap slot {slot}"))?;
            if *object > 127 || *instance > 31 || *output > 31 {
                return Err("audio watch: object 0..127, instance 0..31, output 0..31".into());
            }
            w.mixmap.push(MixMapWatch { slot: slot.clone(), object: *object, instance: *instance, output: *output, key: skate_audio::mixmap::keys::obj(s, *object, *instance) });
        }
        if w.globals.is_empty() && w.mixmap.is_empty() {
            self.watches.remove(owner);
            self.readback.remove(owner);
        } else {
            self.watches.insert(owner.to_owned(), w);
        }
        Ok(())
    }

    /// Subscribe `owner` to audio events with these tags (empty = all rows); `None` stops.
    pub(crate) fn subscribe(&mut self, owner: &str, tags: Option<Vec<String>>, native: Option<&Native>) -> Result<(), String> {
        match tags {
            None => {
                self.events.subscribers.remove(owner);
            }
            Some(tags) => {
                if let Some(bad) = tags.iter().find(|t| !TAGS.contains(&t.as_str())) {
                    return Err(format!("audio subscribe: unknown tag {bad} ({})", TAGS.join(", ")));
                }
                // Early copy only: `events_frame` refreshes the ids every frame (native may start later).
                if let Some(p) = native.and_then(|n| n.player.as_ref()) {
                    self.events.tags = Tags::from_tuning(&p.contact_tuning);
                }
                self.events.subscribers.insert(owner.to_owned(), tags);
            }
        }
        Ok(())
    }

    /// A tuning write changed the Contacts tuning: the pop / landing tags follow.
    pub(crate) fn retune_tags(&mut self, native: Option<&Native>) {
        if let Some(p) = native.and_then(|n| n.player.as_ref()).filter(|_| self.events.on()) {
            self.events.tags = Tags::from_tuning(&p.contact_tuning);
        }
    }

    /// Everything `owner` holds: live posts released, globals restored, watches and
    /// subscriptions dropped (a mod stopped, failed or was reloaded).
    pub(crate) fn clear_owner(&mut self, native: Option<&Native>, generation: u64, owner: &str) {
        self.queue.retain(|op| match op {
            Op::Post { owner: o, .. } | Op::Redeliver { owner: o, .. } | Op::Release { owner: o, .. } | Op::Global { owner: o, .. } => o != owner,
        });
        self.posts.remove(owner);
        let keys: Vec<_> = self.handles.keys().filter(|(o, _)| o == owner).cloned().collect();
        let mut rt = native.and_then(|n| n.shared.lock().ok().map(|rt| (n.map_epoch, rt)));
        for k in keys {
            if let (Some(h), Some((epoch, rt))) = (self.handles.remove(&k), rt.as_mut()) {
                if let Some(node) = h.node.filter(|_| h.epoch == *epoch && h.generation == generation) {
                    rt.release(node);
                }
            }
        }
        let names: Vec<_> = self.globals.iter().filter(|(_, g)| g.owner == owner).map(|(n, _)| n.clone()).collect();
        for n in names {
            if let Some(g) = self.globals.remove(&n) {
                if let Some((_, rt)) = rt.as_mut() {
                    rt.eval.set_global(g.id, g.original);
                }
            }
        }
        drop(rt);
        self.watches.remove(owner);
        self.readback.remove(owner);
        self.events.subscribers.remove(owner);
    }

    /// A map change: every live post is released and every global restored (the mods are told
    /// `world_changed` and post again); watches and subscriptions stay.
    pub(crate) fn clear_runtime(&mut self, native: Option<&Native>, generation: u64) {
        let owners: std::collections::BTreeSet<String> = self.handles.keys().map(|(o, _)| o.clone()).chain(self.globals.values().map(|g| g.owner.clone())).collect();
        for o in owners {
            let (w, s) = (self.watches.remove(&o), self.events.subscribers.remove(&o));
            self.clear_owner(native, generation, &o);
            if let Some(w) = w {
                self.watches.insert(o.clone(), w);
            }
            if let Some(s) = s {
                self.events.subscribers.insert(o, s);
            }
        }
    }

    /// After a restart (new runtime): the global overrides again, over the new runtime's values.
    pub(crate) fn reapply_globals(&mut self, native: &Native) {
        if self.globals.is_empty() {
            return;
        }
        let Ok(mut rt) = native.shared.lock() else { return };
        for (name, g) in &mut self.globals {
            match rt.eval.global_id(name) {
                Some(id) => {
                    g.id = id;
                    g.original = rt.eval.global(id).unwrap_or(0);
                    rt.eval.set_global(id, g.value);
                }
                None => warn!("Game audio: global {name} is not in the restarted runtime"),
            }
        }
    }

    /// The snapshot section of one mod (None when it holds nothing).
    pub(crate) fn snapshot(&self, native: Option<&Native>, generation: u64, owner: &str) -> Value {
        let epoch = native.map(|n| n.map_epoch);
        let handles: serde_json::Map<String, Value> = self.handles.iter().filter(|((o, _), _)| o == owner).map(|((_, k), h)| {
            let live = h.node.is_some() && Some(h.epoch) == epoch && h.generation == generation;
            (k.clone(), json!({"live": live, "class": h.class}))
        }).collect();
        let globals: serde_json::Map<String, Value> = self.globals.iter().filter(|(_, g)| g.owner == owner).map(|(n, g)| (n.clone(), json!(g.value))).collect();
        let watch = self.readback.get(owner);
        let events = self.events.rows_for(owner);
        if handles.is_empty() && globals.is_empty() && watch.is_none() && events.is_none() {
            return Value::Null;
        }
        json!({"handles": handles, "set_globals": globals, "watch": watch, "events": events})
    }
}

/// The start of the audio pass (before `native::mixmap_frame`): the queued posts / globals, in
/// command order (nothing to do, nothing done).
pub(super) fn drain(native: Option<Res<Native>>, content: Res<super::AudioContent>, mut api: ResMut<AudioApi>) {
    if api.queue.is_empty() && api.posts.is_empty() {
        return;
    }
    let api = &mut *api;
    api.posts.clear();
    match native {
        Some(native) => api.apply_queue(&native, content.runtime_generation),
        None => api.queue.clear(),
    }
}

impl AudioApi {
    /// Apply the queued operations to the runtime (the drain point).
    pub(crate) fn apply_queue(&mut self, native: &Native, generation: u64) {
        if self.queue.is_empty() {
            return;
        }
        let epoch = native.map_epoch;
        let live = |h: &Handle| h.node.filter(|_| h.epoch == epoch && h.generation == generation);
        let Ok(mut rt) = native.shared.lock() else { return };
        for op in std::mem::take(&mut self.queue) {
            match op {
                Op::Post { owner, key, class, name, words } => {
                    let h = self.handles.entry((owner, key)).or_insert(Handle { node: None, epoch, generation, class: name.clone() });
                    if let Some(old) = live(h) {
                        rt.release(old);
                    }
                    *h = Handle { node: Some(rt.post(class, &words)), epoch, generation, class: name };
                }
                Op::Redeliver { owner, key, words } => {
                    if let Some(node) = self.handles.get(&(owner, key)).and_then(live) {
                        rt.redeliver(node, &words);
                    }
                }
                Op::Release { owner, key } => {
                    if let Some(node) = self.handles.remove(&(owner, key)).as_ref().and_then(live) {
                        rt.release(node);
                    }
                }
                Op::Global { owner, id, name, value } => match value {
                    Some(v) => {
                        let original = self.globals.get(&name).map_or_else(|| rt.eval.global(id).unwrap_or(0), |g| g.original);
                        self.globals.insert(name, GlobalOverride { owner, id, original, value: v });
                        rt.eval.set_global(id, v);
                    }
                    None => {
                        if let Some(g) = self.globals.remove(&name) {
                            rt.eval.set_global(g.id, g.original);
                        }
                    }
                },
            }
        }
    }
}

/// The event frame boundary, first in the audio pass: the hosts' rows of the last frame join the
/// rows the systems pushed, and become what mods read (`last`). The sites record only while some
/// mod subscribes: their buffers are created / dropped here (nothing is recorded otherwise).
pub(super) fn events_frame(
    mut native: Option<ResMut<Native>>,
    mut api: ResMut<AudioApi>,
    mut world: Option<ResMut<super::world_sources::WorldHost>>,
    mut npc: Option<ResMut<super::npc_skaters::NpcHost>>,
    mut speech: Option<ResMut<super::world_speech::WorldSpeech>>,
) {
    let api = &mut *api;
    let on = api.events.on();
    if !on && api.events.last.is_empty() && api.events.current.is_empty() && !api.events.truncated {
        // Nobody subscribes and nothing is left: drop the sites' buffers once, then nothing.
        if let Some(p) = native.as_deref_mut().and_then(|n| n.player.as_mut()).filter(|p| p.events.is_some()) {
            p.events = None;
        }
        if let Some(w) = world.as_deref_mut().filter(|w| w.events.is_some()) {
            w.events = None;
        }
        if let Some(n) = npc.as_deref_mut().filter(|n| n.events.is_some()) {
            n.events = None;
        }
        if let Some(s) = speech.as_deref_mut().filter(|s| s.events.is_some()) {
            s.events = None;
        }
        return;
    }
    // The pop / land ids of the player whose rows this frame collects (a copy, no allocation):
    // right however the subscription and the native start / restart / tuning change are ordered.
    api.events.tags = native.as_deref().and_then(|n| n.player.as_ref()).map_or_else(Tags::default, |p| Tags::from_tuning(&p.contact_tuning));
    if let Some(p) = native.as_deref_mut().and_then(|n| n.player.as_mut()) {
        collect(&mut p.events, on, &mut api.events);
    }
    if let Some(w) = world.as_deref_mut() {
        collect(&mut w.events, on, &mut api.events);
    }
    if let Some(n) = npc.as_deref_mut() {
        collect(&mut n.events, on, &mut api.events);
    }
    if let Some(s) = speech.as_deref_mut() {
        collect(&mut s.events, on, &mut api.events);
    }
    api.events.swap();
    api.events.truncated = false;
    if !on {
        api.events.last.clear();
    }
}

/// A Splice host that records each start (`EventKind::Splice`) while the site records, and applies
/// the mods' rules (`mod_rules`: a muted start does not happen); every other call goes to the real
/// host unchanged.
pub(crate) struct Observed<'a, 'b> {
    inner: &'a mut dyn skate_audio::player::contacts::SpliceHost,
    rows: &'b mut EventBuf,
    source: Source,
    owner: u64,
    slot: &'static str,
    rules: Option<&'b super::mod_rules::RuleSet>,
}

impl<'a, 'b> Observed<'a, 'b> {
    pub(crate) fn new(inner: &'a mut dyn skate_audio::player::contacts::SpliceHost, rows: &'b mut EventBuf, source: Source, owner: u64) -> Self {
        Self { inner, rows, source, owner, slot: "", rules: None }
    }

    /// The site's rules (None: no rules, nothing is checked).
    pub(crate) fn rules(mut self, rules: Option<&'b super::mod_rules::RuleSet>) -> Self {
        self.rules = rules;
        self
    }

    /// The object's name for the recorded rows (`body_fall`, `ring`; "" by default).
    pub(crate) fn slot(mut self, slot: &'static str) -> Self {
        self.slot = slot;
        self
    }
}

impl skate_audio::player::contacts::SpliceHost for Observed<'_, '_> {
    fn set_route(&mut self, route: skate_audio::bus::Route) {
        self.inner.set_route(route);
    }
    fn set_submix(&mut self, submix: Option<skate_audio::player::footsteps::Submix>) {
        self.inner.set_submix(submix);
    }
    fn start(&mut self, bank: &str, id: u32, block: [f32; 6]) -> Option<skate_audio::splice::SoundId> {
        if let Some(rules) = self.rules {
            let row = EventRow { kind: EventKind::Splice, source: self.source, class: intern(bank), slot: self.slot, id: id as i32, owner: self.owner, hit: None };
            if rules.mutes(&row) {
                // The request is still reported to subscribers; the sound does not start.
                record(self.rows, row);
                return None;
            }
        }
        let sound = self.inner.start(bank, id, block);
        if sound.is_some() && self.rows.is_some() {
            record(self.rows, EventRow { kind: EventKind::Splice, source: self.source, class: intern(bank), slot: self.slot, id: id as i32, owner: self.owner, hit: None });
        }
        sound
    }
    fn update(&mut self, sound: skate_audio::splice::SoundId, block: [f32; 6]) {
        self.inner.update(sound, block);
    }
    fn alive(&self, sound: skate_audio::splice::SoundId) -> bool {
        self.inner.alive(sound)
    }
    fn release(&mut self, sound: skate_audio::splice::SoundId) {
        self.inner.release(sound);
    }
}

/// A bank / class name as `&'static str` for event rows (a few dozen distinct names; each is kept
/// once, the first time it is recorded).
pub(crate) fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut names) = NAMES.lock() else { return "" };
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    if names.len() >= 4096 {
        return "";
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

/// A player component slot's name and index for event rows.
pub(crate) fn player_slot(slot: &skate_audio::player::components::Slot) -> (&'static str, i32) {
    use skate_audio::player::components::Slot;
    match *slot {
        Slot::Grind(n) => ("grind", i32::from(n)),
        Slot::Rattle => ("rattle", 0),
        Slot::Wind => ("wind", 0),
        Slot::FootDrag => ("foot_drag", 0),
        Slot::Skid => ("skid", 0),
        Slot::Squeaks => ("squeaks", 0),
        Slot::Seam(n) => ("seam", i32::from(n)),
        Slot::RollingSurface(n) => ("rolling_surface", i32::from(n)),
        Slot::RollingLayer(n) => ("rolling_layer", i32::from(n)),
        Slot::RollingRattle => ("rolling_rattle", 0),
        Slot::BoardSlide => ("board_slide", 0),
        Slot::Flips => ("flips", 0),
        Slot::Cloth(n) => ("cloth", i32::from(n)),
        Slot::Treatment => ("treatment", 0),
        Slot::HomSloMo => ("hom_slo_mo", 0),
        Slot::Footstep(n) => ("footstep", i32::from(n)),
        Slot::ClothFalls => ("cloth_falls", 0),
        Slot::BodySlide => ("body_slide", 0),
        #[allow(unreachable_patterns)]
        _ => ("other", 0),
    }
}

/// A world slot's name and index for event rows.
pub(crate) fn world_slot(slot: &skate_audio::world::WorldSlot) -> (&'static str, i32) {
    use skate_audio::world::WorldSlot;
    match *slot {
        WorldSlot::Engine => ("engine", 0),
        WorldSlot::Horn => ("horn", 0),
        WorldSlot::Alarm => ("alarm", 0),
        WorldSlot::Skid => ("skid", 0),
        WorldSlot::PedFootstep(n) => ("ped_footstep", i32::from(n)),
        WorldSlot::PedTazer => ("ped_tazer", 0),
    }
}

fn collect(buf: &mut EventBuf, on: bool, events: &mut Events) {
    match (on, buf.as_mut()) {
        (true, Some(rows)) => {
            for r in rows.drain(..) {
                events.push(r);
            }
        }
        (true, None) => *buf = Some(Vec::with_capacity(64)),
        (false, Some(_)) => *buf = None,
        (false, None) => {}
    }
}

/// After the pass's ticks (`native::mixmap_tick`): the watched MixMap outputs and globals.
pub(super) fn readback(native: Option<Res<Native>>, mut api: ResMut<AudioApi>) {
    if api.watches.is_empty() {
        return;
    }
    let Some(native) = native else { return };
    let api = &mut *api;
    let rt = native.shared.lock().ok();
    for (owner, w) in &api.watches {
        let globals: serde_json::Map<String, Value> = w.globals.iter().map(|(n, id)| (n.clone(), json!(rt.as_ref().and_then(|r| r.eval.global(*id))))).collect();
        let mixmap: Vec<Value> = w.mixmap.iter().map(|m| match &native.mixmap {
            Some(mm) => {
                let id = m.output as usize;
                json!({"slot": m.slot, "object": m.object, "instance": m.instance, "output": m.output,
                    "level": mm.level(m.key, id), "raw": mm.raw(m.key, id), "pitch": mm.pitch_4096(m.key, id), "half": mm.half(m.key, id)})
            }
            None => json!({"slot": m.slot, "object": m.object, "instance": m.instance, "output": m.output}),
        }).collect();
        api.readback.insert(owner.clone(), json!({"globals": globals, "mixmap": mixmap}));
    }
}

/// `sdk.audio.info()` / the `audio_info` snapshot section.
pub(crate) fn info(native: Option<&Native>, content: Option<&super::AudioContent>, map: Option<&super::map_audio::MapAudio>) -> Value {
    json!({
        "native": native.is_some(),
        "map_epoch": native.map(|n| n.map_epoch),
        "generation": content.map(|c| c.generation),
        "restarts": content.map(|c| c.restarts),
        // Doc 16 L1: content changes swapped in place, and the last change ("swap" / "restart: …").
        "swaps": content.map(|c| c.swaps),
        "last_change": content.and_then(|c| c.last_change.clone()),
        "overlays": content.map(|c| c.overlays.keys().cloned().collect::<Vec<_>>()),
        "conflicts": content.map(|c| c.report.conflicts.len()),
        "map": map.map(|m| json!({"stem": m.stem, "district": m.district, "ems": m.ems, "sources": m.sources})),
        "limits": {"handles_per_mod": MAX_HANDLES_PER_MOD, "handles": MAX_HANDLES, "posts_per_frame": MAX_POSTS_PER_FRAME,
                   "watch": MAX_WATCH, "events_per_frame": MAX_EVENTS},
        "tags": TAGS,
    })
}

/// `sdk.engine.inspect('audio_catalog')`: the runtime's classes, functions and globals (with
/// values), loaded banks, the map's audio, location sets and zones by name, the overlays.
pub(crate) fn catalog(native: Option<&Native>, library: Option<&super::Library>, map: Option<&super::map_audio::MapAudio>, content: Option<&super::AudioContent>) -> Value {
    let (classes, functions, globals) = match native.and_then(|n| n.shared.lock().ok()) {
        Some(rt) => {
            let r = &rt.eval.registry;
            (
                r.classes.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                r.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
                r.globals.iter().enumerate().map(|(i, g)| (g.name.clone(), json!(rt.eval.global(i)))).collect::<serde_json::Map<_, _>>(),
            )
        }
        None => Default::default(),
    };
    json!({
        "classes": classes,
        "functions": functions,
        "globals": globals,
        "banks": native.map(|n| n.loaded_banks()),
        "map": map.map(|m| json!({"stem": m.stem, "district": m.district, "ems": m.ems, "crossfade_bank": m.crossfade_bank, "extra_records": m.emitters.len(), "sources": m.sources})),
        "location_sets": library.map(|l| l.random_set_names()),
        "zones": library.map(|l| l.zone_names()),
        "overlays": content.map(|c| c.overlays.keys().cloned().collect::<Vec<_>>()),
        "owners": content.map(|c| c.report.owners.clone()),
        "tags": TAGS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use skate_audio::eval::synthetic::{self, Ex};
    use skate_audio::runtime::Runtime;
    use std::sync::Arc;

    /// A runtime with the synthetic project (class `c_test`, global `g_snd` = 77) and one bank.
    fn native() -> Native {
        let mut rt = Runtime::new();
        rt.install_project(&synthetic::project());
        let bank = synthetic::bank(&[synthetic::player_module(8)], &[Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }], &[(4800, false)]);
        rt.load_bank(bank, vec![Some(Arc::new(skate_audio::mixer::Pcm { rate: 48000, channels: vec![vec![0.25; 4800]] }))]);
        Native::for_test(rt)
    }

    fn frame(api: &mut AudioApi, native: &Native, generation: u64) {
        api.posts.clear();
        api.apply_queue(native, generation);
    }

    fn node(api: &AudioApi, owner: &str, key: &str) -> Option<NodeId> {
        api.handles.get(&(owner.to_owned(), key.to_owned())).and_then(|h| h.node)
    }

    /// Posts by class name with key-scoped handles: applied at the drain, a key's post replaces
    /// its last one, unknown classes are errors, the limits hold.
    #[test]
    fn posts_are_key_scoped_and_bounded() {
        let n = native();
        let mut api = AudioApi::default();
        assert!(api.post(None, "dev.a", "k", "c_test", &[]).is_err(), "no runtime");
        assert!(api.post(Some(&n), "dev.a", "k", "c_nope", &[]).unwrap_err().contains("no class"));
        api.post(Some(&n), "dev.a", "k", "c_test", &[4096, 0, 0]).unwrap();
        assert!(node(&api, "dev.a", "k").is_none(), "queued until the drain");
        frame(&mut api, &n, 0);
        let first = node(&api, "dev.a", "k").expect("posted");
        let class = n.shared.lock().unwrap().eval.class_id("c_test");
        assert_eq!(n.shared.lock().unwrap().eval.node_class(first), class);
        assert_eq!(api.snapshot(Some(&n), 0, "dev.a")["handles"]["k"]["live"], true);
        api.post(Some(&n), "dev.a", "k", "c_test", &[]).unwrap();
        frame(&mut api, &n, 0);
        let second = node(&api, "dev.a", "k").unwrap();
        assert_ne!(first, second);
        assert_ne!(n.shared.lock().unwrap().eval.node_refcount(first), Some(1), "the key's last post was released");
        // Release, then post again in the same frame: the new post stands.
        api.release("dev.a", "k");
        api.post(Some(&n), "dev.a", "k", "c_test", &[]).unwrap();
        frame(&mut api, &n, 0);
        assert!(node(&api, "dev.a", "k").is_some_and(|x| x != second));
        assert!(api.redeliver("dev.a", "nope", &[]).is_err());
        // 16 posts per frame, 32 handles per mod.
        for i in 0..16 {
            api.post(Some(&n), "dev.a", &format!("p{i}"), "c_test", &[]).unwrap();
        }
        assert!(api.post(Some(&n), "dev.a", "p16", "c_test", &[]).unwrap_err().contains("per frame"));
        frame(&mut api, &n, 0);
        for i in 16..31 {
            api.post(Some(&n), "dev.a", &format!("p{i}"), "c_test", &[]).unwrap();
        }
        frame(&mut api, &n, 0);
        assert_eq!(api.owned("dev.a"), 32);
        assert!(api.post(Some(&n), "dev.a", "p99", "c_test", &[]).unwrap_err().contains("handles per mod"));
        assert!(api.post(Some(&n), "dev.b", "p0", "c_test", &[]).is_ok(), "another mod has its own");
    }

    /// A map change or a restart makes handles dead: they read `live = false` and their ids are
    /// never released into the runtime again; the mod posts afresh.
    #[test]
    fn dead_handles_are_forgotten_not_released() {
        let mut n = native();
        let mut api = AudioApi::default();
        api.post(Some(&n), "dev.a", "k", "c_test", &[]).unwrap();
        frame(&mut api, &n, 0);
        let old = node(&api, "dev.a", "k").unwrap();
        let refs = n.shared.lock().unwrap().eval.node_refcount(old);
        n.map_epoch += 1;
        assert_eq!(api.snapshot(Some(&n), 0, "dev.a")["handles"]["k"]["live"], false);
        api.post(Some(&n), "dev.a", "k", "c_test", &[]).unwrap();
        api.redeliver("dev.a", "k", &[1]).unwrap();
        frame(&mut api, &n, 0);
        assert_eq!(n.shared.lock().unwrap().eval.node_refcount(old), refs, "the dead id was not released");
        assert_eq!(api.snapshot(Some(&n), 0, "dev.a")["handles"]["k"]["live"], true);
        assert_eq!(api.snapshot(Some(&n), 1, "dev.a")["handles"]["k"]["live"], false, "a new content generation");
    }

    /// Globals: the first owner wins; `nil`, the owner stopping and a map change restore the
    /// value seen before the first write; a restart applies the override to the new runtime.
    #[test]
    fn globals_restore_and_survive_a_restart() {
        let n = native();
        let mut api = AudioApi::default();
        let g = |n: &Native| {
            let rt = n.shared.lock().unwrap();
            rt.eval.global(rt.eval.global_id("g_snd").unwrap()).unwrap()
        };
        assert_eq!(g(&n), 77);
        assert!(api.set_global(Some(&n), "dev.a", "g_nope", Some(1)).is_err());
        api.set_global(Some(&n), "dev.a", "g_snd", Some(5)).unwrap();
        frame(&mut api, &n, 0);
        assert_eq!(g(&n), 5);
        assert!(api.set_global(Some(&n), "dev.b", "g_snd", Some(6)).unwrap_err().contains("owned by another mod"));
        api.set_global(Some(&n), "dev.a", "g_snd", Some(8)).unwrap();
        frame(&mut api, &n, 0);
        api.set_global(Some(&n), "dev.a", "g_snd", None).unwrap();
        frame(&mut api, &n, 0);
        assert_eq!(g(&n), 77, "nil restores the value before the first write");
        api.set_global(Some(&n), "dev.a", "g_snd", Some(9)).unwrap();
        frame(&mut api, &n, 0);
        let fresh = native();
        api.reapply_globals(&fresh);
        assert_eq!(g(&fresh), 9, "the override holds in a restarted runtime");
        api.clear_owner(Some(&n), 0, "dev.a");
        assert_eq!(g(&n), 77, "restored when the mod stops");
        api.set_global(Some(&n), "dev.b", "g_snd", Some(3)).unwrap();
        frame(&mut api, &n, 0);
        api.clear_runtime(Some(&n), 0);
        assert_eq!(g(&n), 77, "restored at a map change");
    }

    /// Watches: unknown names are errors; the readback copies the values after the pass; a map
    /// change keeps watches, a stopping mod drops them with its posts.
    #[test]
    fn watches_read_back_and_clean_up() {
        let n = native();
        let mut api = AudioApi::default();
        assert!(api.watch(Some(&n), "dev.a", &["g_nope".into()], &[]).is_err());
        assert!(api.watch(Some(&n), "dev.a", &[], &[("music".into(), 0, 0, 0)]).is_err());
        api.watch(Some(&n), "dev.a", &["g_snd".into()], &[("emitter".into(), 0, 2, 4)]).unwrap();
        let mut world = World::new();
        world.insert_resource(api);
        world.insert_resource(n);
        world.run_system_once(readback).unwrap();
        let api = world.resource::<AudioApi>();
        let n = world.resource::<Native>();
        let s = api.snapshot(Some(n), 0, "dev.a");
        assert_eq!(s["watch"]["globals"]["g_snd"], 77);
        assert_eq!(s["watch"]["mixmap"][0]["instance"], 2);
        let mut api = world.remove_resource::<AudioApi>().unwrap();
        let n = world.remove_resource::<Native>().unwrap();
        api.post(Some(&n), "dev.a", "k", "c_test", &[]).unwrap();
        frame(&mut api, &n, 0);
        api.clear_runtime(Some(&n), 0);
        assert!(api.watches.contains_key("dev.a") && api.handles.is_empty());
        api.clear_owner(Some(&n), 0, "dev.a");
        assert!(api.watches.is_empty() && api.snapshot(Some(&n), 0, "dev.a").is_null());
    }

    /// The event frame: nothing is recorded without a subscriber (the sites' buffers stay None);
    /// with one, the hosts' rows and the systems' rows of a frame are what the mod reads at the
    /// next frame, filtered by its tags, once per serial; unsubscribing drops the buffers.
    #[test]
    fn events_record_only_for_subscribers() {
        let mut world = World::new();
        let mut player = super::super::player_audio::PlayerAudio::new(Default::default(), true);
        player.events = None;
        let mut n = native();
        n.player = Some(player);
        world.insert_resource(n);
        world.init_resource::<AudioApi>();
        world.init_resource::<super::super::world_sources::WorldHost>();
        world.init_resource::<super::super::npc_skaters::NpcHost>();
        world.init_resource::<super::super::world_speech::WorldSpeech>();
        let row = |kind, source, class, slot, id| EventRow { kind, source, class, slot, id, owner: 0, hit: None };
        world.run_system_once(events_frame).unwrap();
        assert!(world.resource::<Native>().player.as_ref().unwrap().events.is_none(), "no subscriber: no buffer");
        world.resource_mut::<AudioApi>().events.push(row(EventKind::Zone, Source::Ambience, "", "", 0));
        assert!(world.resource::<AudioApi>().events.current.is_empty(), "no subscriber: nothing pushed");
        world.resource_mut::<AudioApi>().subscribe("dev.a", Some(vec!["grind_start".into(), "zone_change".into()]), None).unwrap();
        assert!(world.resource_mut::<AudioApi>().subscribe("dev.a", Some(vec!["nope".into()]), None).is_err());
        world.run_system_once(events_frame).unwrap();
        assert!(world.resource::<Native>().player.as_ref().unwrap().events.is_some(), "a subscriber: the sites record");
        // A frame: the player posts a grind and a wind layer, the ambience changes zone.
        record(&mut world.resource_mut::<Native>().player.as_mut().unwrap().events, row(EventKind::Post, Source::Player, "Class_grind", "grind", 0));
        record(&mut world.resource_mut::<Native>().player.as_mut().unwrap().events, row(EventKind::Post, Source::Player, "SenseOfSpeed", "wind", 0));
        world.resource_mut::<AudioApi>().events.push(row(EventKind::Zone, Source::Ambience, "", "", 0));
        world.run_system_once(events_frame).unwrap();
        let api = world.resource::<AudioApi>();
        let s = api.snapshot(None, 0, "dev.a");
        let rows = s["events"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!((rows[0]["tag"].as_str(), rows[1]["tag"].as_str()), (Some("zone_change"), Some("grind_start")));
        let serial = s["events"]["serial"].as_u64().unwrap();
        world.run_system_once(events_frame).unwrap();
        let s = world.resource::<AudioApi>().snapshot(None, 0, "dev.a");
        assert!(s["events"]["rows"].as_array().unwrap().is_empty() && s["events"]["serial"].as_u64() == Some(serial + 1), "the next frame has its own rows");
        world.resource_mut::<AudioApi>().subscribe("dev.a", None, None).unwrap();
        world.run_system_once(events_frame).unwrap();
        world.run_system_once(events_frame).unwrap();
        assert!(world.resource::<Native>().player.as_ref().unwrap().events.is_none(), "unsubscribed: the buffers go");
        assert!(world.resource::<AudioApi>().snapshot(None, 0, "dev.a").is_null());
    }

    /// `body_impact`: the body poster's hits of the local player and of an NPC skater reach a
    /// subscriber with region, impact, tiers, materials and position (the skater point, retail's
    /// `+48`); a stopped mod's subscription and the sites' buffers go.
    #[test]
    fn body_impacts_reach_a_subscriber_per_skater() {
        use skate_audio::player::{collision::Message, contacts::BodyHit};
        let mut world = World::new();
        let mut n = native();
        n.player = Some(super::super::player_audio::PlayerAudio::new(Default::default(), true));
        world.insert_resource(n);
        world.init_resource::<AudioApi>();
        world.init_resource::<super::super::world_sources::WorldHost>();
        world.init_resource::<super::super::npc_skaters::NpcHost>();
        world.init_resource::<super::super::world_speech::WorldSpeech>();
        world.resource_mut::<AudioApi>().subscribe("dev.a", Some(vec!["body_impact".into()]), None).unwrap();
        world.run_system_once(events_frame).unwrap();
        let hit = |region, position| BodyHit { region, impact: 3.03, message: Message { material: [99, 2], tier: [2, 2], position, level: [20000, 30000], local: true } };
        let player = [hit(4, [323.5, 88.3, -644.6])];
        super::super::player_audio::body_hits(&player, Source::Player, 0, Some([326.5, 89.8, -642.6]), &mut world.resource_mut::<Native>().player.as_mut().unwrap().events);
        super::super::player_audio::body_hits(&[hit(0, [5.0, 1.0, 2.0])], Source::Npc, 42, None, &mut world.resource_mut::<super::super::npc_skaters::NpcHost>().events);
        world.run_system_once(events_frame).unwrap();
        let s = world.resource::<AudioApi>().snapshot(None, 0, "dev.a");
        let rows = s["events"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        let r = &rows[0];
        assert_eq!((r["tag"].as_str(), r["kind"].as_str(), r["source"].as_str(), r["owner"].as_str()), (Some("body_impact"), Some("body_impact"), Some("player"), Some("0")));
        assert_eq!((r["region"].as_u64(), r["tier"].clone(), r["material"].clone()), (Some(4), json!([2, 2]), json!([99, 2])));
        assert_eq!(r["position"], json!([323.5f32, 88.3f32, -644.6f32]));
        assert!((r["impact"].as_f64().unwrap() - 3.03).abs() < 1e-5);
        assert_eq!((rows[1]["source"].as_str(), rows[1]["owner"].as_str(), rows[1]["region"].as_u64()), (Some("npc"), Some("42"), Some(0)));
        // Mod disabled: subscription dropped, then the sites' buffers.
        world.resource_mut::<AudioApi>().clear_owner(None, 0, "dev.a");
        world.run_system_once(events_frame).unwrap();
        world.run_system_once(events_frame).unwrap();
        assert!(world.resource::<Native>().player.as_ref().unwrap().events.is_none());
        assert!(world.resource::<super::super::npc_skaters::NpcHost>().events.is_none());
        assert!(world.resource::<AudioApi>().snapshot(None, 0, "dev.a").is_null());
    }

    /// Found by the in-game autotest: a mod that subscribes in `on_load`, before native audio
    /// starts, kept unset pop / land ids for the session (pops untagged, a Splice row with id 0
    /// tagged `land`). The ids now follow the running player, whenever it starts or changes.
    #[test]
    fn event_tags_follow_the_player_whatever_the_subscription_order() {
        use skate_audio::player::contacts::BANK;
        let splice = |id| EventRow { kind: EventKind::Splice, source: Source::Player, class: BANK, slot: "", id, owner: 0, hit: None };
        let tags_of = |world: &World, owner: &str| -> Vec<(i32, Option<String>)> {
            let s = world.resource::<AudioApi>().snapshot(None, 0, owner);
            s["events"]["rows"].as_array().unwrap().iter().map(|r| (r["id"].as_i64().unwrap() as i32, r["tag"].as_str().map(str::to_owned))).collect()
        };
        let frame_with = |world: &mut World, ids: &[i32]| {
            for &id in ids {
                record(&mut world.resource_mut::<Native>().player.as_mut().unwrap().events, splice(id));
            }
            world.run_system_once(events_frame).unwrap();
        };
        let tag = |s: &str| Some(s.to_owned());
        assert_eq!(Tags::default().tag(&splice(0)), None, "unset ids tag nothing");

        // Subscribe at load: no native audio yet.
        let mut world = World::new();
        world.init_resource::<AudioApi>();
        world.resource_mut::<AudioApi>().subscribe("dev.a", Some(vec!["pop".into(), "land".into()]), None).unwrap();
        world.resource_mut::<AudioApi>().subscribe("dev.all", Some(vec![]), None).unwrap();
        world.run_system_once(events_frame).unwrap();
        // Native audio starts later (the player's Contacts tuning: pops 1097.. / 1103.., landing 1095).
        let mut player = super::super::player_audio::PlayerAudio::new(Default::default(), true);
        player.events = None;
        let (pop, hollow, land) = (player.contact_tuning.pop_ids[0] as i32, player.contact_tuning.pop_ids_hollow[2] as i32, player.contact_tuning.landing_id as i32);
        assert!(pop != 0 && hollow != 0 && land != 0);
        let mut n = native();
        n.player = Some(player);
        world.insert_resource(n);
        world.run_system_once(events_frame).unwrap();
        frame_with(&mut world, &[pop, land, 0, hollow]);
        assert_eq!(tags_of(&world, "dev.a"), vec![(pop, tag("pop")), (land, tag("land")), (hollow, tag("pop"))]);
        assert_eq!(tags_of(&world, "dev.all"), vec![(pop, tag("pop")), (land, tag("land")), (0, None), (hollow, tag("pop"))], "id 0 is never `land`");

        // Re-subscribing keeps the ids.
        world.resource_mut::<AudioApi>().subscribe("dev.a", None, None).unwrap();
        world.run_system_once(events_frame).unwrap();
        world.resource_scope(|world, mut api: Mut<AudioApi>| api.subscribe("dev.a", Some(vec!["land".into()]), world.get_resource::<Native>())).unwrap();
        frame_with(&mut world, &[pop, land]);
        assert_eq!(tags_of(&world, "dev.a"), vec![(land, tag("land"))]);

        // A tuning change (or a restarted player with other ids) retags from the next frame.
        world.resource_mut::<Native>().player.as_mut().unwrap().contact_tuning.landing_id = 2000;
        frame_with(&mut world, &[land, 2000]);
        assert_eq!(tags_of(&world, "dev.all"), vec![(land, None), (2000, tag("land"))]);
        let mut player = super::super::player_audio::PlayerAudio::new(Default::default(), true);
        player.contact_tuning.pop_ids[0] = 3000;
        world.resource_mut::<Native>().player = Some(player);
        world.run_system_once(events_frame).unwrap();
        frame_with(&mut world, &[3000, pop, land]);
        assert_eq!(tags_of(&world, "dev.all"), vec![(3000, tag("pop")), (pop, None), (land, tag("land"))]);

        // Native audio stops: back to unset ids.
        world.remove_resource::<Native>();
        world.run_system_once(events_frame).unwrap();
        assert_eq!(world.resource::<AudioApi>().events.tags, Tags::default());
    }

    /// A mod's post of a retail class plays (data-gated): `c_emitter` with a DownTown emitter's
    /// bank loaded sounds through the native mixer, as the emitter system's own post does;
    /// without the mod's post the output is silent.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_mod_post_of_a_retail_class_plays() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = super::super::Library::load(root) else { panic!("missing private data: no audio install") };
        let r = library.emitters("sfx_downtown").iter().find(|r| r.kind == 1 && r.bank.as_ref().is_some_and(|b| library.aems().banks.contains_key(b))).unwrap().clone();
        let render = |post: bool| {
            let mut n = Native::start(&library).unwrap();
            n.ensure_bank(&library, r.bank.as_deref().unwrap()).unwrap();
            let mut api = AudioApi::default();
            if post {
                let words = n.emitter_payload(None, 1.0, 0, r.patch);
                api.post(Some(&n), "dev.a", "fountain", "c_emitter", &words).unwrap();
                frame(&mut api, &n, 0);
            }
            let mut out = vec![0.0f32; 2 * skate_audio::BLOCK];
            let mut peak = 0.0f32;
            for _ in 0..400 {
                n.shared.lock().unwrap().fill_stereo(&mut out);
                peak = out.iter().fold(peak, |p, s| p.max(s.abs()));
            }
            peak
        };
        assert_eq!(render(false), 0.0, "nothing posts the emitter bank by itself");
        assert!(render(true) > 0.001, "the mod's post plays the bank's program");
    }
}
