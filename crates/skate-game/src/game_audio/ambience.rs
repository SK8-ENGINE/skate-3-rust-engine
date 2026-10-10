//! Native TU3 zone-bed phases and MixMap controls, with interim Bevy playback.
//! The controller processes before the MixMap tick; gains/pitch read the actual outputs afterward.
//! Beds still use exported stereo downmixes and Bevy voices; their multichannel/filter/release
//! graph is not a 1:1 reproduction. Crossfade layouts come from the bank or mod declarations.
//! Installs without zone data retain the explicitly approximate per-map fallback below.
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use bevy::prelude::*;

/// Fallback bed level and fade for installs without zone data.
const LEVEL: f32 = 0.6;
const CROSSFADE: f32 = 2.0;
/// Directional crossfade voices sit this far from the listener (Bevy attenuation stays 1).
const PAN_DISTANCE: f32 = 8.0;

/// Map name -> bed, used only without zone data (choices by name and ear, not retail data; a map's
/// audio definition can name its own, `map_audio::MapAudio::fallback_bed`).
const BEDS: &[(&str, &str)] = &[
    ("University", "09_univ_campus"),
    ("StartPark", "10_univ_housing"),
    ("DownTown", "04_dt_main"),
    ("DownTownSkatePark", "05_dt_parks"),
    ("MegaPark", "05_dt_parks"),
    ("Industrial", "11_indu_shipyard"),
    ("IndustrialSkatePark", "20_indu_old_factory"),
    ("SkateSchool", "18_skate_school"),
    ("MaloofMoneyCup", "21_interior_arena_amb"),
    ("BlackBoxPark", "22_interior_tunnel_amb"),
];

pub(super) fn bed_for(map: &str) -> Option<&'static str> {
    BEDS.iter().find(|(name, _)| name.eq_ignore_ascii_case(map)).map(|(_, bed)| *bed)
}


#[derive(Resource, Default)]
pub(super) struct State {
    /// Map name, map generation, audio content generation.
    map: Option<(String, u64, u64)>,
    control: skate_audio::world::ambience::Controller,
    bed: Option<(VoiceId, Clip)>,
    crossfade: Vec<(VoiceId, Clip, f32, f32)>,
    crossfade_level: f32,
    fading: Vec<Clip>,
    /// Fallback (no zone data): the per-map bed.
    fallback: Option<(VoiceId, Clip)>,
    /// The zone the skater was last in (audio events: `zone_change`).
    zone_seen: u64,
    /// The map's crossfade bank, the content generation its layouts were built for, and the
    /// layouts (built when either changes, i.e. at map load or an audio content restart).
    layouts: Option<(Option<String>, u64, super::crossfade_layouts::Layouts)>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: ResMut<State>,
    mut commands: Commands,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    cues: Res<super::skate_events::Cues>,
    time: Res<Time<Real>>,
    virtual_time: Res<Time<Virtual>>,
    native: Option<ResMut<super::native::Native>>,
    content: Res<super::AudioContent>,
    audio: Res<super::map_audio::MapAudio>,
    mut api: ResMut<super::mod_audio::AudioApi>,
) {
    let Some(mut library) = library else { return };
    let state = &mut *state;
    let now = time.elapsed_secs_f64();
    let dt = virtual_time.delta_secs();
    // Free clips whose voices have finished fading.
    let fading = std::mem::take(&mut state.fading);
    for clip in fading {
        if voices.uses(&clip) { state.fading.push(clip); } else { library.release(&mut assets, &clip); }
    }
    let identity = (map.name.clone(), map.generation, content.world_generation);
    let new_map = state.map.as_ref() != Some(&identity);
    if new_map {
        state.map = Some(identity);
        // A new map starts from silence: stop everything at once.
        stop_all(state, &mut voices, CROSSFADE);
    }
    if !library.has_zones() {
        if new_map {
            let bed = audio.fallback_bed.clone().or_else(|| bed_for(&map.name).map(str::to_owned));
            fallback(state, &mut commands, &map.name, bed.as_deref(), &mut library, &mut voices, &mut assets, now);
        }
        return;
    }
    let bank = audio.crossfade_bank.clone();
    if state.layouts.as_ref().is_none_or(|(b, g, _)| *b != bank || *g != content.generation) {
        let (layouts, why) = match &bank {
            Some(bank) => super::crossfade_layouts::build(&library, bank),
            None => (super::crossfade_layouts::Layouts::empty(), None),
        };
        if let Some(name) = &bank {
            match why {
                None => info!("AUDIO_AMBIENCE crossfade layouts {name}: {:?}, groups {:?}", layouts.source, layouts.groups().collect::<Vec<_>>()),
                Some(why) => warn!("AUDIO_AMBIENCE crossfade bank {name} has no layout ({why}): its crossfades are silent"),
            }
        }
        state.layouts = Some((bank, content.generation, layouts));
    }
    let at = cues.riding.board;
    let desired = audio.region_key(&library, "audio_ambience", at.x, at.z)
        .filter(|key| library.zone(*key).and_then(|z| z.bed.as_ref()).is_some()).unwrap_or(0);
    if desired != state.zone_seen {
        state.zone_seen = desired;
        api.events.push(super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Zone, source: super::mod_audio::Source::Ambience, class: "", slot: "", id: 0, owner: desired, hit: None });
    }

    let zone = library.zone(state.control.current).cloned();
    let (fade_in, fade_out) = zone.as_ref().map_or((1.0, 1.0), |z| (z.time_b, z.time_a));
    let ready = state.layouts.as_ref().is_some_and(|(bank, _, _)| bank.is_some());
    let change = state.control.process(desired, dt, fade_in, fade_out, ready);
    if change.stop_crossfade { stop_crossfade(state, &mut voices); }
    if change.start_crossfade {
        let layouts = state.layouts.take();
        if let Some((Some(bank), _, layout)) = &layouts {
            start_crossfade(state, bank, layout, desired, &mut commands, &mut library, &mut voices, &mut assets, now);
        }
        state.layouts = layouts;
    }
    if change.stop_bed {
        if let Some((id, clip)) = state.bed.take() {
            voices.stop(id, 0.05);
            state.fading.push(clip);
        }
    }
    if change.start_bed {
        let zone = library.zone(state.control.current).cloned();
        if let Some(bed) = zone.as_ref().and_then(|z| z.bed.clone()) {
            if let Some(clip) = library.ambience(&mut assets, &bed) {
                let play = Play { category: Category::Ambience, volume: 0.0, pitch: 1.0, position: None,
                    looping: true, fade_in: 0.0, envelope: None };
                if let Some(id) = voices.play(&mut commands, &clip, play, now) {
                    info!("AUDIO_AMBIENCE zone {} bed {bed}", zone.as_ref().and_then(|z| z.name.as_deref()).unwrap_or("?"));
                    state.bed = Some((id, clip));
                } else { state.fading.push(clip); }
            }
        }
    }
    let zone = library.zone(state.control.current);
    let (fade_in, fade_out) = zone.map_or((1.0, 1.0), |z| (z.time_b, z.time_a));
    if let Some(mut native) = native {
        if let Some(mixmap) = native.mixmap.as_mut() {
            mixmap.set_input(skate_audio::mixmap::keys::AMBIENCE, 0, state.control.input(fade_in, fade_out));
        }
    }
}

/// Native 824D42C8 reads outputs after the MixMap tick. Bevy playback remains an interim host;
/// its downmix, filter graph and release behavior are not certified by the phase comparison.
pub(super) fn mixmap_output(
    state: Res<State>, library: Option<Res<Library>>, mut voices: ResMut<Voices>,
    native: Option<Res<super::native::Native>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
) {
    let (Some(library), Some(native)) = (library, native) else { return };
    let Some(mixmap) = native.mixmap.as_ref() else { return };
    let key = skate_audio::mixmap::keys::AMBIENCE;
    if let (Some((id, _)), Some(zone)) = (&state.bed, library.zone(state.control.current)) {
        let gain = (mixmap.level(key, 0) as f32 * skate_audio::dsp::INV_32767) * zone.volume;
        let pitch = mixmap.pitch_4096(key, 2) as f32 / 4096.0;
        voices.set(*id, gain * super::voices::RETAIL_SCALE, pitch, None);
    }
    if let Ok(ear) = listener.single() {
        let (forward, right, origin) = (ear.forward().as_vec3(), ear.right().as_vec3(), ear.translation());
        let word = ((mixmap.level(key, 1) as f32 * state.crossfade_level) as i32).clamp(0, 32767);
        let level = word as f32 * skate_audio::dsp::INV_32767;
        for (id, _, pan, voice_level) in &state.crossfade {
            let angle = pan.to_radians();
            let at = origin + (forward * angle.cos() + right * angle.sin()) * PAN_DISTANCE;
            voices.set(*id, voice_level * level, 1.0, Some(at));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_crossfade(
    state: &mut State, bank: &str, layouts: &super::crossfade_layouts::Layouts, to: u64, commands: &mut Commands,
    library: &mut Library, voices: &mut Voices, assets: &mut Assets<AudioSource>, now: f64,
) {
    let (group, level) = library.crossfade(state.control.current, to).map_or((1, 1.0), |c| (c.group, c.level));
    let Some(layout) = layouts.group(group) else {
        return;
    };
    // The native packet level is quantized from MixMap out1 in mixmap_output.
    state.crossfade_level = level;
    for (sample, pan, voice_level) in layout {
        if let Some(clip) = library.sample(assets, bank, *sample) {
            let play = Play { category: Category::Ambience, volume: 0.0, pitch: 1.0, position: Some(Vec3::ZERO),
                looping: true, fade_in: 0.0, envelope: None };
            if let Some(id) = voices.play(commands, &clip, play, now) {
                state.crossfade.push((id, clip, *pan, *voice_level));
            }
        }
    }
    info!("AUDIO_AMBIENCE crossfade {bank} group {group} level {level:.2}");
}

fn stop_crossfade(state: &mut State, voices: &mut Voices) {
    for (id, clip, ..) in state.crossfade.drain(..) {
        voices.stop(id, 0.05);
        state.fading.push(clip);
    }
}

fn stop_all(state: &mut State, voices: &mut Voices, fade: f32) {
    stop_crossfade(state, voices);
    for (id, clip) in state.bed.take().into_iter().chain(state.fallback.take()) {
        voices.stop(id, fade);
        state.fading.push(clip);
    }
    state.control = Default::default();
}

#[allow(clippy::too_many_arguments)]
fn fallback(
    state: &mut State, commands: &mut Commands, map: &str, bed: Option<&str>, library: &mut Library, voices: &mut Voices,
    assets: &mut Assets<AudioSource>, now: f64,
) {
    let Some(bed) = bed else {
        info!("Ambience: none for map {map:?}");
        return;
    };
    let Some(clip) = library.ambience(assets, bed) else { return };
    let play = Play { category: Category::Ambience, volume: LEVEL, pitch: 1.0, position: None, looping: true, fade_in: CROSSFADE, envelope: None };
    match voices.play(commands, &clip, play, now) {
        Some(voice) => {
            info!("Ambience: {bed} for map {map:?} (no zone data; run setup)");
            state.fallback = Some((voice, clip));
        }
        None => state.fading.push(clip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_installed_map_has_a_fallback_bed() {
        for map in ["University", "BlackBoxPark", "DownTown", "DownTownSkatePark", "Industrial",
                    "IndustrialSkatePark", "MaloofMoneyCup", "MegaPark", "SkateSchool", "StartPark"] {
            assert!(bed_for(map).is_some(), "{map}");
        }
        assert_eq!(bed_for("Test world"), None);
    }

    /// A mod crossfade bank plays: an overlay adds a WAV bank with a declared layout, a zone pair
    /// naming its group and a map whose crossfade bank it is; walking from one zone into the
    /// other starts the declared voices (the mod's samples) for the transition and stops them
    /// when the new bed has faded in.
    #[test]
    fn a_mod_crossfade_bank_plays_its_layout() {
        use super::super::library::{OverlaySource, tests::{content_fixture, test_wav}};
        let (dir, mods) = content_fixture("ambience-crossfade");
        let manifest = dir.join("private/audio/audio_manifest.json");
        let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        m["zones"]["00000000000000CD"] = serde_json::json!({"name": "street", "bed": "bed"});
        std::fs::write(&manifest, m.to_string()).unwrap();
        std::fs::write(mods.join("audio/f0.wav"), test_wav(4800, 48000, 11)).unwrap();
        std::fs::write(mods.join("audio/f1.wav"), test_wav(4800, 48000, 22)).unwrap();
        let o: skate_mods::audio_content::AudioOverlay = serde_json::from_value(serde_json::json!({"version": 1,
            "add": {
                "banks": {"MOD_fade": {"samples": ["audio/f0.wav", "audio/f1.wav"], "group": "world"}},
                "crossfades": [{"from": "00000000000000AB", "to": "00000000000000CD", "group": 2}],
                "crossfade_layouts": {"MOD_fade": {"2": [{"sample": 1, "pan": 45}, {"sample": 0, "pan": 225, "level": 0.7}]}}
            },
            "maps": {"MyMap": {"crossfade_bank": "MOD_fade", "regions": {"audio_ambience": [
                {"box": [-50, 0, 50, 50], "key": "plaza"}, {"box": [50, 0, 50, 50], "key": "street"}]}}}
        })).unwrap();
        o.validate().unwrap();
        let (library, report) = Library::load_with(&dir, &[OverlaySource { id: "dev.a", root: &mods, overlay: &o }]).unwrap();
        assert!(report.warnings.is_empty() && report.conflicts.is_empty() && report.rejected.is_empty(), "{report:?}");
        let audio = super::super::map_audio::build("MyMap", None, &library, Some(&std::collections::HashMap::new()));
        assert_eq!(audio.crossfade_bank.as_deref(), Some("MOD_fade"));
        let mut world = World::new();
        let mut assets = Assets::<AudioSource>::default();
        let mut probe = Library::load_with(&dir, &[OverlaySource { id: "dev.a", root: &mods, overlay: &o }]).unwrap().0;
        let (f0, f1) = (probe.sample(&mut assets, "MOD_fade", 0).unwrap(), probe.sample(&mut assets, "MOD_fade", 1).unwrap());
        world.insert_resource(library);
        world.insert_resource(assets);
        world.insert_resource(Voices::default());
        world.insert_resource(crate::map_transition::CurrentMap { path: None, name: "MyMap".into(), spawn: [0.0; 3], heading: 0.0, generation: 1, audio_tag: None });
        world.insert_resource(super::super::skate_events::Cues::default());
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(Time::<Virtual>::default());
        world.insert_resource(State::default());
        world.insert_resource(super::super::AudioContent::default());
        world.insert_resource(audio);
        world.insert_resource(super::super::mod_audio::AudioApi::default());
        world.spawn((GlobalTransform::default(), super::super::GameAudioListener));
        // One persistent ambience resource for the whole walk.
        let system = world.register_system(update);
        let start = std::time::Instant::now();
        let step = |world: &mut World, x: f32, t: f32| {
            world.resource_mut::<super::super::skate_events::Cues>().riding.board = Vec3::new(x, 0.0, 0.0);
            world.resource_mut::<Time<Real>>().update_with_instant(start + std::time::Duration::from_secs_f32(t));
            world.resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs_f32(0.1));
            world.run_system(system).unwrap();
        };
        // In the plaza until its bed has faded in (1 s), then across into the street.
        for i in 0..15 {
            step(&mut world, -10.0, i as f32 * 0.1);
        }
        assert!(!world.resource::<Voices>().uses(&f0) && !world.resource::<Voices>().uses(&f1), "no crossfade inside one zone");
        step(&mut world, 10.0, 1.6);
        assert!(world.resource::<Voices>().sounds(&f0) && world.resource::<Voices>().sounds(&f1), "the declared group plays the mod's samples");
        // The old bed fades out (1 s), the new one fades in (1 s): then the crossfade stops.
        for i in 0..30 {
            step(&mut world, 10.0, 1.7 + i as f32 * 0.1);
        }
        assert!(!world.resource::<Voices>().sounds(&f0) && !world.resource::<Voices>().sounds(&f1), "stopped when the fade-in ended");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn crossfade_banks_exist_only_for_the_three_districts() {
        use super::super::map_audio::tests::old_crossfade_bank;
        assert_eq!(old_crossfade_bank("DownTown"), Some("Main_Ambience_Crossfade_DT"));
        assert_eq!(old_crossfade_bank("MegaPark"), None);
        for bank in ["Main_Ambience_Crossfade_DT", "Main_Ambience_Crossfade_Ind", "Main_Ambience_Crossfade_Uni"] {
            assert!(super::super::crossfade_groups::GROUPS.iter().any(|(b, g, _)| *b == bank && *g == 1), "{bank}");
        }
    }
}
