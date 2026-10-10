//! Game audio: per-map ambience, board rolling, trick cues and footsteps,
//! played from the owned disc's sounds that setup decodes to PCM WAV
//! (tools/asset_pipeline/audio_export.py -> assets/private/audio).
//!
//! The skater's sounds, the world emitters and the rolling bed are the native AEMS runtime
//! (`crates/skate-audio`, `native.rs`): retail's patch programs, MixMap and player components fed
//! from the physics state (`skate_events::observe`). The zone beds, location sets and crossfades
//! still play measured layers through Bevy voices (docs/hails-additions/11-audio.md).
//!
//! Loudness is deliberately conservative: master volume defaults to 75%,
//! a cue may raise a quiet clip only until the clip's own peak reaches full
//! scale (at most x4) before the master and category volumes (both <= 1)
//! scale it down (voices.rs), sounds fade in, voice counts are capped, and nothing plays while
//! the menu is open or a replay runs. `--mute` silences game and mod audio.
mod ambience;
mod car_alarm;
mod content;
#[cfg(test)]
mod crossfade_groups;
mod crossfade_layouts;
#[cfg(test)]
mod e2e;
mod emitters;
mod frontend;
mod grain_bed;
mod library;
mod map_audio;
pub(crate) mod mixmap_inputs;
pub(crate) mod mod_audio;
pub(crate) mod mod_rules;
pub(crate) mod mod_voices;
pub(crate) mod mod_world;
mod native;
mod npc_skaters;
mod player_audio;
mod random_programs;
mod random_sets;
pub(crate) mod skate_events;
mod state_log;
mod seed;
mod state_replay;
mod swap;
pub(crate) mod timing;
pub(crate) mod tuning;
mod voices;
pub(crate) mod world_bridge;
mod world_sources;
mod world_speech;

use bevy::{audio::Volume, prelude::*};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) use content::AudioContent;
pub(crate) use library::Library;
pub(crate) use native::Native;
pub(crate) use voices::{Category, Play, Voices};

/// Volume steps for the menu (percent).
const STEP: u32 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct SavedSettings {
    master: u32,
    ambience: u32,
    effects: u32,
    /// Opt-in, NOT retail (user decision 2026-10-03): more audible world objects at once (8 cars,
    /// 24 peds, 3 NPC / remote skaters instead of retail's 4 / 15 / 1; `native::WorldInstances`).
    /// Read at start. `SKATE_AUDIO_MORE_AUDIBLE=1` turns it on for one run.
    more_audible_world: bool,
    /// Where mod emitters (`WorldEmitter`, `sdk.world_audio.spawn(key, 'emitter', …)`) get their
    /// emitter state: `"extra"` (the default, user decision 2026-10-04: their own instances of the
    /// private MixMap, so the map's emitters keep retail's 5) or `"shared"` (retail's rule: they
    /// share the 5 emitter states with the map's emitters, the first reached served first). Read
    /// every frame. `SKATE_AUDIO_MOD_EMITTER_SLOTS=shared|extra` overrides it for one run.
    mod_emitter_slots: ModEmitterSlots,
    /// Retail's option volumes (Settings menu rows SFX / Dialog / Music, settings object +128 /
    /// +132 / +136), fed to the MixMap's Master controller every pass (`RetailVolumes::master_inputs`).
    retail_volumes: RetailVolumes,
    // Files saved before 2026-10-03 may hold `"interim"` (the opt-out to the removed interim cue
    // tables) or the older `"native"`; unknown keys are ignored, so they still load.
}
impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            master: 75,
            ambience: 100,
            effects: 100,
            more_audible_world: false,
            mod_emitter_slots: ModEmitterSlots::Extra,
            retail_volumes: RetailVolumes::default(),
        }
    }
}

/// One of retail's option volumes (the audio rows of the Settings menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetailVolume {
    /// Row 7 `ID_GAMESETTINGS_SFXVOLUME`, settings +128.
    Sfx,
    /// Row 8 `ID_GAMESETTINGS_DIALOGVOLUME`, settings +132.
    Dialog,
    /// Row 9 `ID_GAMESETTINGS_MUSICVOLUME`, settings +136.
    Music,
}

/// Retail's option volumes, 0..1. Retail's routing (TU3 `sub_824D5160`, the MixMap Master
/// controller's step, every pass): input = trunc(volume × 32767.0) (`fmuls` by the constant at
/// 0x821747FC, `fctiwz`), clamped to 0..32767, written to Master.in1 (Music, +136), Master.in2
/// (SFX, +128) and Master.in3 (Dialog, +132). The MixMap data does the rest (mixmap-spec §6.5:
/// in1 → Music, in2 → nearly every SFX sum, in3 → Announcer / NIS / CameraMan speech). Master.in4
/// (emitters, ambience) is not an option volume. Default 1.0 each: the settings constructor is not
/// decoded; the recomp's free-skate capture had Master.in1..in3 = 32767 (mixmap-spec §7.4).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RetailVolumes {
    pub(crate) sfx: f32,
    pub(crate) dialog: f32,
    pub(crate) music: f32,
}
impl Default for RetailVolumes {
    fn default() -> Self {
        Self { sfx: 1.0, dialog: 1.0, music: 1.0 }
    }
}
impl RetailVolumes {
    pub(crate) fn get(&self, volume: RetailVolume) -> f32 {
        match volume {
            RetailVolume::Sfx => self.sfx,
            RetailVolume::Dialog => self.dialog,
            RetailVolume::Music => self.music,
        }
    }
    fn field(&mut self, volume: RetailVolume) -> &mut f32 {
        match volume {
            RetailVolume::Sfx => &mut self.sfx,
            RetailVolume::Dialog => &mut self.dialog,
            RetailVolume::Music => &mut self.music,
        }
    }
    /// The Master controller input of one option volume (`sub_824D5160`).
    pub(crate) fn master_input(volume: f32) -> i32 {
        // `as` truncates toward zero like `fctiwz` and maps NaN to 0.
        ((volume * 32767.0) as i32).clamp(0, 32767)
    }
    /// (Master input id, value) in retail's write order: in1 Music, in2 SFX, in3 Dialog.
    pub(crate) fn master_inputs(&self) -> [(usize, i32); 3] {
        [(1, Self::master_input(self.music)), (2, Self::master_input(self.sfx)), (3, Self::master_input(self.dialog))]
    }
}

/// See `SavedSettings::mod_emitter_slots`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModEmitterSlots {
    Shared,
    #[default]
    Extra,
}

impl ModEmitterSlots {
    /// The setting in force: `SKATE_AUDIO_MOD_EMITTER_SLOTS` (`shared` / `extra`) wins over the
    /// saved value; anything else in the variable is ignored.
    fn in_force(saved: Self, env: Option<&str>) -> Self {
        match env {
            Some("shared") => Self::Shared,
            Some("extra") => Self::Extra,
            _ => saved,
        }
    }
}
impl SavedSettings {
    fn validated(mut self) -> Self {
        for value in [&mut self.master, &mut self.ambience, &mut self.effects] {
            *value = (*value).min(100) / STEP * STEP;
        }
        for volume in [&mut self.retail_volumes.sfx, &mut self.retail_volumes.dialog, &mut self.retail_volumes.music] {
            *volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        }
        self
    }
}

/// Player volume settings, saved beside the graphics settings.
#[derive(Resource)]
pub(crate) struct AudioSettings {
    saved: SavedSettings,
    path: PathBuf,
    muted: bool,
    /// Option volumes a mod holds through the retail menus (`sdk.menus.set_value`): in force
    /// over the saved ones, never saved, dropped when the mod stops.
    volume_overrides: RetailVolumeOverrides,
}

/// Runtime-only replacements of the saved option volumes (None = the player's setting).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RetailVolumeOverrides {
    sfx: Option<f32>,
    dialog: Option<f32>,
    music: Option<f32>,
}
impl AudioSettings {
    fn load(config: &crate::config::Config) -> Self {
        let path = config.asset_root.parent().unwrap_or(&config.asset_root).join("settings/audio.json");
        let saved = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<SavedSettings>(&bytes).unwrap_or_else(|e| {
                warn!("Audio settings: {e}");
                SavedSettings::default()
            }),
            Err(_) => SavedSettings::default(),
        }
        .validated();
        Self { saved, path, muted: config.mute, volume_overrides: RetailVolumeOverrides::default() }
    }
    /// Linear master gain (0 when muted).
    pub(crate) fn master(&self) -> f32 {
        if self.muted { 0.0 } else { self.saved.master as f32 / 100.0 }
    }
    /// The non-retail "more audible" world layout (see `SavedSettings::more_audible_world`).
    pub(crate) fn more_audible_world(&self) -> bool {
        self.saved.more_audible_world || std::env::var("SKATE_AUDIO_MORE_AUDIBLE").is_ok_and(|v| v == "1")
    }
    /// Whether mod emitters get their own instances (the default; see `SavedSettings::mod_emitter_slots`).
    pub(crate) fn extra_mod_emitter_slots(&self) -> bool {
        let env = std::env::var("SKATE_AUDIO_MOD_EMITTER_SLOTS").ok();
        ModEmitterSlots::in_force(self.saved.mod_emitter_slots, env.as_deref()) == ModEmitterSlots::Extra
    }
    pub(crate) fn category(&self, category: Category) -> f32 {
        let percent = match category {
            Category::Ambience => self.saved.ambience,
            Category::Effects => self.saved.effects,
        };
        percent as f32 / 100.0
    }
    fn field(&mut self, row: AudioRow) -> &mut u32 {
        match row {
            AudioRow::Master => &mut self.saved.master,
            AudioRow::Ambience => &mut self.saved.ambience,
            AudioRow::Effects => &mut self.saved.effects,
        }
    }
    /// Step a menu row by `direction` (wrapping 0..=100) and save; returns a status line.
    pub(crate) fn adjust(&mut self, row: AudioRow, direction: i32) -> String {
        let steps = (100 / STEP + 1) as i32;
        let value = self.field(row);
        *value = ((*value / STEP) as i32 + direction).rem_euclid(steps) as u32 * STEP;
        self.save()
    }
    fn save(&self) -> String {
        let save = (|| -> Result<(), String> {
            std::fs::create_dir_all(self.path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&self.path, serde_json::to_vec_pretty(&self.saved).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        })();
        match save {
            Ok(()) if self.muted => "Saved (audio is muted by --mute)".into(),
            Ok(()) => "Saved".into(),
            Err(e) => format!("Could not save: {e}"),
        }
    }
    /// Default settings saved to `path` (tests outside this module).
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf) -> Self {
        Self { saved: SavedSettings::default(), path, muted: false, volume_overrides: Default::default() }
    }
    /// One of retail's option volumes (0..1, the settings object's float).
    pub(crate) fn retail_volume(&self, volume: RetailVolume) -> f32 {
        self.saved.retail_volumes.get(volume)
    }
    /// Set one of retail's option volumes (clamped to 0..1, as the menu's step rule keeps it) and save.
    pub(crate) fn set_retail_volume(&mut self, volume: RetailVolume, value: f32) -> String {
        *self.saved.retail_volumes.field(volume) = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
        self.save()
    }
    /// Master.in1..in3 as retail's Master step writes them from the option volumes.
    pub(crate) fn master_inputs(&self) -> [(usize, i32); 3] {
        let mut volumes = self.saved.retail_volumes;
        let o = self.volume_overrides;
        for (field, value) in [(&mut volumes.sfx, o.sfx), (&mut volumes.dialog, o.dialog), (&mut volumes.music, o.music)] {
            if let Some(value) = value {
                *field = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
            }
        }
        volumes.master_inputs()
    }
    /// Hold (`Some`) or drop (`None`) a runtime value for one option volume; nothing is saved.
    pub(crate) fn override_retail_volume(&mut self, volume: RetailVolume, value: Option<f32>) {
        let o = &mut self.volume_overrides;
        *match volume {
            RetailVolume::Sfx => &mut o.sfx,
            RetailVolume::Dialog => &mut o.dialog,
            RetailVolume::Music => &mut o.music,
        } = value;
    }
    pub(crate) fn label(&self, row: AudioRow) -> String {
        let (name, value) = match row {
            AudioRow::Master => ("Master volume", self.saved.master),
            AudioRow::Ambience => ("Ambience volume", self.saved.ambience),
            AudioRow::Effects => ("Effects volume", self.saved.effects),
        };
        if row == AudioRow::Master && self.muted {
            format!("{name:<22}{value}%  (muted)")
        } else {
            format!("{name:<22}{value}%")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioRow {
    Master,
    Ambience,
    Effects,
}

/// The one spatial listener (game and mod audio), following the gameplay camera.
#[derive(Component)]
struct GameAudioListener;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CueSet;

pub(crate) struct GameAudioPlugin;
impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Voices>()
            .init_resource::<skate_events::Cues>()
            .init_resource::<emitters::ReverbZones>()
            .init_resource::<AudioContent>()
            .init_resource::<map_audio::MapAudio>()
            .init_resource::<mod_audio::AudioApi>()
            .init_resource::<mod_voices::ModMix>()
            .init_resource::<mod_voices::ModVoices>()
            .init_resource::<tuning::AudioTuning>()
            .init_resource::<mod_rules::AudioRules>()
            .init_resource::<ambience::State>()
            .init_resource::<seed::AudioSeed>()
            .init_resource::<mixmap_inputs::MixMapInputs>()
            .init_resource::<mod_world::OwnWorldOwners>()
            .init_resource::<mod_world::ModWorld>()
            .init_resource::<crate::world_audio::WorldEmitterStats>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, skate_events::observe.after(crate::app::SimulationSet::Physics))
            .add_systems(
                Update,
                // The pass: inputs and the local player's process, the world / NPC owners' process,
                // the ticks and the local update, then the beds (retail's process / tick / update).
                // The front-end sounds (session marker) after the ticks, once per pass.
                (content::frame, map_audio::update, mod_audio::events_frame, mod_rules::frame, mod_audio::drain, ambience::update, native::mixmap_frame, world_sources::frame, npc_skaters::frame_pre, native::mixmap_tick, ambience::mixmap_output, frontend::frame, mod_world::frame, mod_audio::readback, mod_voices::frame, grain_bed::update, emitters::reverb_zones, native::reverb_frame)
                    .chain()
                    .before(CueSet)
                    .after(crate::app::FrameSet::Animation)
                    .after(crate::ui_audio::UiAudioSet),
            )
            .add_systems(Update, (emitters::update, random_sets::update).in_set(CueSet).after(crate::app::FrameSet::Animation))
            .add_systems(Update, voices::sync.after(CueSet))
            .add_systems(Update, timing::report)
            .add_plugins(native::register)
            .add_plugins(world_sources::register)
            .add_plugins(npc_skaters::register)
            .add_plugins(world_speech::register)
            .add_plugins(world_bridge::register)
            .add_plugins(car_alarm::register)
            .add_systems(
                PostUpdate,
                (apply_global_volume, follow_camera).before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

fn setup(mut commands: Commands, config: Res<crate::config::Config>) {
    commands.insert_resource(AudioSettings::load(&config));
    commands.spawn((GameAudioListener, SpatialListener::new(0.2), Transform::default()));
    match Library::load(&config.asset_root) {
        Ok(library) => commands.insert_resource(library),
        Err(error) => info!("Game audio unavailable (run setup to extract it): {error}"),
    }
}

/// Run `f` on the runtime audio API with the native runtime (if running) and the audio content
/// generation: the mod command handlers' entry (`modding`), the same calls engine systems make.
pub(crate) fn with_api<R>(world: &mut World, f: impl FnOnce(&mut mod_audio::AudioApi, Option<&native::Native>, u64) -> R) -> Option<R> {
    // Handles live as long as the runtime: a hot swap keeps them (`AudioContent::runtime_generation`).
    let generation = world.get_resource::<AudioContent>().map_or(0, |c| c.runtime_generation);
    world.get_resource::<mod_audio::AudioApi>()?;
    Some(world.resource_scope(|world, mut api: Mut<mod_audio::AudioApi>| f(&mut api, world.get_resource::<native::Native>(), generation)))
}

/// A mod stopped, failed or was reloaded: its posts, globals, watches, subscriptions and tuning
/// patches go (the tuning is restored at the next audio pass).
pub(crate) fn clear_mod(world: &mut World, owner: &str) {
    with_api(world, |api, native, generation| api.clear_owner(native, generation, owner));
    if let Some(mut t) = world.get_resource_mut::<tuning::AudioTuning>() {
        t.clear_owner(owner);
    }
    if let Some(mut r) = world.get_resource_mut::<mod_rules::AudioRules>() {
        r.clear_owner(owner);
    }
    if let Some(mut s) = world.get_resource_mut::<seed::AudioSeed>() {
        s.clear_owner(owner);
    }
    if let Some(mut i) = world.get_resource_mut::<mixmap_inputs::MixMapInputs>() {
        i.clear_owner(owner);
    }
}

/// `sdk.audio.set_mixmap_input`: write (or with `None` release) one MixMap input (doc 16 L2).
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_mixmap_input(world: &mut World, owner: &str, slot: &str, object: u32, instance: u32, input: u32, value: Option<mixmap_inputs::InputValue>) -> Result<(), String> {
    world.resource_scope(|world, mut inputs: Mut<mixmap_inputs::MixMapInputs>| {
        let mixmap = world.get_resource::<native::Native>().and_then(|n| n.mixmap.as_ref());
        inputs.set(mixmap, owner, slot, object, instance, input, value)
    })
}

/// `sdk.audio.seed`: seed (or with `None` release) the audio random state (doc 16 L5).
pub(crate) fn set_seed(world: &mut World, owner: &str, seed: Option<u64>) -> Result<(), String> {
    world.get_resource_mut::<seed::AudioSeed>().ok_or("game audio is unavailable")?.set(owner, seed)
}

/// `sdk.audio.rule`: set (or with `None` remove) a mod's rule; a replace / layer rule's WAV is
/// loaded into the mod's native bank now (`load` reads and checks it: the mod system's limits).
pub(crate) fn set_rule(world: &mut World, owner: &str, key: &str, rule: Option<skate_mods::audio_rules::Rule>, load: impl FnOnce(&mut World, &str) -> Result<(), String>) -> Result<(), String> {
    if world.get_resource::<mod_rules::AudioRules>().is_none() {
        return Err("game audio is unavailable".into());
    }
    // The whole rule (match, action, the sound's placement and reach) is checked before its WAV
    // is read into the mod's bank.
    if rule.as_ref().is_some_and(|r| !r.validate()) {
        return Err("audio rule: a rule needs a known match, an action and (replace / layer) a valid play (at / offset / position / falloff)".into());
    }
    if let Some(p) = rule.as_ref().and_then(|r| r.play.as_ref()) {
        load(world, &p.path)?;
    }
    world.resource_mut::<mod_rules::AudioRules>().set_rule(owner, key, rule)
}

/// `sdk.audio.set_tuning`: set (or with `None` restore) a mod's patch of a tuning domain.
pub(crate) fn set_tuning(world: &mut World, owner: &str, domain: &str, patch: Option<serde_json::Value>) -> Result<(), String> {
    if world.get_resource::<tuning::AudioTuning>().is_none() {
        return Err("game audio is unavailable".into());
    }
    world.resource_scope(|world, mut t: Mut<tuning::AudioTuning>| {
        let library = world.get_resource::<Library>().ok_or("game audio is unavailable (no audio install)")?;
        t.set(library, owner, domain, patch)
    })
}

/// `sdk.engine.inspect(key, 'audio_tuning:<domain>[/path]')`: the domain as the game uses it now.
pub(crate) fn tuning_read(world: &World, query: &str) -> serde_json::Value {
    match (world.get_resource::<tuning::AudioTuning>(), world.get_resource::<Library>()) {
        (Some(t), Some(library)) => t.read(library, query),
        _ => serde_json::Value::Null,
    }
}

/// A map change: every mod post is released and every global restored.
pub(crate) fn clear_mods_runtime(world: &mut World) {
    with_api(world, |api, native, generation| api.clear_runtime(native, generation));
}

/// The `audio` (per mod) and `audio_info` snapshot sections.
pub(crate) fn mod_snapshot(world: &World, owner: &str) -> serde_json::Value {
    let generation = world.get_resource::<AudioContent>().map_or(0, |c| c.runtime_generation);
    let mut v = world.get_resource::<mod_audio::AudioApi>().map_or(serde_json::Value::Null, |api| api.snapshot(world.get_resource::<native::Native>(), generation, owner));
    // The MixMap inputs this mod writes (doc 16 L2).
    if let Some(rows) = world.get_resource::<mixmap_inputs::MixMapInputs>().and_then(|i| i.snapshot(owner)) {
        if v.is_null() {
            v = serde_json::json!({});
        }
        v["inputs"] = rows;
    }
    // The tuning fields this mod owns (as of the last audio pass).
    let owned = world.get_resource::<tuning::AudioTuning>().map(|t| t.owned(owner)).unwrap_or_default();
    if !owned.is_empty() {
        if v.is_null() {
            v = serde_json::json!({});
        }
        v["tuning"] = serde_json::json!(owned);
    }
    v
}
pub(crate) fn mod_info(world: &World) -> serde_json::Value {
    let mut v = mod_audio::info(world.get_resource::<native::Native>(), world.get_resource::<AudioContent>(), world.get_resource::<map_audio::MapAudio>());
    // audio/moddability-2: rules in force, native mod voices, where mod emitters get their state.
    v["rules"] = serde_json::json!(world.get_resource::<mod_rules::AudioRules>().map_or(0, mod_rules::AudioRules::count));
    v["native_voices"] = serde_json::json!(world.get_resource::<mod_voices::ModVoices>().map_or(0, |m| m.voice_usage("").1));
    v["native_voices_max"] = serde_json::json!(mod_voices::MAX_NATIVE_VOICES);
    v["mixmap_inputs"] = serde_json::json!(world.get_resource::<mixmap_inputs::MixMapInputs>().map_or(0, mixmap_inputs::MixMapInputs::count));
    v["seed"] = world.get_resource::<seed::AudioSeed>().and_then(|s| s.current()).map_or(serde_json::Value::Null, |(o, n)| serde_json::json!({"owner": o, "seed": n}));
    v["mod_emitter_slots"] = serde_json::json!(if world.get_resource::<AudioSettings>().is_some_and(AudioSettings::extra_mod_emitter_slots) { "extra" } else { "shared" });
    v
}

/// `sdk.engine.inspect('audio_catalog')`.
pub(crate) fn catalog(world: &World) -> serde_json::Value {
    mod_audio::catalog(world.get_resource::<native::Native>(), world.get_resource::<Library>(), world.get_resource::<map_audio::MapAudio>(), world.get_resource::<AudioContent>())
}

/// Mod voices scale by GlobalVolume, so the master volume and --mute apply to them too.
fn apply_global_volume(settings: Res<AudioSettings>, mut global: ResMut<GlobalVolume>) {
    let master = Volume::Linear(settings.master());
    if global.volume != master {
        global.volume = master;
    }
}

fn follow_camera(
    camera: Query<&Transform, (With<crate::camera::GameplayCamera>, Without<GameAudioListener>)>,
    mut listener: Query<&mut Transform, With<GameAudioListener>>,
) {
    if let (Some(camera), Ok(mut listener)) = (camera.iter().next(), listener.single_mut()) {
        *listener = *camera;
    }
}

/// True while game sound must be silent: menu open, replay running.
fn silenced(menu: Option<&crate::graphics_menu::Menu>, replay: &crate::replay::Replay) -> bool {
    menu.is_some_and(|m| m.open) || replay.active
}

#[cfg(test)]
mod tests {
    #[test]
    fn retail_volumes_feed_the_master_inputs_as_sub_824d5160() {
        assert_eq!(RetailVolumes::default().master_inputs(), [(1, 32767), (2, 32767), (3, 32767)], "free-skate capture");
        let v = RetailVolumes { sfx: 0.5, dialog: 0.0, music: 0.9 };
        // trunc(v * 32767.0) in f32: 0.9 -> 29490.3 -> 29490; 0.5 -> 16383.5 -> 16383.
        assert_eq!(v.master_inputs(), [(1, 29490), (2, 16383), (3, 0)], "in1 Music, in2 SFX, in3 Dialog");
        assert_eq!((RetailVolumes::master_input(-0.2), RetailVolumes::master_input(1.5), RetailVolumes::master_input(f32::NAN)), (0, 32767, 0));
        let loaded: SavedSettings = serde_json::from_str(r#"{"retail_volumes":{"music":3.0,"sfx":-1.0}}"#).unwrap();
        assert_eq!(loaded.validated().retail_volumes, RetailVolumes { sfx: 0.0, dialog: 1.0, music: 1.0 });
    }

    use super::*;

    fn settings(master: u32) -> AudioSettings {
        AudioSettings {
            saved: SavedSettings { master, ..SavedSettings::default() },
            path: std::env::temp_dir().join(format!("skate-audio-test-{}-{master}/audio.json", std::process::id())),
            muted: false,
            volume_overrides: RetailVolumeOverrides::default(),
        }
    }

    /// Settings files from before the interim tables were removed still load; their `"interim"` /
    /// `"native"` keys are ignored.
    #[test]
    fn old_settings_files_with_interim_or_native_keys_still_load() {
        let old: SavedSettings = serde_json::from_str(r#"{"master":60,"ambience":75,"effects":75,"native":false}"#).unwrap();
        assert_eq!(old, SavedSettings { master: 60, ambience: 75, effects: 75, ..SavedSettings::default() });
        let opted_out: SavedSettings = serde_json::from_str(r#"{"master":50,"interim":true}"#).unwrap();
        assert_eq!(opted_out, SavedSettings { master: 50, ..SavedSettings::default() });
    }

    #[test]
    fn defaults_are_quiet_and_saved_values_are_bounded() {
        assert_eq!(SavedSettings::default().master, 75);
        let loaded: SavedSettings = serde_json::from_str(r#"{"master":400,"ambience":33,"effects":7}"#).unwrap();
        assert_eq!(loaded.validated(), SavedSettings { master: 100, ambience: 30, effects: 5, ..SavedSettings::default() });
        let more: SavedSettings = serde_json::from_str(r#"{"more_audible_world":true}"#).unwrap();
        assert!(more.more_audible_world && more.master == 75);
    }

    /// Mod emitters get their own instances by default (user decision 2026-10-04); `"shared"` in
    /// the file or the variable selects retail's 5; the variable wins over the file either way; a
    /// file without the key gets the default.
    #[test]
    fn mod_emitter_slots_default_to_extra() {
        use ModEmitterSlots::{Extra, Shared};
        assert_eq!(SavedSettings::default().mod_emitter_slots, Extra);
        let old: SavedSettings = serde_json::from_str(r#"{"master":60}"#).unwrap();
        assert_eq!(old.mod_emitter_slots, Extra, "a file without the key");
        let shared: SavedSettings = serde_json::from_str(r#"{"mod_emitter_slots":"shared"}"#).unwrap();
        let extra: SavedSettings = serde_json::from_str(r#"{"mod_emitter_slots":"extra"}"#).unwrap();
        assert_eq!((shared.mod_emitter_slots, extra.mod_emitter_slots), (Shared, Extra));
        assert_eq!(ModEmitterSlots::in_force(Extra, None), Extra);
        assert_eq!(ModEmitterSlots::in_force(Shared, None), Shared);
        assert_eq!(ModEmitterSlots::in_force(Extra, Some("shared")), Shared);
        assert_eq!(ModEmitterSlots::in_force(Shared, Some("extra")), Extra);
        assert_eq!(ModEmitterSlots::in_force(Shared, Some("1")), Shared, "unknown values are ignored");
        let s = |slots| AudioSettings { saved: SavedSettings { mod_emitter_slots: slots, ..Default::default() }, path: std::env::temp_dir().join("x.json"), muted: false, volume_overrides: Default::default() };
        if std::env::var("SKATE_AUDIO_MOD_EMITTER_SLOTS").is_err() {
            assert!(s(Extra).extra_mod_emitter_slots() && !s(Shared).extra_mod_emitter_slots());
        }
    }

    #[test]
    fn adjust_wraps_in_steps_and_mute_wins() {
        let mut s = settings(95);
        s.adjust(AudioRow::Master, 1);
        assert_eq!(s.saved.master, 100);
        s.adjust(AudioRow::Master, 1);
        assert_eq!(s.saved.master, 0);
        s.adjust(AudioRow::Master, -1);
        assert_eq!(s.saved.master, 100);
        s.muted = true;
        assert_eq!(s.master(), 0.0);
        let _ = std::fs::remove_dir_all(s.path.parent().unwrap());
    }
}
