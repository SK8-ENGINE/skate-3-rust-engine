//! The music packer's converter (`skate3rust --pack-music-folder`; docs/hails-additions/11-audio.md#music-banks):
//! the player's own music files in most formats, decoded with Symphonia (pure Rust) a packet at a time into
//! 10 s 16-bit segments, so an hour-long mix never sits in memory whole. Each file keeps its rate; more than two
//! channels fold to the front left / right. Names come from the file's tags, else `Artist - Title.ext`.
use std::path::Path;

use skate_audio::music::bank::{artist_title, Segment, SEGMENT_SECONDS};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;

/// Printed by the packer: banks hold the music they were made from.
pub(crate) const NOTICE: &str = "Music banks contain the music you pack into them. Do not share bank files that hold \
copyrighted music; share playlists that point to banks people make from their own music instead.";

/// Extensions the packer looks at (what the enabled Symphonia formats read).
pub(crate) const EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "mp4", "alac", "flac", "ogg", "oga", "wav", "aif", "aiff", "mka", "webm"];

pub(crate) fn is_music_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A file's title and artist: its tags when it has them, else the file name.
pub(crate) struct Info {
    pub title: String,
    pub artist: String,
}

/// Decodes `path`, handing each 10 s segment (16-bit, interleaved, at most stereo) to `segment`; returns its names.
pub(crate) fn decode(path: &Path, mut segment: impl FnMut(Segment) -> Result<(), String>) -> Result<Info, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("not a readable music file ({e})"))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (artist, title) = artist_title(&stem);
    let mut info = Info { title, artist };
    let mut tags = Vec::new();
    if let Some(m) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
        tags.extend(m.tags().iter().cloned());
    }
    if let Some(m) = probed.format.metadata().current() {
        tags.extend(m.tags().iter().cloned());
    }
    for tag in &tags {
        let text = tag.value.to_string();
        if text.trim().is_empty() {
            continue;
        }
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) => info.title = text.trim().to_owned(),
            Some(StandardTagKey::Artist) => info.artist = text.trim().to_owned(),
            _ => {}
        }
    }
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or("no audio track")?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("unsupported audio ({e})"))?;
    let (mut rate, mut channels, mut pending): (u32, u32, Vec<i16>) = (0, 0, Vec::new());
    let emit = |rate: u32, channels: u32, pending: &mut Vec<i16>, all: bool, segment: &mut dyn FnMut(Segment) -> Result<(), String>| {
        let per = (rate * SEGMENT_SECONDS) as usize * channels as usize;
        while per > 0 && (pending.len() >= per || (all && !pending.is_empty())) {
            let take = per.min(pending.len());
            let rest = pending.split_off(take);
            segment(Segment { rate, channels, samples: std::mem::replace(pending, rest) })?;
        }
        Ok::<(), String>(())
    };
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(format!("read error ({e})")),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged frame is skipped, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode error ({e})")),
        };
        let spec = *decoded.spec();
        let source_channels = spec.channels.count().max(1);
        let out_channels = source_channels.min(2) as u32;
        if rate != 0 && (spec.rate != rate || out_channels != channels) {
            // The stream changed shape: what came before is one segment run of its own.
            emit(rate, channels, &mut pending, true, &mut segment)?;
        }
        rate = spec.rate;
        channels = out_channels;
        let mut buffer = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks(source_channels) {
            pending.extend_from_slice(&frame[..out_channels as usize]);
        }
        emit(rate, channels, &mut pending, false, &mut segment)?;
    }
    if rate == 0 {
        return Err("no audio decoded".into());
    }
    emit(rate, channels, &mut pending, true, &mut segment)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit stereo WAV written by hand, 25 s at 8 kHz: three segments (10, 10, 5 s), samples kept exactly.
    #[test]
    fn a_wav_decodes_into_ten_second_segments_with_its_name() {
        let dir = std::env::temp_dir().join(format!("skate-music-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let frames = 25 * 8000u32;
        let samples: Vec<i16> = (0..frames * 2).map(|i| (i % 2000) as i16 - 1000).collect();
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend((36 + frames * 4).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        for v in [1u16, 2] { w.extend(v.to_le_bytes()); }
        w.extend(8000u32.to_le_bytes());
        w.extend(32_000u32.to_le_bytes());
        for v in [4u16, 16] { w.extend(v.to_le_bytes()); }
        w.extend(b"data");
        w.extend((frames * 4).to_le_bytes());
        for s in &samples { w.extend(s.to_le_bytes()); }
        let path = dir.join("Band - Song.wav");
        std::fs::write(&path, &w).unwrap();
        assert!(is_music_file(&path) && !is_music_file(&dir.join("notes.txt")));
        let mut parts = Vec::new();
        let info = decode(&path, |s| { parts.push(s); Ok(()) }).unwrap();
        assert_eq!((info.artist.as_str(), info.title.as_str()), ("Band", "Song"));
        assert_eq!(parts.iter().map(|p| p.samples.len() / 2).collect::<Vec<_>>(), [80_000, 80_000, 40_000]);
        assert_eq!(parts.iter().flat_map(|p| p.samples.iter().copied()).collect::<Vec<_>>(), samples);
        assert!(parts.iter().all(|p| p.rate == 8000 && p.channels == 2));
        std::fs::remove_dir_all(&dir).ok();
    }
}
