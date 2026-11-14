//! Music playlists (docs/hails-additions/11-audio.md#music-banks, mod music): a JSON file naming tracks in music
//! banks, bank paths relative to the playlist file. A track is found by its index in the bank and checked against
//! its name (a repacked bank in another order is found again by name). The tools behind `skate3rust
//! --music-bank-info` and `--music-playlist`.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use skate_audio::music::bank::Bank;

pub(crate) const SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Playlist {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub shuffle: bool,
    pub tracks: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// The bank file, relative to the playlist (forward slashes); empty for a soundtrack entry.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bank: String,
    /// A disc soundtrack track (1-based) instead of a bank's track: banks and the soundtrack mix in one playlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soundtrack: Option<u32>,
    /// The track's index in the bank.
    #[serde(default)]
    pub track: u32,
    /// The track's name when the playlist was made (to find it again in a repacked bank).
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
}

/// A bank's index read from a file.
pub(crate) fn read_bank(path: &Path) -> Result<Bank, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Bank::read_index(&mut f).map_err(|e| format!("{}: {e}", path.display()))
}

/// One line per track: index, seconds, artist, name.
pub(crate) fn describe(bank: &Bank) -> Vec<String> {
    bank.tracks.iter().enumerate().map(|(i, t)| {
        let seconds: f32 = bank.entries.iter().skip(t.first as usize).take(t.count as usize).map(|e| e.seconds()).sum();
        let artist = if t.artist.is_empty() { String::new() } else { format!("{} - ", t.artist) };
        format!("{i:4}  {:>2}:{:02}  {artist}{}", (seconds / 60.0) as u32, (seconds % 60.0) as u32, t.name)
    }).collect()
}

/// `bank` relative to the playlist's folder when it is inside it (else as given), forward slashes.
fn relative(playlist: &Path, bank: &Path) -> String {
    let base = playlist.parent().unwrap_or(Path::new(""));
    let bank_abs = std::fs::canonicalize(bank).unwrap_or_else(|_| bank.to_path_buf());
    let base_abs = std::fs::canonicalize(if base.as_os_str().is_empty() { Path::new(".") } else { base }).unwrap_or_else(|_| base.to_path_buf());
    let shown = bank_abs.strip_prefix(&base_abs).map(Path::to_path_buf).unwrap_or_else(|_| bank.to_path_buf());
    shown.to_string_lossy().replace('\\', "/")
}

/// A playlist of every track of `banks`, in bank order.
pub(crate) fn from_banks(playlist_path: &Path, name: &str, shuffle: bool, banks: &[PathBuf]) -> Result<Playlist, String> {
    let mut tracks = Vec::new();
    for path in banks {
        let bank = read_bank(path)?;
        if bank.tracks.is_empty() {
            return Err(format!("{}: no track list (packed by an older packer?)", path.display()));
        }
        let rel = relative(playlist_path, path);
        for (i, t) in bank.tracks.iter().enumerate() {
            tracks.push(Entry { bank: rel.clone(), soundtrack: None, track: i as u32, name: t.name.clone(), artist: t.artist.clone() });
        }
    }
    Ok(Playlist { schema: SCHEMA, name: name.to_owned(), shuffle, tracks })
}

/// The bank file and the track index an entry plays: the stored index when its name still matches (or the entry has
/// no name), else the first track of that name.
pub(crate) fn resolve(playlist_path: &Path, entry: &Entry, bank: &Bank) -> Option<(PathBuf, usize)> {
    let base = playlist_path.parent().unwrap_or(Path::new(""));
    let file = base.join(&entry.bank);
    let at = entry.track as usize;
    let index = match bank.tracks.get(at) {
        Some(t) if entry.name.is_empty() || t.name == entry.name => at,
        _ => bank.tracks.iter().position(|t| t.name == entry.name)?,
    };
    Some((file, index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_audio::music::bank::{write_bank, Segment, Track};

    fn bank_file(path: &Path, names: &[&str]) {
        let tracks: Vec<Track> = names.iter().enumerate()
            .map(|(i, n)| Track { first: i as u32, count: 1, name: (*n).into(), artist: "A".into() }).collect();
        let segments = names.iter().map(|_| Ok(Segment { rate: 8000, channels: 1, samples: vec![1; 8000] }));
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write_bank(&mut f, names.len() as u32, segments, &tracks).unwrap();
    }

    #[test]
    fn a_playlist_names_every_track_relative_to_itself_and_finds_a_moved_track_by_name() {
        let dir = std::env::temp_dir().join(format!("skate-playlist-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("banks")).unwrap();
        bank_file(&dir.join("banks/one.bank"), &["First", "Second"]);
        let list = dir.join("mine.json");
        let p = from_banks(&list, "Mine", true, &[dir.join("banks/one.bank")]).unwrap();
        assert_eq!((p.name.as_str(), p.shuffle, p.tracks.len()), ("Mine", true, 2));
        assert_eq!((p.tracks[1].bank.as_str(), p.tracks[1].track, p.tracks[1].name.as_str()), ("banks/one.bank", 1, "Second"));
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Playlist>(&json).unwrap(), p);
        let bank = read_bank(&dir.join("banks/one.bank")).unwrap();
        assert_eq!(describe(&bank)[0], "   0   0:01  A - First");
        assert_eq!(resolve(&list, &p.tracks[1], &bank).unwrap().1, 1);
        // Repacked in another order: found by name.
        bank_file(&dir.join("banks/one.bank"), &["Second", "First"]);
        let repacked = read_bank(&dir.join("banks/one.bank")).unwrap();
        assert_eq!(resolve(&list, &p.tracks[1], &repacked).unwrap().1, 0);
        let gone = Entry { name: "Missing".into(), ..p.tracks[0].clone() };
        assert!(resolve(&list, &gone, &repacked).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
