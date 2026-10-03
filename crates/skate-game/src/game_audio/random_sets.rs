//! Random distant one-shots — police sirens, horns, dogs, bangs, jets — from the
//! location sets (`aud_wp_emitters`) setup exports into the audio manifest.
//!
//! Retail's scheduler (TU3 EmitterSystem; notes ems-emitters-re.md, docs 11), confirmed by a
//! recomp trace at the Aletown spawn (set `e_dwtn_spillway_brewery`, a sound every 4–8 s):
//! - the current location selects one set; a change rebuilds its entries and draws an interval;
//! - at most `LOADED` banks are loaded at once, each picked by weight among unloaded entries;
//! - a timer runs; past the interval the loaded idle entry loaded longest ago plays, the timer
//!   resets and a new interval is drawn (uniform min..max, 1 ms steps) even if nothing played;
//! - a post plays at volume x L, L = (min x 10 + rand % ((max-min) x 10)) / 10, from a random
//!   fixed direction (not positional), and ends with its program or after its timeout; its bank
//!   then unloads and another weighted pick loads.
//! What a post plays (layers, delays, pan sweep) is the bank's program, measured per bank in
//! `random_programs.rs` until the AEMS evaluator is ported.
//!
//! Location -> set: the district's world-painter region layer `audio_emitters` (128 m tiles of
//! quadtrees in the `cSim_*.xsf` streams; TU3 sub_827A2E88 -> sub_82C0EAC0 at the focused
//! skater's x, z). Verified against the recomp at the Aletown spawn and University start.
//! `SKATE_AUDIO_SET=<set name>` forces a set for testing.
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use bevy::prelude::*;

/// Banks loaded at once (EmitterSystem +56).
const LOADED: usize = 2;
/// Directional voices sit this far from the listener (Bevy attenuation stays 1).
const PAN_DISTANCE: f32 = 8.0;

/// One voice a post opens, measured from the bank's program.
#[derive(Clone, Copy, Debug)]
pub(super) struct Layer {
    pub delay: f32,
    /// Sample index, or `SHUFFLE` for the bank's shuffle draw shared by the post.
    pub sample: usize,
    pub level: f32,
    /// Degrees per second the voice's direction turns.
    pub pan_sweep: f32,
    pub looping: bool,
}
pub(super) const SHUFFLE: usize = usize::MAX;

fn program(bank: &str) -> &'static [Layer] {
    super::random_programs::PROGRAMS.iter().find(|(b, _)| *b == bank).map_or(&[], |(_, l)| *l)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Unloaded,
    /// Loaded at this time (the oldest loaded plays first).
    Loaded(f64),
    Playing,
}

struct Post {
    entry: usize,
    started: f64,
    until: f64,
    /// Direction in degrees, and per layer: (start time, voice once started, layer).
    angle: f32,
    layers: Vec<(f64, Option<(VoiceId, Clip)>, Layer, usize)>,
}

#[derive(Default)]
pub(super) struct State {
    key: Option<u64>,
    slots: Vec<Slot>,
    timer: f32,
    interval: f32,
    posts: Vec<Post>,
    bags: std::collections::HashMap<String, Vec<usize>>,
    rng: u32,
}

/// Retail's `rand()` stand-in: uniform u32.
fn next(rng: &mut u32) -> u32 {
    if *rng == 0 {
        *rng = 0x2545_f491;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    *rng
}

/// Uniform interval min..max in 1 ms steps (sub_824A1980).
fn interval(min: f32, max: f32, rng: &mut u32) -> f32 {
    min + (max - min) * (next(rng) % 1000) as f32 * 0.001
}

/// Level factor in 0.1 steps from min, max excluded (sub_824A1A20).
fn level(min: f32, max: f32, rng: &mut u32) -> f32 {
    let steps = ((max - min) * 10.0).round().max(0.0) as u32;
    let offset = if steps == 0 { 0 } else { next(rng) % steps };
    ((min * 10.0).round() + offset as f32) / 10.0
}

/// Weighted pick among unloaded entries (`rand() % Σweights`).
fn weighted(weights: &[i32], slots: &[Slot], rng: &mut u32) -> Option<usize> {
    let total: i64 = weights.iter().zip(slots).filter(|(_, s)| **s == Slot::Unloaded).map(|(w, _)| i64::from((*w).max(0))).sum();
    if total <= 0 {
        return None;
    }
    let mut pick = i64::from(next(rng)) % total;
    for (index, (weight, slot)) in weights.iter().zip(slots).enumerate() {
        if *slot != Slot::Unloaded {
            continue;
        }
        pick -= i64::from((*weight).max(0));
        if pick < 0 {
            return Some(index);
        }
    }
    None
}

fn draw(bag: &mut Vec<usize>, count: usize, rng: &mut u32) -> usize {
    if bag.is_empty() {
        *bag = (0..count).collect();
    }
    let at = next(rng) as usize % bag.len();
    bag.swap_remove(at)
}

/// The set for the skater's location (see module docs).
fn selected(library: &Library, district: &str, at: Vec3) -> Option<u64> {
    static FORCED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    if let Some(name) = FORCED.get_or_init(|| std::env::var("SKATE_AUDIO_SET").ok()).as_deref() {
        return library.random_set_named(name).map(|(key, _)| key);
    }
    library.region_key(district, "audio_emitters", at.x, at.z).filter(|key| library.random_set(*key).is_some())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: Local<State>,
    mut commands: Commands,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    cues: Res<super::skate_events::Cues>,
    map: Res<crate::map_transition::CurrentMap>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
) {
    let Some(mut library) = library else { return };
    let state = &mut *state;
    let Ok(ear) = listener.single() else { return };
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs().clamp(0.0, 0.25);
    let mut rng = state.rng;

    // Location change (or silenced: no set): rebuild the entries.
    let district = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
    let key = if super::silenced(menu.as_deref(), &replay) { None } else { selected(&library, district, cues.riding.board) };
    if key != state.key {
        for post in state.posts.drain(..) {
            for (_, voice, ..) in post.layers {
                if let Some((id, _)) = voice {
                    voices.stop(id, 0.3);
                }
            }
        }
        state.key = key;
        state.slots = key.and_then(|k| library.random_set(k)).map_or(Vec::new(), |s| vec![Slot::Unloaded; s.sounds.len()]);
        state.timer = 0.0;
        if let Some(set) = key.and_then(|k| library.random_set(k)) {
            state.interval = interval(set.min_interval, set.max_interval, &mut rng);
            info!("AUDIO_RANDOM set {} ({} sounds, {:.0}–{:.0} s)", set.name.as_deref().unwrap_or("?"), set.sounds.len(),
                set.min_interval, set.max_interval);
        }
    }
    let Some(set) = state.key.and_then(|k| library.random_set(k)).cloned() else {
        state.rng = rng;
        return;
    };

    // Keep LOADED banks loaded.
    let weights: Vec<i32> = set.sounds.iter().map(|s| s.weight).collect();
    while state.slots.iter().filter(|s| **s != Slot::Unloaded).count() < LOADED {
        let Some(pick) = weighted(&weights, &state.slots, &mut rng) else { break };
        state.slots[pick] = Slot::Loaded(now);
    }

    // Timer: past the interval the oldest loaded idle entry plays.
    state.timer += dt;
    if state.timer >= state.interval {
        let oldest = state.slots.iter().enumerate()
            .filter_map(|(i, s)| if let Slot::Loaded(t) = s { Some((i, *t)) } else { None })
            .min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
        if let Some(entry) = oldest {
            let sound = &set.sounds[entry];
            let layers = program(&sound.bank);
            if layers.is_empty() {
                // The bank's program does not answer this sound's selector: retail is silent too.
                state.slots[entry] = Slot::Unloaded;
            } else {
                let gain = sound.volume * level(set.min_level, set.max_level, &mut rng);
                let samples = library.bank_len(&sound.bank);
                let shuffle = if samples > 0 {
                    draw(state.bags.entry(sound.bank.clone()).or_default(), samples, &mut rng)
                } else {
                    0
                };
                let angle = (next(&mut rng) & 0xFFFF) as f32 * 360.0 / 65536.0;
                info!("AUDIO_RANDOM fire {} level {:.2} angle {:.0}", sound.bank, gain, angle);
                state.posts.push(Post {
                    entry, started: now, until: now + f64::from(sound.seconds), angle,
                    layers: layers.iter().map(|l| {
                        let sample = if l.sample == SHUFFLE { shuffle } else { l.sample };
                        (now + f64::from(l.delay), None, Layer { level: l.level * gain, ..*l }, sample)
                    }).collect(),
                });
                state.slots[entry] = Slot::Playing;
            }
        }
        state.timer = 0.0;
        state.interval = interval(set.min_interval, set.max_interval, &mut rng);
    }

    // Run the posts: start due layers, turn their direction, end on timeout or when all finished.
    let (forward, right) = (ear.forward().as_vec3(), ear.right().as_vec3());
    let origin = ear.translation();
    let bank_of = |entry: usize| set.sounds[entry].bank.clone();
    let mut finished = Vec::new();
    for (index, post) in state.posts.iter_mut().enumerate() {
        let elapsed = (now - post.started) as f32;
        let bank = bank_of(post.entry);
        let mut alive = false;
        for (start, voice, layer, sample) in post.layers.iter_mut() {
            let angle = (post.angle + layer.pan_sweep * elapsed).to_radians();
            let at = origin + (forward * angle.cos() + right * angle.sin()) * PAN_DISTANCE;
            match voice {
                None if now >= *start => {
                    if let Some(clip) = library.sample(&mut assets, &bank, *sample) {
                        let play = Play { category: Category::Ambience, volume: layer.level, pitch: 1.0, position: Some(at),
                            looping: layer.looping, fade_in: 0.0, envelope: None };
                        if let Some(id) = voices.play(&mut commands, &clip, play, now) {
                            *voice = Some((id, clip));
                        }
                    }
                    alive = true;
                }
                None => alive = true,
                Some((id, _)) => {
                    if voices.playing(*id) {
                        voices.set(*id, layer.level, 1.0, Some(at));
                        alive = true;
                    }
                }
            }
        }
        if !alive || now >= post.until {
            finished.push(index);
        }
    }
    for index in finished.into_iter().rev() {
        let post = state.posts.remove(index);
        for (_, voice, ..) in post.layers {
            if let Some((id, _)) = voice {
                voices.stop(id, 0.3);
            }
        }
        if let Some(slot) = state.slots.get_mut(post.entry) {
            *slot = Slot::Unloaded;
        }
    }
    state.rng = rng;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_stay_in_range_with_millisecond_steps() {
        let mut rng = 7;
        for _ in 0..1000 {
            let v = interval(4.0, 8.0, &mut rng);
            assert!((4.0..8.0).contains(&v));
            assert!(((v - 4.0) / 0.004).fract().abs() < 1e-3 || ((v - 4.0) / 0.004).fract() > 0.999);
        }
    }

    #[test]
    fn levels_are_tenths_below_the_maximum() {
        let mut rng = 3;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            seen.insert((level(0.2, 1.0, &mut rng) * 10.0).round() as i32);
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), (2..10).collect::<Vec<_>>());
        assert_eq!(level(0.5, 0.5, &mut rng), 0.5);
    }

    #[test]
    fn weighted_pick_skips_loaded_entries_and_follows_weights() {
        let mut rng = 11;
        let slots = [Slot::Loaded(0.0), Slot::Unloaded, Slot::Unloaded];
        let mut counts = [0; 3];
        for _ in 0..3000 {
            counts[weighted(&[100, 1, 3], &slots, &mut rng).unwrap()] += 1;
        }
        assert_eq!(counts[0], 0);
        assert!(counts[2] > counts[1] * 2, "{counts:?}");
        assert_eq!(weighted(&[1], &[Slot::Playing], &mut rng), None);
    }

    #[test]
    fn region_tiles_walk_their_quadtree() {
        use super::super::library::{NO_KEY, RegionTile};
        // A 128 m tile split once: only the +x+z quarter has a key.
        let tile = RegionTile {
            r#box: [576.0, 192.0, 64.0, 64.0],
            nodes: vec![[1, 2, 3, 4, 0], [NO_KEY, 0, 0, 0, NO_KEY], [NO_KEY, 0, 0, 0, NO_KEY], [NO_KEY, 0, 0, 0, NO_KEY], [NO_KEY, 0, 0, 0, 0]],
            keys: vec!["7EE1991E4909C50E".into()],
        };
        assert_eq!(tile.key(600.0, 220.0), Some(0x7EE1_991E_4909_C50E));
        assert_eq!(tile.key(550.0, 220.0), None);
        assert_eq!(tile.key(700.0, 220.0), None);
    }

    #[test]
    fn programs_cover_sirens_and_mark_unanswered_selectors_silent() {
        assert!(!program("Siren_city_8").is_empty());
        assert!(program("Siren_city_6").is_empty());
        assert!(program("Siren_euro_1").len() == 2);
    }
}
