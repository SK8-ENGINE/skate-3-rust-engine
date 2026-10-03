//! The voice graph and the default bus (spec: `.claude/notes/aems-voice-graph-spec.md` §3–§6).
//!
//! Per voice: SndPlayer1 (PCM source with loop, end, first-block format change, 16-frame stop
//! fade) → Rechannel (no-op: our PCM always has the header's channel count) → Resample → HighPassIir2
//! → LowPassIir2 → Send A (pre-gain, N → 1 into the environment bus at master × FXWET0) → Gain
//! (master × dry) → [Send B: N → 1 into a FlangeSub return at property 11, when the routing
//! records ask for one] → Pan2D1 (6 outputs) → Send (6 → 6) into its output bus: an eEQChain bus
//! (routing codes 0–7, or 10–17 = re-roll the bus's EQ on first use) or SFX Master
//! ([`crate::bus`]). Direct (Splice / stream) voices take a [`Route`] instead: their bus and the
//! env send of the owner one-shot bus they play through.
//!
//! Not modelled (UNCERTAIN in the spec): the 512/2048 buses — their codes are parsed and kept on
//! the voice. Send B renders only while the returns are enabled ([`crate::bus::flange`]).
//! The sample-group "level %" byte is not applied as gain (spec §3.4: its meaning is unproven).
use std::collections::HashMap;
use std::sync::Arc;

use crate::dsp::biquad::{Iir2, Kind};
use crate::dsp::gain::Gain;
use crate::dsp::pan::{self, Pan2D};
use crate::dsp::resample::{Resampler, ratio};
use crate::dsp::routes::to_six;
use crate::dsp::send::{Mode, Send, fold_release};
use crate::dsp::{DEGREES_PER_UNIT, INV_32767};
use crate::bus::env::Level;
use crate::bus::{Buses, Output, Route};
use crate::eval::{OpenRequest, VoiceHost, VoiceStatus};
use crate::formats::SampleHeader;
use crate::{BLOCK, MIX_RATE};

/// Decoded sample: planar f32 channels at the header's rate.
#[derive(Clone, Debug)]
pub struct Pcm {
    pub rate: u32,
    pub channels: Vec<Vec<f32>>,
}

/// Routing selected by a voice's input records with id ≥ 9 at open (evaluator spec §5.4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Routing {
    /// eEQChain bus 0..7 and whether resolving it re-rolls its EQ (codes 0–7: no; 10–17: yes).
    pub output_bus: Option<(u8, bool)>,
    /// Effect return (codes 4096/8192 → 0, 16384 → 1) and its level.
    pub effect: Option<(u8, f32)>,
    /// Fixed buses (codes 512 / 2048).
    pub fixed: Option<u16>,
}

impl Routing {
    pub fn parse(inputs: &[(u8, i32)]) -> Self {
        let mut r = Self::default();
        let mut k = 0;
        while k < inputs.len() {
            let (id, v) = inputs[k];
            k += 1;
            if id < 9 {
                continue;
            }
            match v {
                4096 | 8192 | 16384 => {
                    let enable = inputs.get(k).map(|e| e.1);
                    let level = inputs.get(k + 1).map(|e| e.1);
                    k += 2;
                    if enable == Some(1) {
                        let bus = u8::from(v == 16384);
                        r.effect = Some((bus, level.unwrap_or(0) as f32 * INV_32767));
                    }
                }
                512 | 2048 => r.fixed = Some(v as u16),
                0..=7 => r.output_bus = Some((v as u8, false)),
                10..=17 => r.output_bus = Some(((v - 10) as u8, true)),
                _ => {}
            }
        }
        r
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Playing,
    /// Fading out for the pause, then frozen.
    Pausing,
    Paused,
    Resuming,
    /// Released while sounding: one 16-frame stop-fade block, then removed.
    Releasing,
}

struct Voice {
    id: u32,
    bank: usize,
    slot: u16,
    header: SampleHeader,
    pcm: Option<Arc<Pcm>>,
    started: bool,
    state: State,
    resampler: Resampler,
    pitch: f32,
    hpf: Iir2,
    lpf: Iir2,
    master: f32,
    dry: f32,
    fx: f32,
    gain: Gain,
    pan: Pan2D,
    send: Send,
    routing: Routing,
    /// The final Send's bus.
    output: Output,
    /// Send A (pre-gain, N → 1 into the environment bus), level master × FXWET0.
    env: Level,
    /// The owner one-shot bus's env send (6 → 1 after the panner), 0 = none.
    owner_env: Level,
    /// Send B (post-gain, N → 1 into the FlangeSub return of `routing.effect`), property 11.
    fx_send: Level,
    /// The output bus is a mono submix ([`Route::mono`]): the final Send sums L..Rs into the
    /// bus's centre.
    mono_out: bool,
    /// The bank's volume group ([`Mixer::group_gain`]).
    group: u8,
    /// Stop fade: (source index where it starts, per-channel last sample).
    fade: Option<(u64, [f32; 8])>,
    done: bool,
}

impl Voice {
    fn total(&self) -> u64 {
        u64::from(self.header.frames)
    }

    /// Source frame index for a virtual (ever-increasing) position, following the loop.
    fn index(&self, i: u64) -> Option<u64> {
        let total = self.total();
        if i < total {
            return Some(i);
        }
        let start = u64::from(self.header.loop_start?);
        if start >= total {
            return None;
        }
        Some(start + (i - total) % (total - start))
    }

    fn sample(&self, ch: usize, i: u64) -> f32 {
        if let Some((from, last)) = self.fade {
            if i >= from {
                let k = i - from;
                return if k < 16 { last[ch] - k as f32 * last[ch] / 16.0 } else { 0.0 };
            }
        }
        match (&self.pcm, self.index(i)) {
            (Some(pcm), Some(idx)) => pcm.channels.get(ch).and_then(|c| c.get(idx as usize)).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Elapsed source time (s): the position inside the sample.
    fn elapsed(&self) -> f64 {
        let at = self.index(self.resampler.position).unwrap_or(self.total());
        at as f64 / f64::from(self.header.rate)
    }

    fn render(&mut self, master: &mut [[f32; BLOCK]; 6], buses: &mut Buses, user: f32) {
        let channels = (self.header.channels as usize).clamp(1, 6);
        let mut src = [[0.0f32; BLOCK]; 6];
        if self.started {
            self.resampler.set_ratio(ratio(self.header.rate, self.pitch));
            for (c, out) in src.iter_mut().enumerate().take(channels) {
                let me = &*self;
                self.resampler.render(out, |i| me.sample(c, i));
            }
            self.resampler.advance(BLOCK);
            if self.header.loop_start.is_none() && self.resampler.position >= self.total() {
                self.done = true;
            }
        }
        // A new player publishes its format with 0 frames: the first block is silent.
        self.started = true;
        // Plane arrays on the stack, not `Vec`s: the render must not allocate (test
        // `tests/render_alloc.rs`).
        {
            let mut planes = src.each_mut().map(|c| &mut c[..]);
            self.hpf.process(&mut planes[..channels], MIX_RATE as f32);
            self.lpf.process(&mut planes[..channels], MIX_RATE as f32);
        }
        // Send A: the channels summed to mono (routes N → 1 at unity, LFE dropped), our user volume
        // applied like on the dry path.
        self.env.target = self.master * self.fx * user;
        if !self.env.silent() {
            let mut mono = [0.0f32; BLOCK];
            for c in src.iter().take(channels.min(5)) {
                for (m, x) in mono.iter_mut().zip(c.iter()) {
                    *m += x;
                }
            }
            self.env.add(&mono, &mut buses.env_in[..]);
        }
        {
            let mut planes = src.each_mut().map(|c| &mut c[..]);
            self.gain.target = self.master * self.dry;
            self.gain.process(&mut planes[..channels]);
        }
        // Send B (`sub_824A3140`: only with an enabled effect record; level posted 0 at open, then
        // property 11 / 32767), our user volume applied like on the other paths.
        if let Some((ret, level)) = self.routing.effect {
            if buses.flange.enabled() {
                let r = usize::from(ret.min(1));
                buses.flange_active[r] = true;
                self.fx_send.target = level * user;
                if !self.fx_send.silent() {
                    let mut mono = [0.0f32; BLOCK];
                    for c in src.iter().take(channels.min(5)) {
                        for (m, x) in mono.iter_mut().zip(c.iter()) {
                            *m += x;
                        }
                    }
                    self.fx_send.add(&mono, &mut buses.flange_in[r][..]);
                }
            }
        }
        let mut six = [[0.0f32; BLOCK]; 6];
        let planes = src.each_ref().map(|c| &c[..]);
        self.pan.process(&planes[..channels], &mut six);
        // Our own user volume of the voice's group (1 = retail level), after the retail graph.
        if user != 1.0 {
            for ch in six.iter_mut() {
                for x in ch.iter_mut() {
                    *x *= user;
                }
            }
        }
        let mode = match self.state {
            State::Pausing => Mode::FadeOut,
            State::Resuming => Mode::FadeIn,
            _ => Mode::Normal,
        };
        if !self.owner_env.silent() {
            let mut mono = [0.0f32; BLOCK];
            for c in six.iter().take(5) {
                for (m, x) in mono.iter_mut().zip(c.iter()) {
                    *m += x;
                }
            }
            self.owner_env.add(&mono, &mut buses.env_in[..]);
        }
        let outs = six.each_ref().map(|c| &c[..]);
        let bus = buses.target(self.output, master);
        let routes = if self.mono_out { SIX_TO_MONO_CENTRE } else { to_six(6) };
        self.send.process(&outs, routes, bus, mode);
        self.state = match self.state {
            State::Pausing => State::Paused,
            State::Resuming => State::Playing,
            s => s,
        };
    }
}

/// A 6-channel send into a mono submix that sends on into its bus's centre: routes 6 → 1 (L, C,
/// R, Ls, Rs at unity, LFE dropped; image tables 0x820ED700 / 0x820ED780) then 1 → 6 (centre).
const SIX_TO_MONO_CENTRE: &[crate::dsp::routes::Route] = &[(0, 1, 1.0), (1, 1, 1.0), (2, 1, 1.0), (3, 1, 1.0), (4, 1, 1.0)];

/// A live voice as [`Mixer::snapshot`] reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceInfo {
    pub id: u32,
    pub bank: usize,
    pub slot: u16,
    pub gain: f32,
    pub pitch: f32,
    pub paused: bool,
    /// Send A (env) level: master × FXWET0, and the owner bus's env send (direct voices).
    pub send: f32,
    /// The output bus.
    pub output: Output,
}

/// Per loaded bank: sample headers and decoded PCM by S10A slot.
struct BankSamples {
    headers: Vec<Option<SampleHeader>>,
    pcm: Vec<Option<Arc<Pcm>>>,
    group: u8,
}

/// Volume groups of the banks (our user volumes; retail has none): world sounds (emitters,
/// ambience) and the player's own sounds.
pub const GROUP_WORLD: u8 = 0;
pub const GROUP_PLAYER: u8 = 1;

pub struct Mixer {
    voices: Vec<Voice>,
    /// The environment network and the eEQChain buses behind the voices.
    pub buses: Buses,
    next: u32,
    banks: HashMap<usize, BankSamples>,
    /// Release de-click values for the next block.
    fold: [f32; 6],
    /// User volume per bank group ([`GROUP_WORLD`], [`GROUP_PLAYER`]).
    pub group_gain: [f32; 2],
    /// Most voices that may sound at once; further opens fail (retail retires voices by CPU
    /// budget instead, spec §6.6).
    pub max_voices: usize,
    /// Opens refused because of `max_voices` or a missing sample.
    pub refused: u64,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    pub fn new() -> Self {
        Self { voices: Vec::new(), buses: Buses::default(), next: 0, banks: HashMap::new(), fold: [0.0; 6], group_gain: [1.0; 2], max_voices: 96, refused: 0 }
    }

    /// Register a bank's sample headers (by slot) and PCM (None = play silence for the sample's
    /// duration, so programs keep their timing).
    pub fn add_bank(&mut self, bank: usize, headers: Vec<Option<SampleHeader>>, pcm: Vec<Option<Arc<Pcm>>>) {
        self.banks.insert(bank, BankSamples { headers, pcm, group: GROUP_WORLD });
    }

    /// Put a bank's voices in a volume group (voices opened from then on).
    pub fn set_bank_group(&mut self, bank: usize, group: u8) {
        if let Some(b) = self.banks.get_mut(&bank) {
            b.group = group.min(1);
        }
    }

    pub fn remove_bank(&mut self, bank: usize) {
        self.banks.remove(&bank);
    }

    /// What each live voice is playing and at which gain (master × dry, before pan and sends):
    /// for diagnostics and tests.
    pub fn snapshot(&self) -> Vec<VoiceInfo> {
        self.voices
            .iter()
            .map(|v| VoiceInfo {
                id: v.id,
                bank: v.bank,
                slot: v.slot,
                gain: v.master * v.dry,
                pitch: v.pitch,
                paused: v.state == State::Paused,
                send: v.master * v.fx + v.owner_env.target,
                output: v.output,
            })
            .collect()
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    fn voice(&mut self, id: u32) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    /// A Splice voice (`crate::splice`): SndPlayer1 from `seek` seconds into the sample →
    /// Resample (pitch) → Gain → Pan2D1 (mono at `azimuth` degrees; multichannel at its default
    /// layout) → Send into the default bus. The filters stay at their bypassing class defaults.
    pub fn open_direct(&mut self, bank: usize, slot: u16, seek: f64, pitch: f32, gain: f32, azimuth: Option<f32>) -> Option<u32> {
        self.open_routed(bank, slot, seek, pitch, gain, azimuth, Route::default())
    }

    /// [`Mixer::open_direct`] into `route` (its eEQChain bus, resolved now, and the owner bus's env
    /// send).
    #[allow(clippy::too_many_arguments)]
    pub fn open_routed(&mut self, bank: usize, slot: u16, seek: f64, pitch: f32, gain: f32, azimuth: Option<f32>, route: Route) -> Option<u32> {
        let samples = self.banks.get(&bank);
        let header = samples.and_then(|b| b.headers.get(usize::from(slot)).copied().flatten());
        let Some(header) = header else {
            self.refused += 1;
            return None;
        };
        if self.voices.len() >= self.max_voices {
            self.refused += 1;
            return None;
        }
        let pcm = samples.and_then(|b| b.pcm.get(usize::from(slot)).cloned().flatten());
        let group = samples.map_or(GROUP_WORLD, |b| b.group);
        self.next = self.next.wrapping_add(1).max(1);
        let channels = (header.channels as usize).clamp(1, 6);
        let mut pan = Pan2D::new(channels);
        if channels == 1 {
            pan.params[pan::ANGLE] = azimuth.unwrap_or(0.0);
        }
        let mut resampler = Resampler::default();
        resampler.position = (seek.max(0.0) * f64::from(header.rate)) as u64;
        if let Output::Eq(i) = route.output {
            self.buses.eq.resolve(i, route.create);
        }
        self.voices.push(Voice {
            id: self.next,
            bank,
            slot,
            header,
            pcm,
            started: false,
            state: State::Playing,
            resampler,
            pitch,
            hpf: Iir2::new(Kind::HighPass),
            lpf: Iir2::new(Kind::LowPass),
            master: gain,
            dry: 1.0,
            fx: 0.0,
            gain: Gain::default(),
            pan,
            send: Send::default(),
            routing: Routing::default(),
            output: route.output,
            env: Level::new(0.0),
            owner_env: Level::new(route.owner_env),
            fx_send: Level::new(0.0),
            mono_out: route.mono,
            group,
            fade: None,
            done: false,
        });
        Some(self.next)
    }

    /// The Splice voice's per-frame values.
    pub fn set_direct(&mut self, voice: u32, pitch: f32, gain: f32, azimuth: Option<f32>) {
        if let Some(v) = self.voice(voice) {
            v.pitch = pitch;
            v.master = gain;
            if let (1, Some(a)) = (v.pan.sources(), azimuth) {
                v.pan.params[pan::ANGLE] = a;
            }
        }
    }

    /// The voice still plays (not ended, released or gone).
    pub fn direct_alive(&self, voice: u32) -> bool {
        self.voices.iter().any(|v| v.id == voice && !v.done && v.state != State::Releasing)
    }

    /// Render one block of SFX Master (6 channels, overwritten): the voices into their buses, then
    /// the environment network and the eEQChain buses.
    pub fn render(&mut self, bus: &mut [[f32; BLOCK]; 6]) {
        self.render_with_env(bus, None);
    }

    /// [`Mixer::render`] with another contributor's mono input to the environment bus, added after
    /// the voices' (the grain chains' graph 2, pass order 5 after the voices' 0).
    pub fn render_with_env(&mut self, bus: &mut [[f32; BLOCK]; 6], env: Option<&[f32; BLOCK]>) {
        for ch in bus.iter_mut() {
            *ch = [0.0; BLOCK];
        }
        self.buses.clear_inputs();
        fold_release(bus, &self.fold);
        self.fold = [0.0; 6];
        for v in &mut self.voices {
            if v.state != State::Paused {
                let user = self.group_gain[usize::from(v.group)];
                v.render(bus, &mut self.buses, user);
            }
        }
        if let Some(env) = env {
            for (d, &x) in self.buses.env_in.iter_mut().zip(env.iter()) {
                *d += x;
            }
        }
        self.buses.render(bus);
        // Voices that played their stop fade leave now.
        let mut fold = [0.0f32; 6];
        self.voices.retain(|v| {
            if v.state == State::Releasing {
                for (f, l) in fold.iter_mut().zip(v.send.last) {
                    *f += l;
                }
                false
            } else {
                true
            }
        });
        self.fold = fold;
    }
}

impl VoiceHost for Mixer {
    fn open(&mut self, r: &OpenRequest) -> Option<u32> {
        let samples = self.banks.get(&r.bank);
        let header = samples.and_then(|b| b.headers.get(r.slot as usize).copied().flatten());
        let Some(header) = header else {
            self.refused += 1;
            return None;
        };
        if self.voices.len() >= self.max_voices {
            self.refused += 1;
            return None;
        }
        let pcm = samples.and_then(|b| b.pcm.get(r.slot as usize).cloned().flatten());
        let group = samples.map_or(GROUP_WORLD, |b| b.group);
        self.next = self.next.wrapping_add(1).max(1);
        let routing = Routing::parse(r.inputs);
        if let Some((i, create)) = routing.output_bus {
            self.buses.eq.resolve(i, create);
        }
        let channels = (header.channels as usize).clamp(1, 6);
        let mut pan = Pan2D::new(channels);
        let angle = |byte: u8| f32::from(u16::from(byte) << 8) * DEGREES_PER_UNIT;
        // Initial panner angles from the sample-group entry (spec §3.4); the census of all banks
        // shows per-channel azimuths, e.g. stereo 224/32 = −45°/+45°.
        match channels {
            1 => pan.params[pan::ANGLE] = angle(r.azimuth[0]),
            2 => {
                pan.params[pan::DISTANCE] = 0.0;
                pan.params[pan::SPREAD1] = angle(r.azimuth[1]);
            }
            4 => {
                pan.params[pan::DISTANCE] = 0.0;
                pan.params[pan::SPREAD1] = angle(r.azimuth[1]);
                pan.params[pan::SPREAD2] = angle(r.azimuth[3]);
            }
            _ => {
                pan.params[pan::DISTANCE] = 0.0;
                pan.params[pan::SPREAD1] = angle(r.azimuth[2]);
                pan.params[pan::SPREAD2] = angle(r.azimuth[4]);
            }
        }
        self.voices.push(Voice {
            id: self.next,
            bank: r.bank,
            slot: r.slot,
            header,
            pcm,
            started: false,
            state: State::Playing,
            resampler: Resampler::default(),
            pitch: 1.0,
            hpf: Iir2::new(Kind::HighPass),
            lpf: Iir2::new(Kind::LowPass),
            master: 1.0,
            dry: 1.0,
            fx: 0.0,
            gain: Gain::default(),
            pan,
            send: Send::default(),
            routing,
            output: routing.output_bus.map_or(Output::Master, |(i, _)| Output::Eq(i)),
            // Send A is posted 0 at open; property 5 / 2 set it.
            env: Level::new(0.0),
            owner_env: Level::new(0.0),
            fx_send: Level::new(0.0),
            mono_out: false,
            group,
            fade: None,
            done: false,
        });
        Some(self.next)
    }

    fn release(&mut self, voice: u32) {
        let Some(at) = self.voices.iter().position(|v| v.id == voice) else { return };
        let v = &mut self.voices[at];
        if v.started && !v.done && v.state != State::Paused {
            // SndPlayer flush: each channel's last sample decays to zero over 16 frames.
            let mut last = [0.0f32; 8];
            let from = v.resampler.position;
            for (c, l) in last.iter_mut().enumerate().take(v.header.channels as usize) {
                *l = v.sample(c, from);
            }
            v.fade = Some((from, last));
            v.state = State::Releasing;
        } else {
            let gone = self.voices.remove(at);
            for (f, l) in self.fold.iter_mut().zip(gone.send.last) {
                *f += l;
            }
        }
    }

    fn pause(&mut self, voice: u32) {
        if let Some(v) = self.voice(voice) {
            if matches!(v.state, State::Playing | State::Resuming) {
                v.state = State::Pausing;
            }
        }
    }

    fn resume(&mut self, voice: u32) {
        if let Some(v) = self.voice(voice) {
            if matches!(v.state, State::Paused | State::Pausing) {
                v.state = State::Resuming;
            }
        }
    }

    fn set(&mut self, voice: u32, id: u8, value: i32) {
        let Some(v) = self.voice(voice) else { return };
        match id {
            0 => v.pitch = value as f32 / 4096.0,
            2 => v.master = value as f32 * INV_32767,
            8 => v.dry = value as f32 * INV_32767,
            5 => v.fx = value as f32 * INV_32767,
            6 => v.lpf.cutoff = value as f32,
            7 => v.hpf.cutoff = value as f32,
            11 => {
                if let Some((_, level)) = &mut v.routing.effect {
                    *level = value as f32 * INV_32767;
                }
            }
            _ => {}
        }
    }

    fn set_azimuth(&mut self, voice: u32, value: i32) {
        if let Some(v) = self.voice(voice) {
            if v.pan.sources() == 1 {
                v.pan.params[pan::ANGLE] = value as f32 * DEGREES_PER_UNIT;
            }
        }
    }

    fn query(&mut self, voice: u32) -> VoiceStatus {
        let Some(v) = self.voice(voice) else { return VoiceStatus::default() };
        if v.done || v.state == State::Releasing {
            return VoiceStatus::default();
        }
        let duration = v.header.seconds();
        let elapsed = v.elapsed();
        VoiceStatus { alive: true, remaining_ms: ((duration - elapsed) * 1000.0) as i32, elapsed_ms: (elapsed * 1000.0) as i32 }
    }
}

impl Voice {
    #[cfg(test)]
    fn routing(&self) -> Routing {
        self.routing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(seconds: f64, rate: u32, looping: bool) -> (SampleHeader, Arc<Pcm>) {
        let frames = (seconds * f64::from(rate)) as u32;
        let header = SampleHeader { codec: 3, channels: 1, rate, frames, loop_start: looping.then_some(0) };
        let data: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        (header, Arc::new(Pcm { rate, channels: vec![data] }))
    }

    fn request(slot: u16) -> OpenRequest<'static> {
        OpenRequest { bank: 0, slot, level: 100, azimuth: [0; 6], stream_offset: u32::MAX, inputs: &[] }
    }

    #[test]
    fn a_voice_starts_after_its_silent_format_block_and_ends_on_time() {
        let (h, pcm) = mono(0.02, 48000, false); // 960 frames = 3.75 blocks
        let mut m = Mixer::new();
        m.add_bank(0, vec![Some(h)], vec![Some(pcm.clone())]);
        let v = m.open(&request(0)).unwrap();
        let mut bus = [[0.0f32; BLOCK]; 6];
        m.render(&mut bus);
        assert!(bus.iter().all(|c| c.iter().all(|&s| s == 0.0)), "first block silent");
        m.render(&mut bus);
        // Mono, azimuth 0 → centre only, unity gain, unity pitch: an exact copy.
        assert_eq!(&bus[1][..], &pcm.channels[0][..BLOCK]);
        assert!(bus[0].iter().all(|&s| s == 0.0));
        let status = m.query(v);
        assert!(status.alive);
        assert_eq!(status.elapsed_ms, (256.0 / 48.0) as i32);
        assert_eq!(status.remaining_ms, ((0.02 - 256.0 / 48000.0) * 1000.0) as i32);
        for _ in 0..3 {
            m.render(&mut bus);
        }
        assert!(!m.query(v).alive);
    }

    #[test]
    fn properties_map_to_the_graph() {
        let (h, pcm) = mono(1.0, 44100, true);
        let mut m = Mixer::new();
        m.add_bank(0, vec![Some(h)], vec![Some(pcm)]);
        let v = m.open(&request(0)).unwrap();
        m.set(v, 2, 32767);
        m.set(v, 8, 16384);
        m.set(v, 0, 4096 * 2);
        m.set(v, 6, 1000);
        m.set_azimuth(v, 65536 - 65536 / 12); // −30° → L
        let voice = m.voice(v).unwrap();
        assert_eq!(voice.master, 32767.0 * INV_32767);
        assert_eq!(voice.pitch, 2.0);
        assert_eq!(voice.lpf.cutoff, 1000.0);
        assert!((voice.pan.params[pan::ANGLE] - 330.0).abs() < 0.01);
        let mut bus = [[0.0f32; BLOCK]; 6];
        m.render(&mut bus);
        m.render(&mut bus);
        assert!(bus[1].iter().all(|&s| s.abs() < 1e-4), "panned hard left: centre (almost) silent");
        assert!(bus[0].iter().any(|&s| s.abs() > 0.01));
        // Looping voices stay alive.
        for _ in 0..400 {
            m.render(&mut bus);
        }
        assert!(m.query(v).alive);
    }

    #[test]
    fn release_plays_a_16_frame_decay_then_removes_the_voice() {
        let (h, _) = mono(1.0, 48000, false);
        let pcm = Arc::new(Pcm { rate: 48000, channels: vec![vec![0.5; 48000]] });
        let mut m = Mixer::new();
        m.add_bank(0, vec![Some(h)], vec![Some(pcm)]);
        let v = m.open(&request(0)).unwrap();
        let mut bus = [[0.0f32; BLOCK]; 6];
        m.render(&mut bus);
        m.render(&mut bus);
        m.release(v);
        m.render(&mut bus);
        assert_eq!(bus[1][0], 0.5);
        assert_eq!(bus[1][8], 0.25);
        assert!(bus[1][16..].iter().all(|&s| s == 0.0));
        assert_eq!(m.voice_count(), 0);
        assert!(!m.query(v).alive);
    }

    #[test]
    fn a_mono_submix_route_sums_the_panned_voice_into_the_centre() {
        let (h, pcm) = mono(0.1, 48000, false);
        let render = |route: Route| {
            let mut m = Mixer::new();
            m.add_bank(0, vec![Some(h)], vec![Some(pcm.clone())]);
            m.open_routed(0, 0, 0.0, 1.0, 1.0, Some(70.0), route).unwrap();
            let mut bus = [[0.0f32; BLOCK]; 6];
            m.render(&mut bus);
            m.render(&mut bus);
            (bus, m.buses.env_in.to_vec())
        };
        let (stereo, _) = render(Route::default());
        let (mono, env) = render(Route { mono: true, owner_env: 0.1, ..Route::default() });
        // Panned off-centre: the plain route spreads it over the speakers; the submix folds every
        // speaker (L..Rs) into the centre, louder than the centre alone by the pan's sum.
        assert!(stereo[2].iter().any(|&v| v != 0.0) && stereo[4].iter().any(|&v| v != 0.0));
        for k in 0..BLOCK {
            let sum: f32 = (0..5).map(|c| stereo[c][k]).sum();
            assert!((mono[1][k] - sum).abs() < 1e-6);
            assert!((env[k] - 0.1 * sum).abs() < 1e-6, "the env send taps the same sum");
        }
        assert!((0..6).filter(|&c| c != 1).all(|c| mono[c].iter().all(|&v| v == 0.0)));
    }

    #[test]
    fn send_b_feeds_the_flange_return_only_when_enabled() {
        use crate::bus::flange::FlangePreset;
        let render = |enabled: bool, inputs: &[(u8, i32)]| {
            let (h, pcm) = mono(0.5, 48000, false);
            let mut m = Mixer::new();
            if enabled {
                let a = FlangePreset([20.0, 0.3, 0.2, 0.1, 1500.0, 0.002, 0.5, 0.9, 0.03]);
                let b = FlangePreset([1.7, 0.3, 0.0, 1.0, 250.0, 0.0005, 0.25, 0.6, 0.11]);
                m.buses.flange.set_presets(a, b);
                m.buses.flange.frame([32692, 2313, 32692, 2313]);
            }
            m.add_bank(0, vec![Some(h)], vec![Some(pcm)]);
            let v = m.open(&OpenRequest { inputs, ..request(0) }).unwrap();
            for &(id, value) in inputs {
                m.set(v, id, value);
            }
            let mut out = Vec::new();
            for _ in 0..20 {
                let mut bus = [[0.0f32; BLOCK]; 6];
                m.render(&mut bus);
                out.push(bus);
            }
            out
        };
        let plain = render(false, &[]);
        let fx = [(9u8, 4096i32), (10, 1), (11, 16384), (12, 5)];
        // Disabled returns: a routed voice renders exactly like before (its EQ bus 5 aside).
        assert_eq!(render(false, &[(9, 5)]), render(false, &fx));
        // Enabled, but the voice has no effect record: identical to the plain render.
        assert_eq!(plain, render(true, &[]));
        // Enabled with the record: the delayed copy adds into the centre.
        let wet = render(true, &[(9, 4096), (10, 1), (11, 16384)]);
        let diff: f32 = wet.iter().zip(&plain).map(|(a, b)| a[1].iter().zip(&b[1]).map(|(x, y)| (x - y).abs()).sum::<f32>()).sum();
        assert!(diff > 0.1, "{diff}");
        // Enable record 0: no Send B.
        assert_eq!(plain, render(true, &[(9, 4096), (10, 0), (11, 16384)]));
    }

    #[test]
    fn routing_codes() {
        let r = Routing::parse(&[(2, 4096), (9, 3), (10, 4096), (12, 1), (11, 16384)]);
        assert_eq!(r.output_bus, Some((3, false)));
        let (bus, level) = r.effect.unwrap();
        assert_eq!(bus, 0);
        assert!((level - 0.5).abs() < 1e-4);
        assert_eq!(Routing::parse(&[(9, 12)]).output_bus, Some((2, true)));
        assert_eq!(Routing::parse(&[(9, 2048)]).fixed, Some(2048));
        assert_eq!(Routing::parse(&[(9, 4096), (10, 0), (11, 9)]).effect, None);
        let (h, pcm) = mono(0.1, 48000, false);
        let mut m = Mixer::new();
        m.add_bank(0, vec![Some(h)], vec![Some(pcm)]);
        let inputs = [(9u8, 5i32)];
        let v = m.open(&OpenRequest { inputs: &inputs, ..request(0) }).unwrap();
        assert_eq!(m.voice(v).unwrap().routing().output_bus, Some((5, false)));
        assert_eq!(m.voice(v).unwrap().output, Output::Eq(5));
    }
}
