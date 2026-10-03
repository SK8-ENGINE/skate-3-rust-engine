//! Zone ambience beds, as retail plays them (TU3 SFXObj_Ambience; notes ems-emitters-re.md, docs 11):
//! - the zone is the district's world-painter region layer `audio_ambience` at the skater's x, z,
//!   an `aud_wp_ambiences` record naming its bed (ambience.big stream), volume and fade times;
//! - one bed at a time: on a zone change the old bed fades out over the OLD zone's fade-out time and
//!   stops, then the new bed starts and fades in over the NEW zone's fade-in time (linear); a return
//!   to the old zone during the fade-out fades it back in; a change during a fade-in waits for it;
//! - for the whole transition the district's crossfade bank (`Main_Ambience_Crossfade_DT/Ind/Uni`)
//!   plays the group of the zone pair (either order; group 1, level 1.0 if no pair), four looping
//!   voices from fixed directions (`crossfade_groups.rs`); stopped when the fade-in ends.
//! Beds are not positional (retail plays the channels as authored; ours are stereo downmixes).
//! Installs without the zone data (manifest v3) keep the old per-map bed choice (`BEDS`).
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use bevy::prelude::*;

/// Fallback bed level and fade for installs without zone data.
const LEVEL: f32 = 0.6;
const CROSSFADE: f32 = 2.0;
/// The Ambience MixMap's bed level carries a −11 dB base before any ducking (mixmap-spec.md; out0).
/// Verified against retail captures at four zones (dt_open, dt_main, dt_rez, indu_quarry): predicted
/// with the base within 0.6 dB, without it 11 dB too loud (tools/check_bed_level.py). Our levels run at
/// `voices::RETAIL_SCALE` × retail, so the bed gets the same scale to keep retail's balance.
const BED_BASE: f32 = 0.281_838_3; // 10^(-11/20)
/// Directional crossfade voices sit this far from the listener (Bevy attenuation stays 1).
const PAN_DISTANCE: f32 = 8.0;

/// Map name -> bed, used only without zone data (choices by name and ear, not retail data).
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

/// The district's crossfade bank (retail: dist_downtown / dist_industrial / dist_university only).
fn crossfade_bank(district: &str) -> Option<&'static str> {
    match district {
        "DownTown" => Some("Main_Ambience_Crossfade_DT"),
        "Industrial" => Some("Main_Ambience_Crossfade_Ind"),
        "University" => Some("Main_Ambience_Crossfade_Uni"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Silent,
    FadeIn,
    Steady,
    FadeOut,
}

/// Attenuation 0 (full) .. 1 (silent) of the bed for a phase and its timer (retail's MixMap input 0).
fn attenuation(phase: Phase, t: f32, fade_in: f32, fade_out: f32) -> f32 {
    match phase {
        Phase::Silent => 1.0,
        Phase::Steady => 0.0,
        Phase::FadeIn => 1.0 - (t / fade_in.max(1e-3)).clamp(0.0, 1.0),
        Phase::FadeOut => (t / fade_out.max(1e-3)).clamp(0.0, 1.0),
    }
}

#[derive(Default)]
pub(super) struct State {
    map: Option<(String, u64)>,
    /// Zone of the bed that plays (retail obj+48), its phase and timer.
    current: u64,
    phase: Option<Phase>,
    t: f32,
    bed: Option<(VoiceId, Clip)>,
    crossfade: Vec<(VoiceId, Clip, f32, f32)>,
    crossfade_level: f32,
    fading: Vec<Clip>,
    /// Fallback (no zone data): the per-map bed.
    fallback: Option<(VoiceId, Clip)>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: Local<State>,
    mut commands: Commands,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    cues: Res<super::skate_events::Cues>,
    time: Res<Time<Real>>,
) {
    let Some(mut library) = library else { return };
    let state = &mut *state;
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs().clamp(0.0, 0.25);
    // Free clips whose voices have finished fading.
    let fading = std::mem::take(&mut state.fading);
    for clip in fading {
        if voices.uses(&clip) { state.fading.push(clip); } else { library.release(&mut assets, &clip); }
    }
    let identity = (map.name.clone(), map.generation);
    let new_map = state.map.as_ref() != Some(&identity);
    if new_map {
        state.map = Some(identity);
        // A new map starts from silence: stop everything at once.
        stop_all(state, &mut voices, CROSSFADE);
    }
    if !library.has_zones() {
        if new_map {
            fallback(state, &mut commands, &map.name, &mut library, &mut voices, &mut assets, now);
        }
        return;
    }
    let district = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("").to_owned();
    let at = cues.riding.board;
    let desired = library.region_key(&district, "audio_ambience", at.x, at.z)
        .filter(|key| library.zone(*key).and_then(|z| z.bed.as_ref()).is_some()).unwrap_or(0);

    let phase = state.phase.unwrap_or(Phase::Silent);
    let zone = library.zone(state.current).cloned();
    let (fade_in, fade_out) = zone.as_ref().map_or((1.0, 1.0), |z| (z.time_b, z.time_a));
    match phase {
        Phase::Silent => {
            if desired != 0 {
                let zone = library.zone(desired).cloned();
                if let Some(bed) = zone.as_ref().and_then(|z| z.bed.clone()) {
                    if let Some(clip) = library.ambience(&mut assets, &bed) {
                        let play = Play { category: Category::Ambience, volume: 0.0, pitch: 1.0, position: None,
                            looping: true, fade_in: 0.0, envelope: None };
                        if let Some(id) = voices.play(&mut commands, &clip, play, now) {
                            info!("AUDIO_AMBIENCE zone {} bed {bed}", zone.as_ref().and_then(|z| z.name.as_deref()).unwrap_or("?"));
                            state.bed = Some((id, clip));
                        } else {
                            state.fading.push(clip);
                        }
                    }
                }
                state.current = desired;
                state.phase = Some(Phase::FadeIn);
                state.t = 0.0;
            }
        }
        Phase::FadeIn => {
            state.t += dt;
            if state.t >= fade_in {
                state.phase = Some(Phase::Steady);
                stop_crossfade(state, &mut voices);
            }
        }
        Phase::Steady => {
            if desired != state.current {
                state.phase = Some(Phase::FadeOut);
                state.t = 0.0;
                stop_crossfade(state, &mut voices);
                if desired != 0 {
                    start_crossfade(state, &district, desired, &mut commands, &mut library, &mut voices, &mut assets, now);
                }
            }
        }
        Phase::FadeOut => {
            state.t += dt;
            if desired == state.current {
                // Back into the zone that is fading out: fade the same bed back in.
                state.t = (1.0 - state.t / fade_out.max(1e-3)).clamp(0.0, 1.0) * fade_in;
                state.phase = Some(Phase::FadeIn);
            } else if state.t >= fade_out {
                if let Some((id, clip)) = state.bed.take() {
                    voices.stop(id, 0.05);
                    state.fading.push(clip);
                }
                state.current = 0;
                state.phase = Some(Phase::Silent);
            }
        }
    }

    // Bed level and the crossfade voices' directions.
    let phase = state.phase.unwrap_or(Phase::Silent);
    let zone = library.zone(state.current).cloned();
    if let (Some((id, _)), Some(zone)) = (&state.bed, &zone) {
        let gain = zone.volume * BED_BASE * super::voices::RETAIL_SCALE * (1.0 - attenuation(phase, state.t, zone.time_b, zone.time_a));
        voices.set(*id, gain, 1.0, None);
    }
    if let Ok(ear) = listener.single() {
        let (forward, right, origin) = (ear.forward().as_vec3(), ear.right().as_vec3(), ear.translation());
        let level = state.crossfade_level;
        for (id, _, pan, voice_level) in &state.crossfade {
            let angle = pan.to_radians();
            let at = origin + (forward * angle.cos() + right * angle.sin()) * PAN_DISTANCE;
            voices.set(*id, voice_level * level, 1.0, Some(at));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_crossfade(
    state: &mut State, district: &str, to: u64, commands: &mut Commands, library: &mut Library,
    voices: &mut Voices, assets: &mut Assets<AudioSource>, now: f64,
) {
    let Some(bank) = crossfade_bank(district) else { return };
    let (group, level) = library.crossfade(state.current, to).map_or((1, 1.0), |c| (c.group, c.level));
    let Some((_, _, layout)) = super::crossfade_groups::GROUPS.iter().find(|(b, g, _)| *b == bank && *g == group) else {
        return;
    };
    // w0 = clamp(MixMap out1 x level); out1 not decoded yet, taken as full scale.
    state.crossfade_level = level.min(1.0);
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
    state.current = 0;
    state.phase = None;
    state.t = 0.0;
}

#[allow(clippy::too_many_arguments)]
fn fallback(
    state: &mut State, commands: &mut Commands, map: &str, library: &mut Library, voices: &mut Voices,
    assets: &mut Assets<AudioSource>, now: f64,
) {
    let Some(bed) = bed_for(map) else {
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

    #[test]
    fn fades_are_linear_and_one_bed_at_a_time() {
        assert_eq!(attenuation(Phase::Silent, 0.0, 2.0, 3.0), 1.0);
        assert_eq!(attenuation(Phase::Steady, 9.0, 2.0, 3.0), 0.0);
        assert!((attenuation(Phase::FadeIn, 1.0, 2.0, 3.0) - 0.5).abs() < 1e-6);
        assert!((attenuation(Phase::FadeOut, 1.5, 2.0, 3.0) - 0.5).abs() < 1e-6);
        assert_eq!(attenuation(Phase::FadeOut, 5.0, 2.0, 3.0), 1.0);
    }

    #[test]
    fn crossfade_banks_exist_only_for_the_three_districts() {
        assert_eq!(crossfade_bank("DownTown"), Some("Main_Ambience_Crossfade_DT"));
        assert_eq!(crossfade_bank("MegaPark"), None);
        for bank in ["Main_Ambience_Crossfade_DT", "Main_Ambience_Crossfade_Ind", "Main_Ambience_Crossfade_Uni"] {
            assert!(super::super::crossfade_groups::GROUPS.iter().any(|(b, g, _)| *b == bank && *g == 1), "{bank}");
        }
    }
}
