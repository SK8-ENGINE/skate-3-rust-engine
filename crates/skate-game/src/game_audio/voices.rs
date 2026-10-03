//! Voice pool for game sounds. Same playback model as the mod adapter
//! (modding/audio.rs): Bevy `AudioPlayer` entities whose sink gain and speed are
//! smoothed every frame. Pitch is playback speed, so it also changes duration.
//!
//! Loudness rules (keep them; the user asked for safe test levels):
//! - a requested volume may raise a quiet clip only until the clip's own peak
//!   reaches full scale (and never more than x4); category and master volumes
//!   (both <= 1) then scale it down. No clip plays hotter than full scale x master;
//! - loops and sounds asking for a fade start silent and fade in (>= `MIN_FADE`);
//!   one-shots start at their level on the first sample — impacts/knocks are
//!   25-35 ms long, and a fade or per-frame gain ramp ate their attack (the
//!   user heard pops/landings lose their layering);
//! - at most `MAX_VOICES` voices, `MAX_PER_CLIP` of one sound, and a repeat of
//!   the same sound needs `MIN_REPEAT` seconds.
use super::{AudioSettings, library::Clip};
use bevy::{
    audio::{AudioSinkPlayback, PlaybackMode, SpatialScale, Volume},
    prelude::*,
};

const MAX_VOICES: usize = 32;
const MAX_PER_CLIP: usize = 3;
const MIN_REPEAT: f64 = 0.04;
const MIN_FADE: f32 = 0.01;
/// World metres are scaled by this before Bevy's distance attenuation, which
/// never amplifies (gain = min(1, 1 / distance^2)). 0.1 keeps a skater a few
/// metres from the camera at full level, like the mod adapter's default.
const SPATIAL_SCALE: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Ambience,
    Effects,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Play {
    pub category: Category,
    pub volume: f32,
    pub pitch: f32,
    /// World position for a spatial sound; None plays it unpanned.
    pub position: Option<Vec3>,
    pub looping: bool,
    pub fade_in: f32,
    /// Gain over time after the start, (seconds, relative gain) points with
    /// linear interpolation (retail's per-layer fader curves); None = flat.
    pub envelope: Option<&'static [(f32, f32)]>,
}
#[cfg(test)]
impl Play {
    pub(crate) fn effect(volume: f32, position: Vec3) -> Self {
        Self { category: Category::Effects, volume, pitch: 1.0, position: Some(position), looping: false, fade_in: MIN_FADE, envelope: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VoiceId(u64);

struct Voice {
    id: VoiceId,
    entity: Entity,
    key: std::sync::Arc<str>,
    category: Category,
    looping: bool,
    volume: f32,
    max_volume: f32,
    pitch: f32,
    gain: f32,
    speed: f32,
    position: Option<Vec3>,
    attack: f32,
    attack_elapsed: f32,
    stopping: Option<(f32, f32)>,
    pending: f32,
    envelope: Option<&'static [(f32, f32)]>,
    /// Seconds played (not counting silenced time), for the envelope.
    age: f32,
    channels: u16,
}

/// Relative gain of `envelope` at `age` seconds (linear between points,
/// held before the first and after the last).
fn envelope_at(envelope: Option<&[(f32, f32)]>, age: f32) -> f32 {
    let Some(points) = envelope.filter(|p| !p.is_empty()) else { return 1.0 };
    if age <= points[0].0 {
        return points[0].1;
    }
    for pair in points.windows(2) {
        let ((t0, g0), (t1, g1)) = (pair[0], pair[1]);
        if age <= t1 {
            let span = (t1 - t0).max(1e-6);
            return g0 + (g1 - g0) * (age - t0) / span;
        }
    }
    points[points.len() - 1].1
}

#[derive(Resource, Default)]
pub(crate) struct Voices {
    voices: Vec<Voice>,
    next: u64,
    last_start: std::collections::HashMap<std::sync::Arc<str>, f64>,
    /// Category x master gain per category and the silenced flag, as of the
    /// last `sync`, so one-shots can start at their final level.
    mix: [f32; 2],
    silenced: bool,
    /// Scale on every Bevy voice: 1 normally; `1 / RETAIL_SCALE` while the native player sounds run
    /// (`native::follow_volume`), so the retail-measured layers (zone beds, location sets,
    /// crossfades) sit at their measured level against the native voices (which play at retail
    /// level) instead of the old rolling-anchored ×2.
    pub(crate) scale: f32,
    /// True while the native host runs (`native::follow_volume`): every Bevy voice then gets
    /// [`native_fold_gain`], so it reaches the ears as a native voice of the same per-voice gain
    /// would (Pan2D1 + the output stage's stereo fold) instead of Bevy's own panning.
    pub(crate) native_fold: bool,
    /// The listener as of the last `sync` (world → listener-local transform, ear offsets).
    view: Option<ListenerView>,
}

/// Measured retail level → Bevy-voice level of the measured world layers (the ambience beds):
/// ×2, anchored in 2026-10-01 to the former interim rolling loop (its level 0.15 / retail rolling
/// p90 0.065, rounded down). [`Voices::scale`] takes it back out while the native player runs.
pub(super) const RETAIL_SCALE: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
struct ListenerView {
    to_local: bevy::math::Affine3A,
    ears: (Vec3, Vec3),
}

/// The output stage's stereo fold (`skate_audio::dsp::routes::output_stereo`) of one Pan2D1 row.
fn fold_row(row: &[f32; 6]) -> (f32, f32) {
    use skate_audio::dsp::routes::G707;
    (G707 * row[0] + 0.5 * row[1] + 0.5 * row[3], G707 * row[2] + 0.5 * row[1] + 0.5 * row[4])
}

/// Gain that makes a Bevy voice (the measured world layers) reach the ears with the power a native voice of the
/// same per-voice gain gets through Pan2D1 and the output stage's stereo fold (aems-voice-graph-spec
/// §4.9, §6.3; `examples/pan_fold_probe.rs`).
///
/// - Non-positional: a mono native voice opens at azimuth 0 → centre → 0.5 per ear; a stereo one
///   (the bed graph has no panner) routes L→L, R→R → 0.707 per ear. Bevy plays both at 1 per ear.
/// - Positional (`local` = the source in listener space, Bevy axes: −Z ahead, +X right): native =
///   a mono Pan2D1 at the azimuth, folded; Bevy = rodio 0.20's spatial ear factors (each ear
///   ((d_this − d_other)/gap + 1)/4 + 0.5, capped at 1, times the distance term, which stays: it
///   is the interim layer's own distance model), with the channels summed into one (rodio's
///   `ChannelVolume`). Multichannel sources compare as uncorrelated channels of equal power.
pub(crate) fn native_fold_gain(channels: u16, local: Option<Vec3>, ears: (Vec3, Vec3)) -> f32 {
    use skate_audio::dsp::pan::{ANGLE, DISTANCE, Pan2D};
    let n = usize::from(channels.clamp(1, 2));
    match local {
        None => {
            let mut pan = Pan2D::new(n);
            if n > 1 {
                // The bed graph has no panner: L→L, R→R, which a distance-0 layout reproduces.
                pan.params[DISTANCE] = 0.0;
            }
            let m = pan.matrix();
            let native: f32 = (0..n).map(|r| fold_row(&m[r])).map(|(l, r)| l * l + r * r).sum();
            // Bevy: a mono clip is duplicated to both ears (power 2); stereo plays L, R as they are
            // (power 2 for two channels of unit power).
            (native / 2.0).sqrt()
        }
        Some(p) => {
            let azimuth = p.x.atan2(-p.z).to_degrees();
            let mut pan = Pan2D::new(1);
            pan.params[ANGLE] = azimuth;
            let (l, r) = fold_row(&pan.matrix()[0]);
            let (dl, dr) = (p.distance(ears.0), p.distance(ears.1));
            let gap = ears.0.distance(ears.1).max(1e-6);
            let side = |d_this: f32, d_other: f32| (((d_this - d_other) / gap + 1.0) / 4.0 + 0.5).min(1.0);
            let (bl, br) = (side(dl, dr), side(dr, dl));
            // Rodio sums the channels into one: n uncorrelated channels carry n × the power.
            let bevy = n as f32 * (bl * bl + br * br);
            if bevy <= 0.0 { 1.0 } else { ((l * l + r * r) / bevy).sqrt() }
        }
    }
}

/// Largest volume for a clip whose loudest sample is `peak` (0..1 of full scale).
fn max_volume(peak: f32) -> f32 {
    if peak.is_finite() && peak > 0.0 { (1.0 / peak).clamp(1.0, 4.0) } else { 1.0 }
}
fn clamp_volume(volume: f32, max: f32) -> f32 {
    if volume.is_finite() { volume.clamp(0.0, max) } else { 0.0 }
}
fn clamp_pitch(pitch: f32) -> f32 {
    if pitch.is_finite() { pitch.clamp(0.25, 4.0) } else { 1.0 }
}

impl Voices {
    fn scale_or_one(&self) -> f32 {
        if self.scale > 0.0 && self.scale.is_finite() { self.scale } else { 1.0 }
    }

    /// [`native_fold_gain`] while the native host runs, else 1.
    fn fold_gain(&self, channels: u16, position: Option<Vec3>) -> f32 {
        if !self.native_fold {
            return 1.0;
        }
        match (position, self.view) {
            (None, _) => native_fold_gain(channels, None, (Vec3::ZERO, Vec3::X)),
            (Some(at), Some(view)) => native_fold_gain(channels, Some(view.to_local.transform_point3(at)), view.ears),
            // No listener yet: as if the sound played ahead.
            (Some(_), None) => native_fold_gain(channels, Some(Vec3::NEG_Z), (Vec3::new(-0.1, 0.0, 0.0), Vec3::new(0.1, 0.0, 0.0))),
        }
    }

    /// Whether a new voice for `clip` may start now (`now` = real seconds).
    fn admit(&self, clip: &Clip, now: f64) -> bool {
        let active = self.voices.iter().filter(|v| v.stopping.is_none());
        if self.voices.len() >= MAX_VOICES || active.filter(|v| v.key == clip.key).count() >= MAX_PER_CLIP {
            return false;
        }
        self.last_start.get(&clip.key).is_none_or(|t| now - t >= MIN_REPEAT)
    }

    pub(crate) fn play(&mut self, commands: &mut Commands, clip: &Clip, play: Play, now: f64) -> Option<VoiceId> {
        if !self.admit(clip, now) {
            return None;
        }
        self.last_start.insert(clip.key.clone(), now);
        let speed = clamp_pitch(play.pitch);
        let max = max_volume(clip.peak);
        let volume = clamp_volume(play.volume, max);
        // One-shots start at their final level (no fade, no ramp); loops and
        // faded sounds start silent until `sync` applies the faded-in gain.
        let instant = !play.looping && play.fade_in <= MIN_FADE;
        let fold = self.fold_gain(clip.channels, play.position);
        let initial = if instant {
            volume * envelope_at(play.envelope, 0.0) * self.mix[play.category as usize] * self.scale_or_one() * fold
        } else {
            0.0
        };
        let settings = PlaybackSettings {
            mode: if play.looping { PlaybackMode::Loop } else { PlaybackMode::Once },
            volume: Volume::Linear(initial),
            speed,
            paused: !instant || self.silenced,
            spatial: play.position.is_some(),
            spatial_scale: Some(SpatialScale::new(SPATIAL_SCALE)),
            ..Default::default()
        };
        let transform = Transform::from_translation(play.position.unwrap_or(Vec3::ZERO));
        let entity = commands.spawn((AudioPlayer::new(clip.handle.clone()), settings, transform)).id();
        let id = VoiceId(self.next);
        self.next += 1;
        self.voices.push(Voice {
            id, entity, key: clip.key.clone(), category: play.category, looping: play.looping,
            volume, max_volume: max, pitch: speed, gain: if instant { volume } else { 0.0 }, speed,
            position: play.position, attack: play.fade_in.max(MIN_FADE),
            attack_elapsed: if instant { MIN_FADE } else { 0.0 },
            stopping: None, pending: 0.0, envelope: play.envelope, age: 0.0, channels: clip.channels,
        });
        Some(id)
    }

    /// Change a playing voice's target volume, pitch and position (smoothed).
    pub(crate) fn set(&mut self, id: VoiceId, volume: f32, pitch: f32, position: Option<Vec3>) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id && v.stopping.is_none()) {
            v.volume = clamp_volume(volume, v.max_volume);
            v.pitch = clamp_pitch(pitch);
            if v.position.is_some() {
                v.position = position.or(v.position);
            }
        }
    }

    /// Fade out and remove. Repeated calls never extend the fade.
    pub(crate) fn stop(&mut self, id: VoiceId, fade: f32) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            if v.stopping.is_none() {
                let fade = fade.max(MIN_FADE);
                v.stopping = Some((fade, fade));
            }
        }
    }

    pub(crate) fn playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|v| v.id == id)
    }

    /// Whether any voice still plays `clip` (fading voices included).
    pub(crate) fn uses(&self, clip: &Clip) -> bool {
        self.voices.iter().any(|v| v.key == clip.key)
    }
}

fn set_sink(sink: &mut impl AudioSinkPlayback, volume: f32, speed: f32, paused: bool) -> bool {
    sink.set_volume(Volume::Linear(volume));
    sink.set_speed(speed);
    if paused {
        if !sink.is_paused() { sink.pause(); }
    } else if sink.is_paused() {
        sink.play();
    }
    sink.empty()
}

type SinkQuery<'w, 's> = Query<
    'w, 's,
    (Option<&'static mut AudioSink>, Option<&'static mut SpatialAudioSink>, &'static mut PlaybackSettings, &'static mut Transform),
>;

pub(super) fn sync(
    mut commands: Commands,
    mut voices: ResMut<Voices>,
    mut sinks: SinkQuery,
    settings: Option<Res<AudioSettings>>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    listener: Query<(&GlobalTransform, &SpatialListener), With<super::GameAudioListener>>,
) {
    let _timing = super::timing::scope(&super::timing::VOICES);
    let Some(settings) = settings else { return };
    voices.view = listener.iter().next().map(|(t, l)| ListenerView {
        to_local: t.affine().inverse(),
        ears: (l.left_ear_offset, l.right_ear_offset),
    });
    let dt = time.delta_secs().clamp(0.0, 0.25);
    let silenced = super::silenced(menu.as_deref(), &replay);
    let master = settings.master().clamp(0.0, 1.0);
    for category in [Category::Ambience, Category::Effects] {
        voices.mix[category as usize] = settings.category(category).clamp(0.0, 1.0) * master;
    }
    voices.silenced = silenced;
    let scale = voices.scale_or_one();
    let folds: Vec<f32> = voices.voices.iter().map(|v| voices.fold_gain(v.channels, v.position)).collect();
    let mut finished = Vec::new();
    for (v, fold) in voices.voices.iter_mut().zip(folds) {
        let Ok((sink, spatial, mut playback, mut transform)) = sinks.get_mut(v.entity) else {
            // Not spawned yet (commands from this frame) or despawned elsewhere.
            v.pending += dt;
            if v.pending > 1.0 { finished.push(v.id); }
            continue;
        };
        if let Some(position) = v.position { transform.translation = position; }
        if !silenced { v.attack_elapsed += dt; v.age += dt; }
        let attack = (v.attack_elapsed / v.attack).min(1.0);
        let mut release = 1.0;
        if let Some((remaining, total)) = &mut v.stopping {
            *remaining -= dt;
            release = (*remaining / *total).max(0.0);
            if *remaining <= 0.0 { finished.push(v.id); continue; }
        }
        // Smooths parameter steps only (no dynamics processing).
        v.gain += (v.volume - v.gain) * (1.0 - (-dt / 0.012).exp());
        v.speed += (v.pitch - v.speed) * (1.0 - (-dt / 0.025).exp());
        let category = settings.category(v.category).clamp(0.0, 1.0);
        let envelope = envelope_at(v.envelope, v.age);
        let gain = (v.gain * attack * release * envelope).clamp(0.0, v.max_volume) * category * master * scale * fold;
        let speed = v.speed.clamp(0.25, 4.0);
        // Before Bevy creates the sink, keep its initial settings current.
        playback.volume = Volume::Linear(gain);
        playback.speed = speed;
        playback.paused = silenced;
        let done = if let Some(mut sink) = sink {
            Some(set_sink(&mut *sink, gain, speed, silenced))
        } else if let Some(mut sink) = spatial {
            Some(set_sink(&mut *sink, gain, speed, silenced))
        } else {
            None
        };
        match done {
            Some(true) if !v.looping => finished.push(v.id),
            Some(_) => v.pending = 0.0,
            None => {
                if !silenced { v.pending += dt; }
                if v.pending > 2.0 && !v.looping { finished.push(v.id); }
            }
        }
    }
    voices.voices.retain(|v| {
        let keep = !finished.contains(&v.id);
        if !keep {
            if let Ok(mut entity) = commands.get_entity(v.entity) { entity.despawn(); }
        }
        keep
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(name: &str) -> Clip {
        Clip { handle: Handle::default(), key: name.into(), peak: 1.0, channels: 1 }
    }

    #[test]
    fn interim_voices_fold_like_native_ones() {
        let ears = (Vec3::new(-0.1, 0.0, 0.0), Vec3::new(0.1, 0.0, 0.0));
        // Non-positional: mono → centre (0.5 per ear vs Bevy's 1), stereo → 0.707.
        assert!((native_fold_gain(1, None, ears) - 0.5).abs() < 1e-4);
        assert!((native_fold_gain(2, None, ears) - 0.707).abs() < 1e-3);
        // Ahead: native 0.5 / 0.5, rodio 0.75 / 0.75 → 2/3.
        let ahead = native_fold_gain(1, Some(Vec3::new(0.0, 0.0, -10.0)), ears);
        assert!((ahead - 2.0 / 3.0).abs() < 1e-3, "{ahead}");
        // Behind: native 0.35 per ear (Ls/Rs), rodio still 0.75 → about −6.6 dB.
        let behind = native_fold_gain(1, Some(Vec3::new(0.0, 0.0, 10.0)), ears);
        assert!((20.0 * behind.log10() + 6.6).abs() < 0.3, "{behind}");
        // Side: native 0.72 / 0.26 (L/Ls pair), rodio 1.0 / 0.5 (power 1.25).
        let side = native_fold_gain(1, Some(Vec3::new(10.0, 0.0, 0.0)), ears);
        assert!(side > 0.6 && side < 0.75, "{side}");
        // Off by default: Bevy voices are unchanged without the native host.
        assert_eq!(Voices::default().fold_gain(1, None), 1.0);
    }

    #[test]
    fn one_shots_start_at_their_level_and_loops_fade_in() {
        let mut world = World::new();
        let mut voices = Voices { mix: [0.5, 0.25], ..Default::default() };
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let shot = voices.play(&mut commands, &clip("knock"), Play::effect(0.8, Vec3::ZERO), 0.0).unwrap();
        let mut looped = Play::effect(0.8, Vec3::ZERO);
        looped.looping = true;
        let lp = voices.play(&mut commands, &clip("loop"), looped, 0.0).unwrap();
        queue.apply(&mut world);
        let level = |id| voices.voices.iter().find(|v| v.id == id).map(|v| (v.gain, v.entity)).unwrap();
        let (gain, entity) = level(shot);
        let settings = world.get::<PlaybackSettings>(entity).unwrap();
        assert_eq!(gain, 0.8);
        assert_eq!(settings.volume, Volume::Linear(0.8 * 0.25));
        assert!(!settings.paused);
        let (gain, entity) = level(lp);
        assert_eq!(gain, 0.0);
        assert!(world.get::<PlaybackSettings>(entity).unwrap().paused);
    }

    #[test]
    fn requested_levels_are_clamped() {
        assert_eq!(clamp_volume(3.0, 1.0), 1.0);
        assert_eq!(clamp_volume(-1.0, 1.0), 0.0);
        assert_eq!(clamp_volume(f32::NAN, 1.0), 0.0);
        // A clip peaking at -12 dBFS may be raised x~4 (to full scale), never more.
        assert_eq!(max_volume(0.25), 4.0);
        assert_eq!(max_volume(0.5), 2.0);
        assert_eq!(max_volume(0.1), 4.0);
        assert_eq!(max_volume(1.0), 1.0);
        assert_eq!(max_volume(0.0), 1.0);
        assert_eq!(clamp_pitch(f32::INFINITY), 1.0);
    }

    #[test]
    fn limits_voices_per_clip_and_repeats() {
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut voices = Voices::default();
        let pop = clip("pop");
        let play = Play::effect(2.0, Vec3::ZERO);
        let first = voices.play(&mut commands, &pop, play, 0.0).unwrap();
        assert_eq!(voices.voices[0].volume, 1.0, "volume clamped");
        assert!(voices.play(&mut commands, &pop, play, 0.01).is_none(), "repeat too soon");
        assert!(voices.play(&mut commands, &pop, play, 0.05).is_some());
        assert!(voices.play(&mut commands, &pop, play, 0.10).is_some());
        assert!(voices.play(&mut commands, &pop, play, 0.20).is_none(), "three of one sound at most");
        voices.stop(first, 0.1);
        voices.stop(first, 5.0);
        assert_eq!(voices.voices[0].stopping, Some((0.1, 0.1)));
        assert!(voices.play(&mut commands, &pop, play, 0.30).is_some(), "fading voices do not count");
        for i in 0..64 {
            voices.play(&mut commands, &clip(&format!("c{i}")), play, 1.0);
        }
        assert_eq!(voices.voices.len(), MAX_VOICES);
        queue.apply(&mut world);
    }
}
