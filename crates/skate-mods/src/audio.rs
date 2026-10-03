//! Bounded API-2 audio commands and canonical PCM WAV validation.
//! Only local, mod-relative PCM16 WAV files are accepted by extension 1.
use serde::Deserialize;

pub const MAX_WAV_BYTES: u64 = 8 * 1024 * 1024;
pub fn default_fade() -> f32 { 0.03 }
fn one() -> f32 { 1.0 }
fn spatial_scale() -> f32 { 0.1 }
fn yes() -> bool { true }
fn fade_in() -> f32 { 0.01 }

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPlayOptions {
    pub path: String,
    #[serde(default)] pub body: Option<String>,
    #[serde(default)] pub position: Option<[f32; 3]>,
    #[serde(default)] pub offset: [f32; 3],
    #[serde(default, rename = "loop")] pub looping: bool,
    #[serde(default = "one")] pub volume: f32,
    #[serde(default = "one")] pub pitch: f32,
    #[serde(default = "yes")] pub spatial: bool,
    #[serde(default = "spatial_scale")] pub spatial_scale: f32,
    #[serde(default)] pub paused: bool,
    #[serde(default = "fade_in")] pub fade_in: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioUpdateOptions {
    pub volume: Option<f32>,
    pub pitch: Option<f32>,
    pub paused: Option<bool>,
    pub position: Option<[f32; 3]>,
    pub offset: Option<[f32; 3]>,
}

fn between(x: f32, lo: f32, hi: f32) -> bool {
    x.is_finite() && (lo..=hi).contains(&x)
}
fn point(v: &[f32; 3]) -> bool { v.iter().all(|x| between(*x, -100_000.0, 100_000.0)) }
fn offset(v: &[f32; 3]) -> bool { v.iter().all(|x| between(*x, -100.0, 100.0)) }

pub fn valid_audio_path(path: &str) -> bool {
    !path.is_empty() && path.len() <= 256 && path.to_ascii_lowercase().ends_with(".wav")
        && !path.chars().any(|c| matches!(c, '\\' | ':' | '#' | '?')) && !path.chars().any(char::is_control)
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}
impl AudioPlayOptions {
    pub fn validate(&self) -> bool {
        valid_audio_path(&self.path)
            && self.body.as_deref().is_none_or(crate::schema::valid_id)
            && self.position.as_ref().is_none_or(point)
            && !(self.body.is_some() && self.position.is_some())
            && offset(&self.offset)
            && between(self.volume, 0.0, 1.0)
            && between(self.pitch, 0.25, 4.0)
            && between(self.spatial_scale, 0.001, 1.0)
            && between(self.fade_in, 0.0, 2.0)
    }
}
impl AudioUpdateOptions {
    pub fn validate(&self) -> bool {
        self.volume.is_none_or(|v| between(v, 0.0, 1.0))
            && self.pitch.is_none_or(|v| between(v, 0.25, 4.0))
            && self.position.as_ref().is_none_or(point)
            && self.offset.as_ref().is_none_or(offset)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WavInfo { pub seconds: f64, pub channels: u16, pub sample_rate: u32 }

/// Validate bounded, uncompressed PCM16 and rebuild a minimal, canonical WAV.
/// Unknown metadata chunks never reach the backend decoder. No resampling here.
pub fn canonical_pcm_wav(bytes: &[u8]) -> Result<(Vec<u8>, WavInfo), String> {
    let bad = || "Audio requires a valid PCM16 WAV: 1-2 channels, 8-48 kHz, 0-30 seconds".to_owned();
    if bytes.len() < 44 || bytes.len() as u64 > MAX_WAV_BYTES
        || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad());
    }
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    if u32le(&bytes[4..8]) as u64 + 8 != bytes.len() as u64 { return Err(bad()); }
    let mut fmt = None;
    let mut data = None;
    let mut cursor = 12usize;
    while cursor < bytes.len() {
        let header_end = cursor.checked_add(8).ok_or_else(bad)?;
        if header_end > bytes.len() { return Err(bad()); }
        let count = u32le(&bytes[cursor + 4..header_end]) as usize;
        let end = header_end.checked_add(count).ok_or_else(bad)?;
        if end > bytes.len() { return Err(bad()); }
        let chunk = &bytes[header_end..end];
        match &bytes[cursor..cursor + 4] {
            b"fmt " => {
                if fmt.is_some() || !(count == 16 || count == 18) { return Err(bad()); }
                if u16le(chunk) != 1 || u16le(&chunk[14..]) != 16
                    || (count == 18 && u16le(&chunk[16..]) != 0) { return Err(bad()); }
                let channels = u16le(&chunk[2..]);
                let sample_rate = u32le(&chunk[4..]);
                if !(1..=2).contains(&channels) || !(8000..=48000).contains(&sample_rate)
                    || u16le(&chunk[12..]) != channels * 2
                    || u32le(&chunk[8..]) != sample_rate * u32::from(channels) * 2 {
                    return Err(bad());
                }
                fmt = Some((channels, sample_rate));
            }
            b"data" => {
                if data.is_some() { return Err(bad()); }
                data = Some(chunk);
            }
            _ => {}
        }
        cursor = end.checked_add(count & 1).ok_or_else(bad)?;
        if cursor > bytes.len() { return Err(bad()); }
    }
    let (channels, sample_rate) = fmt.ok_or_else(bad)?;
    let data = data.ok_or_else(bad)?;
    let block = usize::from(channels) * 2;
    if data.is_empty() || data.len() % block != 0 { return Err(bad()); }
    let seconds = (data.len() / block) as f64 / f64::from(sample_rate);
    if seconds > 30.0 { return Err(bad()); }
    let mut out = Vec::with_capacity(data.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(data.len() as u32 + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    Ok((out, WavInfo { seconds, channels, sample_rate }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn wav() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF"); b.extend_from_slice(&196u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt "); b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes()); b.extend_from_slice(&16000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes()); b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data"); b.extend_from_slice(&160u32.to_le_bytes());
        b.resize(204, 0); b
    }
    #[test] fn safe_paths() {
        assert!(valid_audio_path("audio/engine.wav"));
        for p in ["/a.wav", "../a.wav", "a/../b.wav", "C:/a.wav", "a\\b.wav", "https://x/a.wav", "a//b.wav", "./a.wav", "a.ogg", "a.wav#b"] {
            assert!(!valid_audio_path(p), "accepted {p}");
        }
    }
    #[test] fn options_and_commands() {
        let p: AudioPlayOptions = serde_json::from_value(json!({"path":"a.wav","loop":true,"volume":0.0})).unwrap();
        assert!(p.validate() && p.looping && p.volume == 0.0);
        for extra in [json!({"pitch":0}), json!({"volume":1.1}), json!({"body":"x","position":[0,0,0]})] {
            let mut value=json!({"path":"a.wav"});
            value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let p: AudioPlayOptions=serde_json::from_value(value).unwrap(); assert!(!p.validate());
        }
        assert!(serde_json::from_value::<AudioPlayOptions>(json!({"path":"a.wav","typo":true})).is_err());
        for kind in ["audio_preload","audio_play","audio_update","audio_stop","audio_stop_all"] {
            let value=match kind {
                "audio_preload" => json!({"kind":kind,"path":"a.wav"}),
                "audio_play" => json!({"kind":kind,"key":"engine","options":{"path":"a.wav"}}),
                "audio_update" => json!({"kind":kind,"key":"engine","options":{"volume":0.0,"paused":false}}),
                "audio_stop" => json!({"kind":kind,"key":"engine","fade_out":0.0}),
                _ => json!({"kind":kind}),
            };
            let c: crate::Command=serde_json::from_value(value).unwrap(); assert!(c.validate());
        }
        let update=AudioUpdateOptions {pitch:Some(f32::NAN), ..Default::default()};
        assert!(!update.validate());
    }
    #[test] fn pcm_round_trip_and_corruption() {
        let b=wav(); let (out,info)=canonical_pcm_wav(&b).unwrap();
        assert_eq!(b,out); assert_eq!(info.channels,1); assert!((info.seconds-0.01).abs()<1e-9);
        for n in 0..b.len() { assert!(canonical_pcm_wav(&b[..n]).is_err()); }
        for (index,value) in [(20,3),(22,0),(32,4),(34,24),(40,255)] {
            let mut bad=b.clone(); bad[index]=value; assert!(canonical_pcm_wav(&bad).is_err());
        }
    }
}
