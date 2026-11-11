//! A music bank: one file per music set (the disc's `ipod`, `world`, `game` PathFinder streams), its segments
//! encoded as FLAC back to back after an index, so the player opens the bank once and decodes one segment at a time
//! from its offset (docs/hails-additions/11-audio.md#music-banks). Readers and writers are generic: no file I/O here.
//!
//! Layout (little endian): `SKMB`, version u32, codec u32 ([`CODEC_FLAC`]), segment count u32, (version 2:) tracks
//! offset u64 and size u32, then per segment {offset u64 from the file start, size u32, frames u32, rate u32,
//! channels u32} (24 bytes), then the data, then (version 2) the track table: count u32, per track {first segment u32,
//! segment count u32, name, artist} with each string a u16 length and UTF-8 bytes. A track is consecutive segments
//! played in order. Version 1 banks (no tracks) still read. Another codec (the disc's own XMA2 copied as is) would only
//! change the codec word and the decoder.
use std::io::{Read, Seek, SeekFrom, Write};

use crate::mixer::Pcm;

pub const MAGIC: &[u8; 4] = b"SKMB";
pub const VERSION: u32 = 2;
/// Each segment is a complete FLAC stream (16-bit).
pub const CODEC_FLAC: u32 = 1;
const HEADER_V1: u64 = 16;
const HEADER: u64 = 28;
const ENTRY: u64 = 24;
const MIN_FRAMES: usize = 16;

/// One segment's place in the bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
}

impl Entry {
    pub fn seconds(&self) -> f32 {
        if self.rate == 0 { 0.0 } else { self.frames as f32 / self.rate as f32 }
    }
}

/// A track: `count` consecutive segments from `first`, played in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub first: u32,
    pub count: u32,
    pub name: String,
    pub artist: String,
}

/// A segment to pack: 16-bit samples, interleaved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub rate: u32,
    pub channels: u32,
    pub samples: Vec<i16>,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// A 16-bit PCM WAV file (vgmstream's output) as a segment.
pub fn wav16(bytes: &[u8]) -> Result<Segment, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF WAVE file".into());
    }
    let (mut at, mut format) = (12usize, None);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = word(bytes, at + 4) as usize;
        let body = at + 8;
        let end = body.checked_add(size).filter(|&e| e <= bytes.len()).unwrap_or(bytes.len());
        if id == b"fmt " && size >= 16 {
            let tag = u16::from_le_bytes([bytes[body], bytes[body + 1]]);
            let channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]) as u32;
            let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
            if tag != 1 || bits != 16 {
                return Err(format!("WAV format {tag} with {bits} bits (16-bit PCM expected)"));
            }
            format = Some((word(bytes, body + 4), channels));
        } else if id == b"data" {
            let (rate, channels) = format.ok_or("WAV data before its fmt chunk")?;
            let samples = bytes[body..end].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            return Ok(Segment { rate, channels, samples });
        }
        at = body + size + (size & 1);
    }
    Err("WAV without a data chunk".into())
}

/// Seconds per segment when a whole track is packed (the player decodes one segment at a time).
pub const SEGMENT_SECONDS: u32 = 10;

/// A whole track cut into segments of [`SEGMENT_SECONDS`] (the last one shorter).
pub fn split_track(track: &Segment) -> Vec<Segment> {
    let channels = track.channels.max(1) as usize;
    let per = (track.rate.max(1) * SEGMENT_SECONDS) as usize * channels;
    track.samples.chunks(per.max(channels)).map(|c| Segment { rate: track.rate, channels: track.channels, samples: c.to_vec() }).collect()
}

/// A FLAC file as a 16-bit track (other bit depths scale to 16 bits).
pub fn flac16(bytes: &[u8]) -> Result<Segment, String> {
    let mut reader = claxon::FlacReader::new(std::io::Cursor::new(bytes)).map_err(|e| format!("FLAC: {e}"))?;
    let info = reader.streaminfo();
    let shift = i32::try_from(info.bits_per_sample).unwrap_or(16) - 16;
    let mut samples = Vec::with_capacity(info.samples.unwrap_or(0) as usize * info.channels as usize);
    for s in reader.samples() {
        let s = s.map_err(|e| format!("FLAC: {e}"))?;
        let v = if shift >= 0 { s >> shift } else { s << -shift };
        samples.push(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    Ok(Segment { rate: info.sample_rate, channels: info.channels, samples })
}

/// "Artist - Title" from a file stem; without the separator the whole stem is the title.
pub fn artist_title(stem: &str) -> (String, String) {
    match stem.split_once(" - ") {
        Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => (a.trim().to_owned(), t.trim().to_owned()),
        _ => (String::new(), stem.trim().to_owned()),
    }
}

/// One segment as a FLAC stream.
pub fn encode_segment(segment: &Segment) -> Result<Vec<u8>, String> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    if segment.channels == 0 || segment.channels > 8 || segment.rate == 0 {
        return Err(format!("music segment: {} channels at {} Hz", segment.channels, segment.rate));
    }
    let config = flacenc::config::Encoder::default().into_verified().map_err(|e| format!("FLAC config: {e:?}"))?;
    let mut samples: Vec<i32> = segment.samples.iter().map(|&s| i32::from(s)).collect();
    // FLAC decoders refuse a stream shorter than its 16-sample minimum block: pad with silence (the index keeps the
    // real length and the decode trims to it).
    samples.resize(samples.len().max(MIN_FRAMES * segment.channels as usize), 0);
    let source = flacenc::source::MemSource::from_samples(&samples, segment.channels as usize, 16, segment.rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| format!("FLAC encode: {e:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| format!("FLAC write: {e:?}"))?;
    Ok(sink.as_slice().to_vec())
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    let mut end = text.len().min(u16::MAX as usize);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.extend((end as u16).to_le_bytes());
    out.extend(&text.as_bytes()[..end]);
}

/// Packs `segments` into `out` in order (index first, data streamed after it, the index and the track table written
/// at the end). `tracks` name consecutive segment runs (empty: a bank of loose segments).
pub fn write_bank<W: Write + Seek>(out: &mut W, count: u32, segments: impl IntoIterator<Item = Result<Segment, String>>,
                                   tracks: &[Track]) -> Result<Vec<Entry>, String> {
    let io = |e: std::io::Error| e.to_string();
    if let Some(t) = tracks.iter().find(|t| t.count == 0 || u64::from(t.first) + u64::from(t.count) > u64::from(count)) {
        return Err(format!("music bank: track {:?} outside the {count} segments", t.name));
    }
    out.seek(SeekFrom::Start(0)).map_err(io)?;
    out.write_all(MAGIC).map_err(io)?;
    for v in [VERSION, CODEC_FLAC, count] {
        out.write_all(&v.to_le_bytes()).map_err(io)?;
    }
    out.write_all(&[0u8; 12]).map_err(io)?;
    out.write_all(&vec![0u8; (ENTRY * u64::from(count)) as usize]).map_err(io)?;
    let mut offset = HEADER + ENTRY * u64::from(count);
    let mut entries = Vec::with_capacity(count as usize);
    for segment in segments {
        let segment = segment?;
        if entries.len() as u32 >= count {
            return Err(format!("music bank: more than the {count} segments announced"));
        }
        let data = encode_segment(&segment)?;
        out.write_all(&data).map_err(io)?;
        let frames = (segment.samples.len() / segment.channels as usize) as u32;
        entries.push(Entry { offset, size: data.len() as u32, frames, rate: segment.rate, channels: segment.channels });
        offset += data.len() as u64;
    }
    if entries.len() as u32 != count {
        return Err(format!("music bank: {} of {count} segments", entries.len()));
    }
    let mut table = Vec::new();
    table.extend((tracks.len() as u32).to_le_bytes());
    for t in tracks {
        table.extend(t.first.to_le_bytes());
        table.extend(t.count.to_le_bytes());
        put_text(&mut table, &t.name);
        put_text(&mut table, &t.artist);
    }
    out.write_all(&table).map_err(io)?;
    out.seek(SeekFrom::Start(HEADER_V1)).map_err(io)?;
    out.write_all(&offset.to_le_bytes()).map_err(io)?;
    out.write_all(&(table.len() as u32).to_le_bytes()).map_err(io)?;
    for e in &entries {
        out.write_all(&e.offset.to_le_bytes()).map_err(io)?;
        for v in [e.size, e.frames, e.rate, e.channels] {
            out.write_all(&v.to_le_bytes()).map_err(io)?;
        }
    }
    out.flush().map_err(io)?;
    Ok(entries)
}

/// A bank's index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bank {
    pub entries: Vec<Entry>,
    pub tracks: Vec<Track>,
}

impl Bank {
    pub fn read_index<R: Read + Seek>(input: &mut R) -> Result<Self, String> {
        let io = |e: std::io::Error| format!("music bank: {e}");
        let mut head = [0u8; HEADER as usize];
        input.seek(SeekFrom::Start(0)).map_err(io)?;
        input.read_exact(&mut head[..HEADER_V1 as usize]).map_err(io)?;
        let version = word(&head, 4);
        if &head[0..4] != MAGIC || !(1..=VERSION).contains(&version) || word(&head, 8) != CODEC_FLAC {
            return Err("music bank: not a version 1 or 2 FLAC bank".into());
        }
        if version >= 2 {
            input.read_exact(&mut head[HEADER_V1 as usize..]).map_err(io)?;
        }
        let count = word(&head, 12) as usize;
        if count > 1 << 20 {
            return Err(format!("music bank: {count} segments"));
        }
        let mut table = vec![0u8; count * ENTRY as usize];
        input.read_exact(&mut table).map_err(io)?;
        let entries = table.chunks_exact(ENTRY as usize).map(|e| Entry {
            offset: u64::from_le_bytes(e[0..8].try_into().expect("8 bytes")),
            size: word(e, 8),
            frames: word(e, 12),
            rate: word(e, 16),
            channels: word(e, 20),
        }).collect();
        let mut tracks = Vec::new();
        if version >= 2 {
            let at = u64::from_le_bytes(head[16..24].try_into().expect("8 bytes"));
            let size = word(&head, 24) as usize;
            if size > 16 << 20 {
                return Err(format!("music bank: track table of {size} bytes"));
            }
            let mut table = vec![0u8; size];
            input.seek(SeekFrom::Start(at)).map_err(io)?;
            input.read_exact(&mut table).map_err(io)?;
            tracks = read_tracks(&table).ok_or("music bank: bad track table")?;
        }
        Ok(Self { entries, tracks })
    }

    /// Decodes segment `index` (planar, -1..1).
    pub fn decode<R: Read + Seek>(&self, input: &mut R, index: usize) -> Result<Pcm, String> {
        let e = self.entries.get(index).ok_or_else(|| format!("music bank: no segment {index}"))?;
        let mut data = vec![0u8; e.size as usize];
        input.seek(SeekFrom::Start(e.offset)).map_err(|err| err.to_string())?;
        input.read_exact(&mut data).map_err(|err| err.to_string())?;
        let mut pcm = decode_flac(&data)?;
        for c in &mut pcm.channels {
            c.truncate(e.frames as usize);
        }
        Ok(pcm)
    }
}

fn read_tracks(t: &[u8]) -> Option<Vec<Track>> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<Vec<u8>> {
        let s = t.get(at..at + n)?.to_vec();
        at += n;
        Some(s)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut tracks = Vec::new();
    for _ in 0..count.min(1 << 16) {
        let first = u32::from_le_bytes(take(4)?.try_into().ok()?);
        let n = u32::from_le_bytes(take(4)?.try_into().ok()?);
        let len = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let name = String::from_utf8(take(len)?).ok()?;
        let len = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let artist = String::from_utf8(take(len)?).ok()?;
        tracks.push(Track { first, count: n, name, artist });
    }
    Some(tracks)
}

/// One FLAC stream to planar PCM.
pub fn decode_flac(data: &[u8]) -> Result<Pcm, String> {
    let mut reader = claxon::FlacReader::new(std::io::Cursor::new(data)).map_err(|e| format!("FLAC: {e}"))?;
    let info = reader.streaminfo();
    let channels = info.channels as usize;
    let scale = 1.0 / (1u32 << (info.bits_per_sample - 1)) as f32;
    let mut planar = vec![Vec::with_capacity(info.samples.unwrap_or(0) as usize); channels];
    for (i, s) in reader.samples().enumerate() {
        let s = s.map_err(|e| format!("FLAC: {e}"))?;
        planar[i % channels].push(s as f32 * scale);
    }
    Ok(Pcm { rate: info.sample_rate, channels: planar })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: u32, rate: u32) -> Segment {
        let samples = (0..frames * channels as usize)
            .map(|i| ((i as f32 * 0.05).sin() * 12000.0) as i16)
            .collect();
        Segment { rate, channels, samples }
    }

    #[test]
    fn a_bank_round_trips_its_segments_exactly() {
        let segments = [tone(5000, 2, 44_100), tone(1, 1, 48_000), tone(9000, 2, 44_100)];
        let mut file = std::io::Cursor::new(Vec::new());
        let tracks = [Track { first: 0, count: 2, name: "\u{dc}n".into(), artist: "A".into() },
            Track { first: 2, count: 1, name: "Two".into(), artist: String::new() }];
        let entries = write_bank(&mut file, 3, segments.iter().cloned().map(Ok), &tracks).unwrap();
        assert_eq!(entries.iter().map(|e| (e.frames, e.channels, e.rate)).collect::<Vec<_>>(),
            [(5000, 2, 44_100), (1, 1, 48_000), (9000, 2, 44_100)]);
        let bank = Bank::read_index(&mut file).unwrap();
        assert_eq!(bank.entries, entries);
        assert_eq!(bank.tracks, tracks);
        for (i, s) in segments.iter().enumerate() {
            let pcm = bank.decode(&mut file, i).unwrap();
            assert_eq!((pcm.rate, pcm.channels.len()), (s.rate, s.channels as usize));
            let back: Vec<i16> = (0..pcm.channels[0].len())
                .flat_map(|f| pcm.channels.iter().map(move |c| (c[f] * 32768.0).round() as i16))
                .collect();
            assert_eq!(back, s.samples, "segment {i} is lossless");
        }
        assert!(file.get_ref().len() < 6 * (5000 + 9000) * 2, "FLAC is smaller than the PCM");
    }

    #[test]
    fn a_track_splits_into_ten_second_segments_and_names_read_artist_and_title() {
        let track = tone(2 * 44_100 * SEGMENT_SECONDS as usize + 5, 2, 44_100);
        let parts = split_track(&track);
        assert_eq!(parts.iter().map(|p| p.samples.len() / 2).collect::<Vec<_>>(), [441_000, 441_000, 5]);
        assert_eq!(parts.concat_samples(), track.samples);
        assert_eq!(artist_title("Band - Song"), ("Band".into(), "Song".into()));
        assert_eq!(artist_title("Just a name"), (String::new(), "Just a name".into()));
        let flac = encode_segment(&tone(100, 2, 48_000)).unwrap();
        assert_eq!(flac16(&flac).unwrap(), tone(100, 2, 48_000));
    }

    trait Concat {
        fn concat_samples(&self) -> Vec<i16>;
    }
    impl Concat for Vec<Segment> {
        fn concat_samples(&self) -> Vec<i16> {
            self.iter().flat_map(|s| s.samples.iter().copied()).collect()
        }
    }

    #[test]
    fn a_16_bit_wav_reads_as_a_segment() {
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend(44u32.to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        for v in [1u16, 2] { w.extend(v.to_le_bytes()); }
        w.extend(44_100u32.to_le_bytes());
        w.extend((44_100u32 * 4).to_le_bytes());
        for v in [4u16, 16] { w.extend(v.to_le_bytes()); }
        w.extend(b"data");
        w.extend(8u32.to_le_bytes());
        for v in [1i16, -2, 3, -4] { w.extend(v.to_le_bytes()); }
        assert_eq!(wav16(&w).unwrap(), Segment { rate: 44_100, channels: 2, samples: vec![1, -2, 3, -4] });
        assert!(wav16(b"RIFF....WAVE").is_err());
    }

    #[test]
    fn a_wrong_count_or_header_is_refused() {
        let mut file = std::io::Cursor::new(Vec::new());
        assert!(write_bank(&mut file, 2, [Ok(tone(10, 1, 44_100))], &[]).is_err());
        assert!(write_bank(&mut file, 1, [Ok(tone(10, 1, 44_100))], &[Track { first: 0, count: 2, ..Default::default() }]).is_err(),
            "a track past the end");
        assert!(Bank::read_index(&mut std::io::Cursor::new(b"NOPE....".to_vec())).is_err());
    }
}
