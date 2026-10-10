//! Mod WAVs through the native mixer (doc 16 H; audio extension 3: `sdk.audio.play` is native by
//! default since 2026-10-04, `native = false` keeps the Bevy voice), and the rules' sounds (doc
//! 16 K: placed at the owner of the game's sound, followed every frame, [`Follow`]).
//!
//! - Each mod's clips become one bank per volume group in the runtime's mixer ([`MOD_BANK_BASE`] +
//!   2 × the mod's index + group): a sample header built from the WAV (rate, length, channels; a
//!   looping play gets its own slot that loops from frame 0).
//! - A voice is a direct voice (`Mixer::open_routed`) on SFX Master, driven every frame like a
//!   retail world emitter's `c_emitter` words (`Native::emitter_payload`, mixmap-spec §6.3): an
//!   Emitter instance of the private MixMap ([`ModMix`]) gives the dry level (out4), the
//!   environment send (out8: −26 dB rolling off with camera distance 4 → 70 m), the pan (out0, the
//!   camera azimuth) and the filters / pitch (out5 / out6); the sound's own reach is the `.ems`
//!   record test of a sphere (`radius`, inner `core`) with the retail falloff curve
//!   (`eVolumeFalloffType`, `emitters::shape_level`), as `level = volume × curve(d)`. A sound
//!   without a position (`spatial = false`) takes the non-positional outputs (out2 dry, out7
//!   send, out3 low-pass), centre-panned.
//! - The private MixMap is retail's MixMap file with the Global slot and [`MIX_INSTANCES`] Emitter
//!   instances; before each of its ticks it takes the retail MixMap's Global inputs (master,
//!   music, reverb, pause), so its Emitter outputs are what a retail Emitter instance gives for
//!   the same position (test `the_private_mixmap_matches_the_retail_emitter_outputs`). It ticks with
//!   the retail pass (the same console evaluations) and only while an instance is held. Retail's
//!   own MixMap and its 5 Emitter instances are untouched.
//! - Nothing here runs while no native mod voice and no private-MixMap user exists ([`frame`]
//!   returns at once), so without mods the audio is unchanged.
//! - A runtime restart (audio content change) forgets the voices' mixer ids (never released into
//!   the new runtime): looping voices open again from the start, finished one-shots go; the
//!   banks are registered in the new runtime again.
use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use skate_audio::bus::{Output, Route};
use skate_audio::dsp::{DEGREES_PER_UNIT, INV_32767};
use skate_audio::eval::VoiceHost;
use skate_audio::formats::SampleHeader;
use skate_audio::mixer::{GROUP_PLAYER, GROUP_WORLD, Pcm};
use skate_audio::mixmap::{MixMap, MixMapFile, keys};

use super::{Library, Native};

/// The first mixer bank id of the mod banks (Splice banks are `1 << 20`, the wheel streams
/// `1 << 21`, speech `1 << 22`).
pub(crate) const MOD_BANK_BASE: usize = 1 << 23;
/// Emitter instances of the private MixMap (the MixMap's group field has 5 bits: 32 at most).
pub(crate) const MIX_INSTANCES: usize = 32;
/// Native mod voices in all (each holds one private-MixMap instance; they share the mixer's voice
/// budget with the game's own voices).
pub(crate) const MAX_NATIVE_VOICES: usize = 24;

/// The private MixMap (see the module docs). Shared by the native mod voices and, with the
/// "extra" emitter slots setting (the default), the mod emitters.
#[derive(Resource, Default)]
pub(crate) struct ModMix {
    mix: Option<MixMap>,
    /// The Global slot's controller keys (copied from the retail MixMap before each tick).
    globals: Vec<u32>,
    /// The runtime it was built for (the runtime's address and the content generation).
    built: Option<(usize, u64)>,
    used: [bool; MIX_INSTANCES],
    /// Bumped by every build: an instance claimed under an older build is gone.
    pub(crate) build: u64,
    /// Evaluations done (an instance is read only after one that saw its position).
    pub(crate) ticks: u64,
}

/// The private MixMap's instances per slot: Global 1, Emitter [`MIX_INSTANCES`], none else.
fn mix_instances() -> [usize; 14] {
    let mut n = [0; 14];
    n[keys::slot::GLOBAL as usize] = 1;
    n[keys::slot::EMITTER as usize] = MIX_INSTANCES;
    n
}

/// The retail words of an Emitter instance (`Native::emitter_payload`'s positional formula):
/// `[32767, out4 × level, out8 × level, out0, out5 → pitch, out6, 0, 0, patch]`.
pub(crate) fn emitter_words(m: &MixMap, g: usize, level: f32, patch: i32) -> [i32; 9] {
    let level = level.clamp(0.0, 1.0);
    let key = keys::emitter(g as u32);
    let scaled = |id: usize| (m.level(key, id) as f32 * level) as i32;
    [32767, scaled(4), scaled(8), m.raw(key, 0), m.pitch_4096(key, 5), m.filter_hz(key, 6), 0, 0, patch.clamp(0, 500)]
}

/// The non-positional branch (mixmap-spec §6.3: out2 dry, out7 send, out1 pitch, out3 low-pass).
fn emitter_words_flat(m: &MixMap, g: usize, level: f32) -> [i32; 9] {
    let level = level.clamp(0.0, 1.0);
    let key = keys::emitter(g as u32);
    let scaled = |id: usize| (m.level(key, id) as f32 * level) as i32;
    [32767, scaled(2), scaled(7), 0, m.pitch_4096(key, 1), m.filter_hz(key, 3), 0, 0, 0]
}

impl ModMix {
    /// Build for this runtime when needed; false without a MixMap.
    pub(crate) fn ensure(&mut self, library: &Library, native: &Native, generation: u64) -> bool {
        let id = (Arc::as_ptr(&native.shared) as usize, generation);
        if self.built == Some(id) {
            return self.mix.is_some();
        }
        self.built = Some(id);
        self.build += 1;
        self.used = [false; MIX_INSTANCES];
        self.mix = None;
        self.globals.clear();
        let (Some(retail), Some(file)) = (&native.mixmap, &library.aems().mixmap) else { return false };
        let Ok(bytes) = library.read(file) else { return false };
        let Ok(parsed) = MixMapFile::parse(&bytes) else { return false };
        self.globals = retail.controller_keys().filter(|k| (k >> 16) & 0xFF == keys::slot::GLOBAL).collect();
        self.mix = Some(MixMap::new(&parsed, &mix_instances()));
        info!("Game audio: private MixMap for mod sounds ({MIX_INSTANCES} Emitter instances)");
        true
    }

    pub(crate) fn in_use(&self) -> bool {
        self.used.iter().any(|u| *u)
    }

    pub(crate) fn claim(&mut self) -> Option<usize> {
        self.mix.as_ref()?;
        let g = self.used.iter().position(|u| !u)?;
        self.used[g] = true;
        Some(g)
    }

    pub(crate) fn release(&mut self, g: usize) {
        if let Some(u) = self.used.get_mut(g) {
            *u = false;
        }
        if let Some(m) = &mut self.mix {
            m.set_input(keys::emitter_pos(g as u32), keys::pos::FLAGS, 0);
        }
    }

    /// The pass's evaluations: the retail Global inputs, then `calls` ticks (only while an
    /// instance is held).
    pub(crate) fn tick(&mut self, retail: &MixMap, calls: usize) {
        if calls == 0 || !self.in_use() {
            return;
        }
        let Some(m) = &mut self.mix else { return };
        for &k in &self.globals {
            for id in 0..16 {
                m.set_input(k, id, retail.input(k, id));
            }
        }
        for _ in 0..calls {
            m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
        }
        self.ticks += calls as u64;
    }

    /// An instance's 3-D input (`native::write_position`, as the retail emitter states').
    pub(crate) fn set_position(&mut self, g: usize, listener: &GlobalTransform, skater: Vec3, source: Vec3) {
        if let Some(m) = &mut self.mix {
            super::native::write_position(m, keys::emitter_pos(g as u32), listener, skater, source);
        }
    }

    pub(crate) fn words(&self, g: usize, level: f32, patch: i32) -> Option<[i32; 9]> {
        self.mix.as_ref().map(|m| emitter_words(m, g, level, patch))
    }

    fn words_flat(&self, g: usize, level: f32) -> Option<[i32; 9]> {
        self.mix.as_ref().map(|m| emitter_words_flat(m, g, level))
    }

    #[cfg(test)]
    pub(crate) fn mixmap(&mut self) -> Option<&mut MixMap> {
        self.mix.as_mut()
    }
}

/// A positional sound's reach: retail's `.ems` record test (`emitters::shape_level`: a sphere when
/// the three extents are equal, else an ellipsoid along `forward`, up and side; the inner `core`
/// at full level) and falloff curve (`eVolumeFalloffType`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reach {
    pub extent: Vec3,
    pub forward: Vec3,
    pub core: f32,
    pub curve: i32,
}

impl Reach {
    pub(crate) fn sphere(radius: f32, core: f32, curve: i32) -> Self {
        Self { extent: Vec3::splat(radius), forward: Vec3::X, core, curve }
    }

    /// A mod's `falloff` (`skate_mods::audio::NativeFalloff`).
    pub(crate) fn from_falloff(f: skate_mods::audio::NativeFalloff) -> Self {
        Self::sphere(f.radius, f.core, f.curve.retail_type())
    }

    /// The level factor at `ear` (0 outside).
    fn level(&self, at: Vec3, ear: Vec3) -> f32 {
        super::emitters::shape_level(at, self.extent, self.forward, self.core, self.curve, ear).unwrap_or(0.0)
    }
}

/// The owner of a game sound a rule's sound replaces or layers (`mod_rules`), located every frame
/// from what the game publishes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    /// The local skater (its centre of mass, as retail's player sounds: MixMap 3DObjPos 60010010).
    Player,
    /// A world owner by id: a car or a ped (`world_sources::WorldOwners`).
    World(u64),
    /// An NPC skater by id (its centre of mass, `npc_skaters::NpcSkaters`).
    Npc(u64),
    /// A fixed place and its reach (an emitter record: records do not move).
    Fixed(Vec3, Reach),
    /// A published emitter (`WorldEmitter` entity bits), its position and reach at the start: it
    /// can move, the sound follows it (its record's reach turned with it).
    Emitter(u64, Vec3, Reach),
}

/// A rule sound's placement relative to its owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Follow {
    pub anchor: Anchor,
    /// Added to the owner's position (world axes).
    pub offset: Vec3,
    /// The sound follows the owner (`at = 'owner'`); false: a fixed position, the owner only gives
    /// the default reach (`at = 'world'`).
    pub track: bool,
    /// `offset` is in the owner's axes (x right, y up, z its facing; `play.frame = 'owner'`).
    pub local: bool,
}

/// An offset in an owner's axes as a world offset: x = right, y = up, z = `facing` (horizontal;
/// world +Z when the owner has none).
pub(crate) fn owner_offset(offset: Vec3, facing: Vec3) -> Vec3 {
    let f = Vec3::new(facing.x, 0.0, facing.z).normalize_or(Vec3::Z);
    let right = f.cross(Vec3::Y);
    right * offset.x + Vec3::Y * offset.y + f * offset.z
}

/// A skater's facing from its wheels (front pair minus back pair; FL, FR, BL, BR), else its
/// centre-of-mass velocity.
fn board_facing(s: &skate_audio::player::AudioState) -> Vec3 {
    let w = s.wheel_position.map(Vec3::from_array);
    let along = (w[0] + w[1] - w[2] - w[3]) * 0.5;
    if along.length_squared() > 1e-6 { along } else { Vec3::from_array(s.com_velocity) }
}

/// The owner's position now and its kind's retail reach: the skater audio radius (30 m, the local
/// player too: its sounds sit in the same Player MixMap slot), the traffic list (40 m), the ped
/// list (50 m), all with the squared curve; an emitter record's own shape and curve.
#[cfg(test)]
pub(crate) fn locate(a: &Anchor, cues: &super::skate_events::Cues, owners: Option<&super::world_sources::WorldOwners>, npcs: Option<&super::npc_skaters::NpcSkaters>) -> Option<(Vec3, Reach)> {
    locate_facing(a, cues, owners, npcs, &|_| None).map(|(p, r, _)| (p, r))
}

/// [`locate`] with the owner's facing (`Follow::local`), and published emitters found through
/// `emitters` (entity bits → position, forward).
pub(crate) fn locate_facing(a: &Anchor, cues: &super::skate_events::Cues, owners: Option<&super::world_sources::WorldOwners>, npcs: Option<&super::npc_skaters::NpcSkaters>, emitters: &dyn Fn(u64) -> Option<(Vec3, Vec3)>) -> Option<(Vec3, Reach, Vec3)> {
    use skate_audio::world::skaters::AUDIO_RADIUS;
    match a {
        Anchor::Player => Some((Vec3::from_array(cues.riding.audio.com_position), Reach::sphere(AUDIO_RADIUS, 0.0, 0), board_facing(&cues.riding.audio))),
        Anchor::World(id) => {
            let o = owners?;
            let (p, r, f) = o.vehicles.get(id).map(|v| (v.position, super::world_sources::TRAFFIC_LIST_RADIUS, v.direction)).or_else(|| o.peds.get(id).map(|p| (p.position, super::world_sources::PED_LIST_RADIUS, p.velocity)))?;
            Some((Vec3::from_array(p), Reach::sphere(r, 0.0, 0), Vec3::from_array(f)))
        }
        Anchor::Npc(id) => npcs?.skaters.iter().find(|s| s.id == *id).map(|s| (Vec3::from_array(s.state.com_position), Reach::sphere(AUDIO_RADIUS, 0.0, 0), board_facing(&s.state))),
        Anchor::Fixed(p, r) => Some((*p, *r, r.forward)),
        Anchor::Emitter(e, _, r) => emitters(*e).map(|(p, forward)| (p, Reach { forward, ..*r }, forward)),
    }
}


/// What `sdk.audio.play` (native) or a rule's sound asks for (validated by `skate_mods`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VoiceSpec {
    pub path: String,
    pub looping: bool,
    pub volume: f32,
    pub pitch: f32,
    pub paused: bool,
    pub fade_in: f32,
    /// World position (None: a non-positional sound).
    pub position: Option<Vec3>,
    /// The reach of a positional sound (None: the follow's owner's, else the default reach).
    pub reach: Option<Reach>,
    /// A rule sound's owner (positions it every frame, gives the default reach).
    pub follow: Option<Follow>,
    pub reverb: bool,
    pub group: u8,
}

struct Clip {
    pcm: Arc<Pcm>,
    bytes: usize,
}

#[derive(Default)]
struct Bank {
    /// (clip path, looping) → slot.
    slots: BTreeMap<(String, bool), u16>,
    headers: Vec<SampleHeader>,
    pcm: Vec<Arc<Pcm>>,
    /// Slots registered in the current runtime.
    registered: usize,
}

struct Voice {
    spec: VoiceSpec,
    bank: usize,
    slot: u16,
    /// The mixer voice (None: not opened yet in this runtime).
    mixer: Option<u32>,
    /// The private-MixMap instance, the build it belongs to and the evaluation count at the claim.
    instance: Option<(usize, u64, u64)>,
    attack_elapsed: f32,
    /// Fading out: remaining and total seconds.
    stopping: Option<(f32, f32)>,
    paused_applied: bool,
    /// The voice ended or was refused: removed at the next frame.
    done: bool,
    /// A followed owner was found at least once.
    placed: bool,
}

/// The native mod voices and banks.
#[derive(Resource, Default)]
pub(crate) struct ModVoices {
    owners: BTreeMap<String, usize>,
    clips: BTreeMap<(String, String), Clip>,
    banks: BTreeMap<(String, u8), Bank>,
    voices: BTreeMap<(String, String), Voice>,
    /// Mixer voices to release (a key replaced or stopped), and banks to drop, in the runtime
    /// they belong to.
    releases: Vec<u32>,
    drop_banks: Vec<usize>,
    /// Private-MixMap instances to free (instance, build).
    free_instances: Vec<(usize, u64)>,
    /// The runtime the banks and voices live in.
    runtime: Option<(usize, u64)>,
    /// Rule sounds started (`mod_rules`: the voice key ring).
    rule_plays: u64,
}

impl ModVoices {
    fn bank_id(&mut self, owner: &str, group: u8) -> usize {
        let n = self.owners.len();
        let index = *self.owners.entry(owner.to_owned()).or_insert(n);
        MOD_BANK_BASE + 2 * index + usize::from(group.min(1))
    }

    pub(crate) fn has_clip(&self, owner: &str, path: &str) -> bool {
        self.clips.contains_key(&(owner.to_owned(), path.to_owned()))
    }

    /// Clips and their bytes (as WAV bytes) of one owner and in all.
    pub(crate) fn clip_usage(&self, owner: &str) -> (usize, usize, usize, usize) {
        let mine = self.clips.iter().filter(|((o, _), _)| o == owner);
        let (n, b) = mine.fold((0, 0), |(n, b), (_, c)| (n + 1, b + c.bytes));
        (n, b, self.clips.len(), self.clips.values().map(|c| c.bytes).sum())
    }

    /// Native voices of one owner and in all.
    pub(crate) fn voice_usage(&self, owner: &str) -> (usize, usize) {
        (self.voices.keys().filter(|(o, _)| o == owner).count(), self.voices.len())
    }

    pub(crate) fn has_voice(&self, owner: &str, key: &str) -> bool {
        self.voices.get(&(owner.to_owned(), key.to_owned())).is_some_and(|v| !v.done)
    }

    /// Decode a canonical PCM16 WAV (`skate_mods::audio::canonical_pcm_wav`) as a clip.
    pub(crate) fn add_clip(&mut self, owner: &str, path: &str, wav: &[u8]) -> Result<(), String> {
        let pcm = super::library::wav_pcm(wav).ok_or_else(|| format!("{path}: not a PCM16 WAV"))?;
        self.clips.insert((owner.to_owned(), path.to_owned()), Clip { pcm: Arc::new(pcm), bytes: wav.len() });
        Ok(())
    }

    /// The (bank, slot) of a clip, played once or looping, in a volume group.
    pub(crate) fn slot(&mut self, owner: &str, path: &str, looping: bool, group: u8) -> Result<(usize, u16), String> {
        let pcm = self.clips.get(&(owner.to_owned(), path.to_owned())).map(|c| c.pcm.clone()).ok_or_else(|| format!("{path}: not loaded"))?;
        let id = self.bank_id(owner, group);
        let bank = self.banks.entry((owner.to_owned(), group.min(1))).or_default();
        if let Some(&slot) = bank.slots.get(&(path.to_owned(), looping)) {
            return Ok((id, slot));
        }
        let slot = u16::try_from(bank.headers.len()).map_err(|_| "too many mod samples")?;
        bank.headers.push(super::library::mod_header(None, &pcm, looping.then_some(0)));
        bank.pcm.push(pcm);
        bank.slots.insert((path.to_owned(), looping), slot);
        Ok((id, slot))
    }

    /// Start (or restart) `owner`'s `key`. Opened at the next audio pass.
    pub(crate) fn play(&mut self, owner: &str, key: &str, spec: VoiceSpec) -> Result<(), String> {
        let (bank, slot) = self.slot(owner, &spec.path, spec.looping, spec.group)?;
        if let Some(old) = self.voices.remove(&(owner.to_owned(), key.to_owned())) {
            self.forget(old);
        }
        self.voices.insert((owner.to_owned(), key.to_owned()), Voice { spec, bank, slot, mixer: None, instance: None, attack_elapsed: 0.0, stopping: None, paused_applied: false, done: false, placed: false });
        Ok(())
    }

    /// The voice's mixer id is released at the next pass; its instance is freed there too.
    fn forget(&mut self, v: Voice) {
        if let Some(id) = v.mixer {
            self.releases.push(id);
        }
        if let Some((g, build, _)) = v.instance {
            self.free_instances.push((g, build));
        }
    }

    pub(crate) fn update(&mut self, owner: &str, key: &str, volume: Option<f32>, pitch: Option<f32>, paused: Option<bool>) {
        let Some(v) = self.voices.get_mut(&(owner.to_owned(), key.to_owned())) else { return };
        if v.stopping.is_some() {
            return;
        }
        if let Some(x) = volume {
            v.spec.volume = x;
        }
        if let Some(x) = pitch {
            v.spec.pitch = x;
        }
        if let Some(x) = paused {
            v.spec.paused = x;
        }
    }

    /// The sound's world position this frame (the mod system resolves bodies and offsets).
    pub(crate) fn set_position(&mut self, owner: &str, key: &str, position: Vec3) {
        if let Some(v) = self.voices.get_mut(&(owner.to_owned(), key.to_owned())) {
            if v.spec.position.is_some() {
                v.spec.position = Some(position);
            }
        }
    }

    pub(crate) fn stop(&mut self, owner: &str, key: &str, fade: f32) {
        let k = (owner.to_owned(), key.to_owned());
        if fade <= 0.0 {
            if let Some(v) = self.voices.remove(&k) {
                self.forget(v);
            }
        } else if let Some(v) = self.voices.get_mut(&k) {
            if v.stopping.is_none() {
                v.stopping = Some((fade, fade));
            }
        }
    }

    /// Every voice of `owner` stops; with `release_clips` its clips and banks go too.
    pub(crate) fn stop_owner(&mut self, owner: &str, release_clips: bool) {
        let keys: Vec<_> = self.voices.keys().filter(|(o, _)| o == owner).cloned().collect();
        for k in keys {
            if let Some(v) = self.voices.remove(&k) {
                self.forget(v);
            }
        }
        if release_clips {
            self.clips.retain(|(o, _), _| o != owner);
            let banks: Vec<_> = self.banks.keys().filter(|(o, _)| o == owner).cloned().collect();
            for (o, group) in banks {
                self.banks.remove(&(o.clone(), group));
                let id = self.bank_id(&o, group);
                self.drop_banks.push(id);
            }
        }
    }

    /// An audio-only reload changed these files of `owner` (doc 16 L7): their clips are dropped
    /// (the next play or rule compile reads the new file) and the voices playing them stop. Their
    /// old slots stay registered in the mod's bank until the mod stops (a later play takes a new
    /// slot). Returns whether anything was dropped.
    pub(crate) fn forget_clips(&mut self, owner: &str, paths: &[String]) -> bool {
        let mut any = false;
        let mut slots: Vec<(usize, u16)> = Vec::new();
        for path in paths {
            any |= self.clips.remove(&(owner.to_owned(), path.clone())).is_some();
            for group in 0..2u8 {
                if !self.banks.contains_key(&(owner.to_owned(), group)) {
                    continue;
                }
                let id = self.bank_id(owner, group);
                if let Some(bank) = self.banks.get_mut(&(owner.to_owned(), group)) {
                    for looping in [false, true] {
                        if let Some(slot) = bank.slots.remove(&(path.clone(), looping)) {
                            slots.push((id, slot));
                        }
                    }
                }
            }
        }
        let keys: Vec<_> = self.voices.iter().filter(|((o, _), v)| o == owner && slots.contains(&(v.bank, v.slot))).map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(v) = self.voices.remove(&k) {
                self.forget(v);
            }
        }
        any || !slots.is_empty()
    }

    /// Everything (a map change: the mod system clears every mod's runtime state).
    pub(crate) fn clear(&mut self) {
        let owners: Vec<String> = self.owners.keys().cloned().collect();
        for o in owners {
            self.stop_owner(&o, true);
        }
    }

    fn idle(&self) -> bool {
        self.voices.is_empty() && self.releases.is_empty() && self.drop_banks.is_empty() && self.free_instances.is_empty()
    }
}

/// The audio pass, after the ticks (`mod_audio::readback`): the private MixMap's evaluations for
/// this pass, then every native mod voice opened, updated from its instance's words, paused,
/// faded or ended. Returns at once while there is nothing to do.
#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    native: Option<Res<Native>>,
    library: Option<Res<Library>>,
    mut mix: ResMut<ModMix>,
    mut voices: ResMut<ModVoices>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    cues: Res<super::skate_events::Cues>,
    content: Res<super::AudioContent>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    rules: Option<Res<super::mod_rules::AudioRules>>,
    owners: Option<Res<super::world_sources::WorldOwners>>,
    npcs: Option<Res<super::npc_skaters::NpcSkaters>>,
    published: Query<(&GlobalTransform, &crate::world_audio::WorldEmitter)>,
) {
    let plays = rules.as_deref().and_then(|r| r.set.as_ref()).filter(|s| s.has_plays());
    if voices.idle() && !mix.in_use() && plays.is_none() {
        return;
    }
    let (Some(native), Some(library)) = (native, library) else { return };
    let mv = &mut *voices;
    // The rules' sounds queued at the sites since the last pass (`mod_rules`): one-shots of the
    // rules' mod banks, placed at their owners (or where the rule says).
    if let Some(set) = plays {
        let mut counter = mv.rule_plays;
        for (owner, key, spec) in set.take_plays(&mut counter) {
            if let Err(e) = mv.play(&owner, &key, spec) {
                warn!("Game audio: a rule sound of {owner}: {e}");
            }
        }
        mv.rule_plays = counter;
    }
    let runtime = (Arc::as_ptr(&native.shared) as usize, content.runtime_generation);
    if mv.runtime != Some(runtime) {
        // A new runtime (first use, or a restart): the old one's mixer ids are forgotten, never
        // released into this one; the banks are registered again.
        mv.releases.clear();
        mv.drop_banks.clear();
        mv.free_instances.clear();
        for b in mv.banks.values_mut() {
            b.registered = 0;
        }
        for v in mv.voices.values_mut() {
            if v.mixer.take().is_some() && !v.spec.looping {
                v.done = true;
            }
            v.instance = None;
        }
        mv.runtime = Some(runtime);
    }
    mix.ensure(&library, &native, content.runtime_generation);
    for (g, build) in std::mem::take(&mut mv.free_instances) {
        if build == mix.build {
            mix.release(g);
        }
    }
    if let (Some(pass), Some(retail)) = (native.pending, native.mixmap.as_ref()) {
        mix.tick(retail, pass.calls);
    }
    let silenced = super::silenced(menu.as_deref(), &replay);
    let dt = if silenced { 0.0 } else { time.delta_secs().clamp(0.0, 0.25) };
    let ear = listener.single().ok();
    let skater = cues.riding.board;
    let Ok(mut rt) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) else { return };
    for id in mv.releases.drain(..) {
        VoiceHost::release(&mut rt.mixer, id);
    }
    for id in mv.drop_banks.drain(..) {
        rt.mixer.remove_bank(id);
    }
    // New samples of the mod banks.
    let ids: Vec<((String, u8), usize)> = mv.banks.keys().map(|k| (k.clone(), 0)).collect();
    for ((owner, group), _) in ids {
        let id = mv.bank_id(&owner, group);
        let Some(b) = mv.banks.get_mut(&(owner, group)) else { continue };
        if b.registered < b.headers.len() {
            for slot in b.registered..b.headers.len() {
                rt.mixer.set_bank_sample(id, slot as u16, b.headers[slot], b.pcm[slot].clone());
            }
            rt.mixer.set_bank_group(id, if group == 1 { GROUP_PLAYER } else { GROUP_WORLD });
            b.registered = b.headers.len();
        }
    }
    let route = Route { output: Output::Master, create: false, owner_env: 0.0, mono: false };
    for v in mv.voices.values_mut() {
        if v.done {
            continue;
        }
        // Fades (game time while not silenced, as the Bevy voices).
        let active = !(silenced || v.spec.paused);
        if active {
            v.attack_elapsed += dt;
        }
        let attack = if v.spec.fade_in <= 0.0 { 1.0 } else { (v.attack_elapsed / v.spec.fade_in).min(1.0) };
        let mut release = 1.0;
        if let Some((remaining, total)) = &mut v.stopping {
            *remaining -= dt;
            release = (*remaining / *total).max(0.0);
            if *remaining <= 0.0 {
                v.done = true;
                continue;
            }
        }
        // The private-MixMap instance (claimed once; read after an evaluation that saw it).
        if v.instance.is_none_or(|(_, build, _)| build != mix.build) {
            v.instance = mix.claim().map(|g| (g, mix.build, mix.ticks));
        }
        let Some((g, _, claimed_at)) = v.instance else { continue };
        // A rule sound's owner: its position now (when followed) and its default reach. An owner
        // gone after it was found leaves the sound where it was; one never found plays it centred.
        if let Some(f) = v.spec.follow {
            let emitter = |bits: u64| {
                let (t, e) = published.get(Entity::try_from_bits(bits)?).ok()?;
                let (_, rotation, position) = t.to_scale_rotation_translation();
                Some((position, (rotation * e.forward).normalize_or(Vec3::X)))
            };
            match locate_facing(&f.anchor, &cues, owners.as_deref(), npcs.as_deref(), &emitter) {
                Some((p, reach, facing)) => {
                    if f.track {
                        v.spec.position = Some(p + if f.local { owner_offset(f.offset, facing) } else { f.offset });
                    }
                    v.spec.reach.get_or_insert(reach);
                    v.placed = true;
                }
                None if !v.placed => {
                    if f.track {
                        v.spec.position = None;
                    }
                    v.spec.follow = None;
                }
                None => {}
            }
        }
        let level = v.spec.volume * attack * release;
        let words = match (v.spec.position, ear) {
            (Some(at), Some(listener)) => {
                let reach = v.spec.reach.unwrap_or_else(|| Reach::from_falloff(skate_mods::audio::DEFAULT_REACH));
                let shape = reach.level(at, listener.translation());
                let w = mix.words(g, level * shape, 0);
                mix.set_position(g, listener, skater, at);
                w
            }
            (Some(_), None) => None,
            (None, _) => mix.words_flat(g, level),
        };
        let Some(w) = words else { continue };
        // A positional instance is read after an evaluation that saw its position; the
        // non-positional outputs depend on the global inputs only (any evaluation will do).
        let ready = if v.spec.position.is_some() { mix.ticks > claimed_at } else { mix.ticks > 0 };
        if !ready {
            continue;
        }
        let gain = w[1] as f32 * INV_32767;
        let send = if v.spec.reverb { w[2] as f32 * INV_32767 } else { 0.0 };
        let azimuth = (v.spec.position.is_some()).then(|| w[3] as f32 * DEGREES_PER_UNIT);
        let pitch = w[4] as f32 / 4096.0 * v.spec.pitch;
        let lpf = w[5] as f32;
        let id = match v.mixer {
            Some(id) => {
                if !rt.mixer.direct_alive(id) {
                    v.done = true;
                    continue;
                }
                rt.mixer.set_direct(id, pitch, gain, azimuth.or(Some(0.0)));
                id
            }
            None => match rt.mixer.open_routed(v.bank, v.slot, 0.0, pitch, gain, azimuth.or(Some(0.0)), route) {
                Some(id) => {
                    v.mixer = Some(id);
                    v.paused_applied = false;
                    id
                }
                None => {
                    warn!("Game audio: a native mod sound was refused (the mixer's voices are all in use)");
                    v.done = true;
                    continue;
                }
            },
        };
        rt.mixer.set_direct_dsp(id, 0.0, lpf, send);
        if v.spec.paused != v.paused_applied {
            if v.spec.paused {
                VoiceHost::pause(&mut rt.mixer, id);
            } else {
                VoiceHost::resume(&mut rt.mixer, id);
            }
            v.paused_applied = v.spec.paused;
        }
    }
    // Ended voices leave (their instance freed, a fading one's voice released).
    let ended: Vec<_> = mv.voices.iter().filter(|(_, v)| v.done).map(|(k, _)| k.clone()).collect();
    for k in ended {
        if let Some(v) = mv.voices.remove(&k) {
            if let Some(id) = v.mixer {
                VoiceHost::release(&mut rt.mixer, id);
            }
            if let Some((g, build, _)) = v.instance {
                if build == mix.build {
                    mix.release(g);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// A mono PCM16 sine WAV (`hz`, `seconds`, peak `amp` of 32767).
    pub(crate) fn tone_wav(hz: f32, seconds: f32, rate: u32, amp: f32) -> Vec<u8> {
        let frames = (seconds * rate as f32) as usize;
        let data = frames * 2;
        let mut b = b"RIFF".to_vec();
        b.extend((36 + data as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend([2, 0, 16, 0]);
        b.extend(b"data");
        b.extend((data as u32).to_le_bytes());
        for i in 0..frames {
            let v = (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * amp * 32767.0;
            b.extend((v as i16).to_le_bytes());
        }
        b
    }

    pub(crate) fn spec(path: &str, position: Option<Vec3>) -> VoiceSpec {
        VoiceSpec { path: path.into(), looping: true, volume: 1.0, pitch: 1.0, paused: false, fade_in: 0.0, position, reach: position.map(|_| Reach::sphere(30.0, 0.0, 0)), follow: None, reverb: true, group: 0 }
    }

    /// `play.frame = 'owner'`: an offset in the owner's axes (x right, y up, z facing, the facing
    /// flattened to the ground), world +Z without a facing; the owners' facings (a car's direction,
    /// a ped's walk, a skater's board from its wheels, an emitter's forward).
    #[test]
    fn offsets_turn_with_the_owner() {
        let o = Vec3::new(1.0, 0.5, 2.0);
        assert!(owner_offset(o, Vec3::Z).abs_diff_eq(Vec3::new(-1.0, 0.5, 2.0), 1e-6), "facing +Z: right is -X");
        assert!(owner_offset(o, Vec3::X).abs_diff_eq(Vec3::new(2.0, 0.5, 1.0), 1e-6));
        assert!(owner_offset(o, Vec3::new(0.0, -3.0, 0.0)).abs_diff_eq(owner_offset(o, Vec3::Z), 1e-6), "no horizontal facing: +Z");
        assert!(owner_offset(o, Vec3::new(0.0, 5.0, -2.0)).abs_diff_eq(Vec3::new(1.0, 0.5, -2.0), 1e-6), "pitched facing flattened");
        let mut cues = super::super::skate_events::Cues::default();
        cues.riding.audio.wheel_position = [[1.0, 0.0, 0.5], [1.0, 0.0, -0.5], [0.0, 0.0, 0.5], [0.0, 0.0, -0.5]];
        let none = |_: u64| None;
        assert_eq!(locate_facing(&Anchor::Player, &cues, None, None, &none).unwrap().2, Vec3::new(1.0, 0.0, 0.0), "the board's nose");
        let mut owners = super::super::world_sources::WorldOwners::default();
        owners.vehicles.insert(7, skate_audio::world::traffic::VehicleState { direction: [0.0, 0.0, -1.0], ..Default::default() });
        assert_eq!(locate_facing(&Anchor::World(7), &cues, Some(&owners), None, &none).unwrap().2, Vec3::new(0.0, 0.0, -1.0));
        let r = Reach::sphere(5.0, 0.0, 0);
        let moved = |_: u64| Some((Vec3::new(9.0, 0.0, 9.0), Vec3::X));
        assert_eq!(locate_facing(&Anchor::Emitter(3, Vec3::ZERO, r), &cues, None, None, &moved).unwrap().0, Vec3::new(9.0, 0.0, 9.0), "a published emitter where it is now");
        assert_eq!(locate_facing(&Anchor::Emitter(3, Vec3::ZERO, r), &cues, None, None, &none), None, "gone");
    }

    /// The per-mod banks: a clip gets one slot per play mode (once / looping) in its group's bank;
    /// every mod has its own banks; a key's play replaces its voice; stopping an owner with its
    /// clips drops its banks (released in the runtime at the next pass).
    #[test]
    fn mod_banks_slots_and_cleanup() {
        let mut v = ModVoices::default();
        let wav = tone_wav(440.0, 0.1, 22050, 0.5);
        v.add_clip("dev.a", "a.wav", &wav).unwrap();
        v.add_clip("dev.a", "b.wav", &wav).unwrap();
        v.add_clip("dev.b", "a.wav", &wav).unwrap();
        assert!(v.add_clip("dev.a", "bad.wav", b"RIFF....WAVE").is_err());
        let once = v.slot("dev.a", "a.wav", false, 0).unwrap();
        let looping = v.slot("dev.a", "a.wav", true, 0).unwrap();
        assert_eq!((once.0, looping.0), (MOD_BANK_BASE, MOD_BANK_BASE));
        assert_ne!(once.1, looping.1, "a looping play has its own slot");
        assert_eq!(v.slot("dev.a", "a.wav", false, 0).unwrap(), once, "the same slot again");
        let player = v.slot("dev.a", "b.wav", false, 1).unwrap();
        assert_eq!(player.0, MOD_BANK_BASE + 1, "the player group's bank");
        assert_eq!(v.slot("dev.b", "a.wav", false, 0).unwrap().0, MOD_BANK_BASE + 2, "another mod, another bank");
        let bank = &v.banks[&("dev.a".to_owned(), 0)];
        assert_eq!(bank.headers[usize::from(looping.1)].loop_start, Some(0));
        assert_eq!(bank.headers[usize::from(once.1)].loop_start, None);
        assert_eq!((bank.headers[0].rate, bank.headers[0].channels, bank.headers[0].frames), (22050, 1, 2205));
        assert!(v.slot("dev.a", "missing.wav", false, 0).is_err());
        v.play("dev.a", "k", spec("a.wav", None)).unwrap();
        v.voices.get_mut(&("dev.a".to_owned(), "k".to_owned())).unwrap().mixer = Some(7);
        v.play("dev.a", "k", spec("b.wav", None)).unwrap();
        assert_eq!(v.releases, [7], "the key's last voice is released");
        assert_eq!(v.voice_usage("dev.a"), (1, 1));
        v.stop("dev.a", "k", 0.5);
        v.stop("dev.a", "k", 2.0);
        assert_eq!(v.voices[&("dev.a".to_owned(), "k".to_owned())].stopping, Some((0.5, 0.5)), "repeated stops keep the first fade");
        let (n, bytes, all, _) = v.clip_usage("dev.a");
        assert_eq!((n, bytes, all), (2, 2 * wav.len(), 3));
        v.stop_owner("dev.a", true);
        assert_eq!(v.voice_usage("dev.a"), (0, 0));
        assert_eq!(v.clip_usage("dev.a").0, 0);
        let mut dropped = v.drop_banks.clone();
        dropped.sort();
        assert_eq!(dropped, [MOD_BANK_BASE, MOD_BANK_BASE + 1]);
        assert!(v.has_clip("dev.b", "a.wav"), "another mod keeps its clips");
        v.clear();
        assert_eq!(v.clip_usage("dev.b").2, 0);
    }

    pub(crate) fn library() -> Library {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        Library::load(root).unwrap_or_else(|_| panic!("missing private data: no audio install"))
    }

    /// The private MixMap's Emitter instances give exactly a retail Emitter instance's words
    /// (data-gated): the retail MixMap (retail layout) and the private one (Global + 32
    /// Emitters, the Global inputs copied before each tick as `ModMix::tick` does) see the same
    /// positions on retail instance 3 and private instances 3 and 29 (moving 1 → 95 m, turning,
    /// with a reverb change in the global inputs); every word of every tick is equal, positional
    /// and non-positional.
    #[test]
    #[ignore = "needs the private install data"]
    fn the_private_mixmap_matches_the_retail_emitter_outputs() {
        let library = library();
        let Some(file) = library.aems().mixmap.clone() else { panic!("missing private data: no MixMap") };
        let parsed = MixMapFile::parse(&library.read(&file).unwrap()).unwrap();
        let mut retail = MixMap::new(&parsed, &super::super::native::WorldInstances::RETAIL.mixmap_instances());
        let mut mix = ModMix { mix: Some(MixMap::new(&parsed, &mix_instances())), ..Default::default() };
        mix.globals = retail.controller_keys().filter(|k| (k >> 16) & 0xFF == keys::slot::GLOBAL).collect();
        mix.used[3] = true;
        mix.used[29] = true;
        let mut compared = 0;
        let mut moved = std::collections::BTreeSet::new();
        for t in 0..400 {
            for id in 1..=4 {
                retail.set_input(keys::MASTER, id, 32767);
            }
            for id in [1, 2, 5] {
                retail.set_input(keys::MUSIC, id, 32767);
            }
            // reverb01 (in4), then a zone preset (in5) half way.
            for id in 0..7 {
                retail.set_input(keys::REVERB, id, if (t < 200 && id == 4) || (t >= 200 && id == 5) { 32767 } else { 0 });
            }
            let angle = t as f32 * 0.05;
            let distance = 1.0 + t as f32 * 0.235;
            let listener = GlobalTransform::from(Transform::from_xyz(0.0, 1.0, 0.0).with_rotation(Quat::from_rotation_y(angle)));
            let source = Vec3::new(distance * 0.6, 0.0, -distance * 0.8);
            let skater = Vec3::new(0.5, 0.0, -1.0);
            super::super::native::write_position(&mut retail, keys::emitter_pos(3), &listener, skater, source);
            mix.set_position(3, &listener, skater, source);
            mix.set_position(29, &listener, skater, source);
            retail.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
            mix.tick(&retail, 1);
            let m = mix.mixmap().unwrap();
            for level in [1.0, 0.5, 0.123] {
                let want = emitter_words(&retail, 3, level, 81);
                assert_eq!(emitter_words(m, 3, level, 81), want, "tick {t}");
                assert_eq!(emitter_words(m, 29, level, 81), want, "tick {t}: another instance");
                assert_eq!(emitter_words_flat(m, 29, level), emitter_words_flat(&retail, 3, level), "tick {t}: non-positional");
                compared += 1;
            }
            moved.insert(emitter_words(m, 3, 1.0, 0)[2]);
        }
        assert!(moved.len() > 10, "the send rolls off with the distance: {moved:?}");
        println!("{compared} word sets equal; {} distinct sends", moved.len());
    }

    /// A world with what [`frame`] reads, the install's runtime started.
    pub(crate) fn world(library: Library) -> World {
        let mut world = World::new();
        let native = Native::start(&library).unwrap_or_else(|e| panic!("missing private data: {e}"));
        world.insert_resource(native);
        world.insert_resource(library);
        world.init_resource::<ModMix>();
        world.init_resource::<ModVoices>();
        world.init_resource::<super::super::skate_events::Cues>();
        world.init_resource::<super::super::AudioContent>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<crate::replay::Replay>();
        world.spawn((super::super::GameAudioListener, Transform::default(), GlobalTransform::default()));
        world
    }

    /// One frame: a pass of one console evaluation, the voices' frame, 3 blocks rendered (stereo).
    pub(crate) fn step(world: &mut World, out: &mut Vec<f32>) {
        {
            // The Global inputs `native::mixmap_frame` writes every pass (free skate).
            let mut native = world.resource_mut::<Native>();
            native.test_pass(1);
            let m = native.mixmap.as_mut().expect("missing private data: no MixMap");
            for id in 1..=4 {
                m.set_input(keys::MASTER, id, 32767);
            }
            for id in [1, 2, 5] {
                m.set_input(keys::MUSIC, id, 32767);
            }
            m.set_input(keys::REVERB, 4, 32767);
            m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
        }
        world.run_system_once(frame).unwrap();
        let mut block = vec![0.0f32; 2 * skate_audio::BLOCK];
        for _ in 0..3 {
            world.resource::<Native>().shared.lock().unwrap().fill_stereo(&mut block);
            out.extend_from_slice(&block);
        }
    }

    pub(crate) fn rms(s: &[f32], channel: usize) -> f32 {
        let n = s.len() / 2;
        (s.iter().skip(channel).step_by(2).map(|x| x * x).sum::<f32>() / n.max(1) as f32).sqrt()
    }

    pub(crate) fn mod_voice(world: &World) -> Option<skate_audio::mixer::VoiceInfo> {
        world.resource::<Native>().shared.lock().unwrap().mixer.snapshot().into_iter().find(|v| v.bank >= MOD_BANK_BASE)
    }

    /// A mod WAV through the native mixer (data-gated): a looping tone 5 m to the listener's
    /// right plays in its mod bank, louder on the right, with an environment send and the
    /// emitter's dry level; out of its reach it is silent (and keeps its voice); a
    /// non-positional one is centred; `reverb = false` has no send; stopping the mod frees the
    /// voice, the bank, the private-MixMap instance, and the frame idles again.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_native_mod_voice_plays_through_the_mixer_with_retail_pan_and_send() {
        let mut world = world(library());
        let wav = tone_wav(440.0, 1.0, 44100, 0.5);
        world.resource_mut::<ModVoices>().add_clip("dev.a", "tone.wav", &wav).unwrap();
        world.resource_mut::<ModVoices>().play("dev.a", "tone", spec("tone.wav", Some(Vec3::new(5.0, 0.0, 0.0)))).unwrap();
        let mut out = Vec::new();
        for _ in 0..40 {
            step(&mut world, &mut out);
        }
        let v = mod_voice(&world).expect("the mod voice is in the mixer");
        assert_eq!(v.bank, MOD_BANK_BASE);
        assert!(v.gain > 0.1 && v.send > 0.0, "dry and send: {v:?}");
        let tail = &out[out.len() / 2..];
        let (left, right) = (rms(tail, 0), rms(tail, 1));
        assert!(right > 2.0 * left && right > 0.01, "panned right: L {left} R {right}");
        assert_eq!(world.resource::<ModMix>().used.iter().filter(|u| **u).count(), 1);
        // Out of its 30 m reach: silent, the voice kept (a loop resumes when the listener returns).
        world.resource_mut::<ModVoices>().set_position("dev.a", "tone", Vec3::new(500.0, 0.0, 0.0));
        let mut far = Vec::new();
        for _ in 0..10 {
            step(&mut world, &mut far);
        }
        let v = mod_voice(&world).expect("kept");
        assert_eq!(v.gain, 0.0);
        // Non-positional, no reverb: centred and dry only.
        world.resource_mut::<ModVoices>().play("dev.a", "tone", VoiceSpec { reverb: false, ..spec("tone.wav", None) }).unwrap();
        let mut flat = Vec::new();
        for _ in 0..40 {
            step(&mut world, &mut flat);
        }
        let v = mod_voice(&world).expect("the non-positional voice");
        assert_eq!(v.send, 0.0);
        let tail = &flat[flat.len() / 2..];
        assert!((rms(tail, 0) - rms(tail, 1)).abs() < 1e-4 && rms(tail, 0) > 0.001, "centred: {} {}", rms(tail, 0), rms(tail, 1));
        // The mod stops: everything goes.
        world.resource_mut::<ModVoices>().stop_owner("dev.a", true);
        let mut after = Vec::new();
        step(&mut world, &mut after);
        assert!(mod_voice(&world).is_none());
        assert!(!world.resource::<ModMix>().in_use());
        assert!(world.resource::<ModVoices>().idle());
        assert!(world.resource::<Native>().shared.lock().unwrap().mixer.open_direct(MOD_BANK_BASE, 0, 0.0, 1.0, 1.0, None).is_none(), "the bank is gone");
    }

    /// A one-shot ends by itself and leaves; a paused voice is paused in the mixer; a fade-out
    /// stops a loop (data-gated).
    #[test]
    #[ignore = "needs the private install data"]
    fn native_one_shots_end_and_fades_stop() {
        let mut world = world(library());
        let wav = tone_wav(300.0, 0.2, 48000, 0.5);
        world.resource_mut::<ModVoices>().add_clip("dev.a", "pip.wav", &wav).unwrap();
        world.resource_mut::<ModVoices>().play("dev.a", "pip", VoiceSpec { looping: false, ..spec("pip.wav", None) }).unwrap();
        world.resource_mut::<ModVoices>().play("dev.a", "loop", VoiceSpec { paused: true, ..spec("pip.wav", None) }).unwrap();
        let mut out = Vec::new();
        for _ in 0..5 {
            step(&mut world, &mut out);
        }
        assert!(world.resource::<ModVoices>().has_voice("dev.a", "pip"));
        assert!(world.resource::<Native>().shared.lock().unwrap().mixer.snapshot().iter().any(|v| v.bank >= MOD_BANK_BASE && v.paused), "paused in the mixer");
        for _ in 0..30 {
            step(&mut world, &mut out);
        }
        assert!(!world.resource::<ModVoices>().has_voice("dev.a", "pip"), "the one-shot ended");
        world.resource_mut::<ModVoices>().update("dev.a", "loop", None, None, Some(false));
        world.resource_mut::<ModVoices>().stop("dev.a", "loop", 0.05);
        for _ in 0..5 {
            world.resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_millis(20));
            step(&mut world, &mut out);
        }
        assert!(!world.resource::<ModVoices>().has_voice("dev.a", "loop"), "faded out");
        assert!(mod_voice(&world).is_none());
    }

    /// A removed mod restores retail (data-gated): the same retail scenario (a DownTown emitter's
    /// `c_emitter` post, redelivered) with and without a native mod voice that plays for half a
    /// second and stops with its mod. Before the mod plays both are bit-identical; while it plays
    /// they differ; after it the mod leaves no voice, bank, instance or work behind, the retail
    /// voices are the same, and once the environment network's tail of the mod's send has died
    /// away the output is the retail output again (bit for bit in the last second).
    #[test]
    #[ignore = "needs the private install data"]
    fn a_removed_native_mod_voice_restores_retail() {
        let run = |with_mod: bool| {
            let library = library();
            let r = library.emitters("sfx_downtown").iter().find(|r| r.kind == 1 && r.bank.as_ref().is_some_and(|b| library.aems().banks.contains_key(b))).cloned().expect("a DownTown emitter");
            let mut world = world(library);
            let node = {
                let lib = world.remove_resource::<Library>().unwrap();
                let mut native = world.resource_mut::<Native>();
                native.ensure_bank(&lib, r.bank.as_deref().unwrap()).unwrap();
                let payload = native.emitter_payload(None, 0.8, 9000, r.patch);
                let node = native.post_emitter(&payload).unwrap();
                drop(native);
                world.insert_resource(lib);
                (node, payload)
            };
            let mut out = Vec::new();
            let mut marks = Vec::new();
            for f in 0..600 {
                if f % 4 == 0 {
                    world.resource::<Native>().redeliver(node.0, &node.1);
                }
                if with_mod && f == 100 {
                    let wav = tone_wav(523.0, 0.5, 32000, 0.6);
                    let mut v = world.resource_mut::<ModVoices>();
                    v.add_clip("dev.a", "t.wav", &wav).unwrap();
                    v.play("dev.a", "t", spec("t.wav", Some(Vec3::new(-3.0, 0.0, -4.0)))).unwrap();
                }
                if with_mod && f == 130 {
                    world.resource_mut::<ModVoices>().stop_owner("dev.a", true);
                }
                marks.push(out.len());
                step(&mut world, &mut out);
            }
            let voices: Vec<_> = world.resource::<Native>().shared.lock().unwrap().mixer.snapshot().into_iter().map(|v| (v.bank, v.slot, v.gain.to_bits())).collect();
            (out, marks, voices, world.resource::<ModVoices>().idle(), world.resource::<ModMix>().in_use())
        };
        let (retail, marks, retail_voices, _, _) = run(false);
        let (modded, _, modded_voices, idle, in_use) = run(true);
        let same = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        assert!(same(&retail[..marks[100]], &modded[..marks[100]]), "identical before the mod plays");
        assert!(!same(&retail[marks[100]..marks[140]], &modded[marks[100]..marks[140]]), "the mod is heard");
        assert!(idle && !in_use, "nothing of the mod is left");
        assert_eq!(retail_voices, modded_voices, "the same retail voices");
        let last = marks[600 - 60];
        let diff = retail[last..].iter().zip(&modded[last..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        let first_equal = (130..600).find(|&f| same(&retail[marks[f]..], &modded[marks[f]..]));
        println!("after the mod: max difference in the last second {diff:e}; bit-identical from frame {first_equal:?}");
        assert!(same(&retail[last..], &modded[last..]), "the last second is retail again (max diff {diff:e})");
    }

    /// An audio restart (data-gated): the old runtime's mixer ids are forgotten, never released
    /// into the new one; the mod bank is registered in the new runtime, a looping voice opens
    /// again there and a started one-shot is gone.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_restart_reopens_loops_in_the_new_runtime() {
        let mut world = world(library());
        let wav = tone_wav(440.0, 0.5, 48000, 0.5);
        world.resource_mut::<ModVoices>().add_clip("dev.a", "t.wav", &wav).unwrap();
        world.resource_mut::<ModVoices>().play("dev.a", "loop", spec("t.wav", None)).unwrap();
        world.resource_mut::<ModVoices>().play("dev.a", "once", VoiceSpec { looping: false, ..spec("t.wav", None) }).unwrap();
        let mut out = Vec::new();
        for _ in 0..5 {
            step(&mut world, &mut out);
        }
        let old = world.resource::<Native>().shared.clone();
        assert_eq!(old.lock().unwrap().mixer.snapshot().iter().filter(|v| v.bank >= MOD_BANK_BASE).count(), 2);
        // The restart: a new runtime, the content generation bumped.
        let fresh = Native::start(world.resource::<Library>()).unwrap();
        world.insert_resource(fresh);
        world.resource_mut::<super::super::AudioContent>().runtime_generation += 1;
        for _ in 0..3 {
            step(&mut world, &mut out);
        }
        let new: Vec<_> = world.resource::<Native>().shared.lock().unwrap().mixer.snapshot().into_iter().filter(|v| v.bank >= MOD_BANK_BASE).collect();
        assert_eq!(new.len(), 1, "the loop plays again in the new runtime: {new:?}");
        assert!(world.resource::<ModVoices>().has_voice("dev.a", "loop") && !world.resource::<ModVoices>().has_voice("dev.a", "once"));
        assert_eq!(old.lock().unwrap().mixer.snapshot().iter().filter(|v| v.bank >= MOD_BANK_BASE).count(), 2, "nothing was released into (or from) the old runtime");
    }

    /// A replace rule's sound plays through the native mixer (data-gated): the site's request
    /// queues it, the next voices frame opens it in the rule owner's bank (non-positional,
    /// centred), and it ends as a one-shot.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_rule_sound_plays_in_the_mod_bank() {
        let mut world = world(library());
        world.init_resource::<super::super::mod_rules::AudioRules>();
        let wav = tone_wav(600.0, 0.15, 48000, 0.5);
        world.resource_mut::<ModVoices>().add_clip("dev.a", "t.wav", &wav).unwrap();
        let rule: skate_mods::audio_rules::Rule = serde_json::from_value(serde_json::json!({"match": {"tag": "horn"}, "action": "replace", "play": {"path": "t.wav"}})).unwrap();
        let set = super::super::mod_rules::RuleSet::for_test(&[("dev.a", "honk", rule)], Default::default());
        world.resource_mut::<super::super::mod_rules::AudioRules>().set = Some(set.clone());
        let row = super::super::mod_audio::EventRow { kind: super::super::mod_audio::EventKind::Post, source: super::super::mod_audio::Source::World, class: skate_audio::world::traffic::HORN_CLASS, slot: "horn", id: 0, owner: 7, hit: None };
        assert!(set.mutes(&row), "replaced");
        let mut out = Vec::new();
        let mut heard = false;
        for _ in 0..4 {
            step(&mut world, &mut out);
            heard |= mod_voice(&world).is_some_and(|v| v.bank == MOD_BANK_BASE + 1 && v.gain > 0.0);
        }
        assert!(heard, "the rule's sound opened in dev.a's player-group bank");
        for _ in 0..40 {
            step(&mut world, &mut out);
        }
        assert!(mod_voice(&world).is_none(), "a one-shot");
    }

    /// Positional rule sounds (user decision 2026-10-04; data-gated): a replaced horn plays at its
    /// car (panned right at 5 m right, with the environment send), follows the car (left after it
    /// moves left), stays where it was when the car is gone, and is silent beyond the car's retail
    /// reach (40 m); `at = 'world'` plays at the fixed position (left), `offset` moves it, `centre`
    /// is centred; the listener faces −Z (right = +X).
    #[test]
    #[ignore = "needs the private install data"]
    fn a_positional_rule_sound_plays_at_its_owner() {
        use super::super::mod_audio::{EventKind, EventRow, Source};
        use super::super::mod_rules::{AudioRules, RuleSet};
        let mut world = world(library());
        world.init_resource::<AudioRules>();
        world.init_resource::<super::super::world_sources::WorldOwners>();
        let wav = tone_wav(500.0, 3.0, 48000, 0.5);
        world.resource_mut::<ModVoices>().add_clip("dev.a", "t.wav", &wav).unwrap();
        let rule = |v: serde_json::Value| -> skate_mods::audio_rules::Rule { serde_json::from_value(v).unwrap() };
        let set = RuleSet::for_test(&[
            ("dev.a", "horn", rule(serde_json::json!({"match": {"tag": "horn"}, "action": "replace", "play": {"path": "t.wav"}, "min_interval": 0}))),
            ("dev.a", "alarm", rule(serde_json::json!({"match": {"tag": "alarm"}, "action": "layer", "play": {"path": "t.wav", "offset": [-10, 0, 0]}, "min_interval": 0}))),
            ("dev.a", "pop", rule(serde_json::json!({"match": {"source": "player", "id": 1}, "action": "layer", "play": {"path": "t.wav", "at": "world", "position": [-6, 0, 0]}, "min_interval": 0}))),
            ("dev.a", "flat", rule(serde_json::json!({"match": {"source": "player", "id": 2}, "action": "layer", "play": {"path": "t.wav", "at": "centre"}, "min_interval": 0}))),
        ], Default::default());
        world.resource_mut::<AudioRules>().set = Some(set.clone());
        let car = |world: &mut World, at: Option<[f32; 3]>| {
            let mut o = world.resource_mut::<super::super::world_sources::WorldOwners>();
            match at {
                Some(position) => {
                    o.vehicles.insert(7, skate_audio::world::traffic::VehicleState { position, ..Default::default() });
                }
                None => {
                    o.vehicles.remove(&7);
                }
            }
        };
        let world_row = |class: &'static str, slot: &'static str| EventRow { kind: EventKind::Post, source: Source::World, class, slot, id: 0, owner: 7, hit: None };
        let player_row = |id: i32| EventRow { kind: EventKind::Post, source: Source::Player, class: "Class_x", slot: "x", id, owner: 0, hit: None };
        // Run `frames`, return (left, right) RMS of the last half and the mod voice.
        let run = |world: &mut World, frames: usize| {
            let mut out = Vec::new();
            for _ in 0..frames {
                step(world, &mut out);
            }
            let tail = &out[out.len() / 2..];
            (rms(tail, 0), rms(tail, 1), mod_voice(world))
        };
        let reset = |world: &mut World| {
            world.resource_mut::<ModVoices>().stop_owner("dev.a", false);
            let mut out = Vec::new();
            step(world, &mut out);
            assert!(mod_voice(world).is_none());
        };
        // At the car, 5 m right.
        car(&mut world, Some([5.0, 0.0, 0.0]));
        assert!(set.mutes(&world_row(skate_audio::world::traffic::HORN_CLASS, "horn")), "replaced");
        let (l, r, v) = run(&mut world, 30);
        let v = v.expect("the rule's sound plays");
        assert!(r > 2.0 * l && r > 0.005 && v.send > 0.0, "at the car, right: L {l} R {r} {v:?}");
        // The car moves 5 m left: the sound follows.
        car(&mut world, Some([-5.0, 0.0, 0.0]));
        let (l, r, _) = run(&mut world, 20);
        assert!(l > 2.0 * r && l > 0.005, "followed the car left: L {l} R {r}");
        // The car is gone: the sound stays where it was (left).
        car(&mut world, None);
        let (l, r, v) = run(&mut world, 10);
        assert!(v.is_some() && l > 2.0 * r, "stays at the car's last position: L {l} R {r}");
        reset(&mut world);
        // Beyond the car's retail reach (40 m): silent.
        car(&mut world, Some([45.0, 0.0, 0.0]));
        set.mutes(&world_row(skate_audio::world::traffic::HORN_CLASS, "horn"));
        let (_, _, v) = run(&mut world, 20);
        assert_eq!(v.expect("kept").gain, 0.0, "out of the car's 40 m reach");
        reset(&mut world);
        // An offset: the car 5 m right, the sound 10 m to its left (5 m left of the listener).
        car(&mut world, Some([5.0, 0.0, 0.0]));
        assert!(!set.mutes(&EventRow { kind: EventKind::Post, source: Source::World, class: skate_audio::world::traffic::ALARM_CLASS, slot: "alarm", id: 0, owner: 7, hit: None }), "layered");
        let (l, r, _) = run(&mut world, 30);
        assert!(l > 2.0 * r && l > 0.005, "offset to the left: L {l} R {r}");
        reset(&mut world);
        // A fixed world position (6 m left), from a player request.
        assert!(!set.mutes(&player_row(1)));
        let (l, r, _) = run(&mut world, 30);
        assert!(l > 2.0 * r && l > 0.005, "at the fixed position: L {l} R {r}");
        reset(&mut world);
        // Centred.
        assert!(!set.mutes(&player_row(2)));
        let (l, r, _) = run(&mut world, 30);
        assert!((l - r).abs() < 1e-4 && l > 0.001, "centred: L {l} R {r}");
    }
}
