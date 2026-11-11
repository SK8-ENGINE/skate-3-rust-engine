//! The Music Player screen's rules (Options tab, natives registered by sub_825AC8B8): the player's playlist of
//! soundtrack songs and the music mode. Details: docs/hails-additions/11-audio.md#music-player-screen

/// Retail keeps up to 100 song ids (options block +0xF8 .. +0x288).
pub const MAX_SONGS: usize = 100;

/// `SetMusicMode` (sub_825E4A60; options block +0xD4). The screen's tabs set it (input sub_8260B6F0 through
/// sub_8260BBE0): the Ambient tab 1, the Music Player tab 2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MusicMode {
    /// 0: the game's default music (music manager state 2); a new profile's mode.
    #[default]
    Default,
    /// 1: the screen's Interactive Mode (sub_82498A80(manager, 4)): the soundtrack stops and the music zones choose
    /// the music (licensed location songs or DJ world music; .local/research/music-zones-dj.md).
    Ambient,
    /// 2: the Music Player plays the playlist (manager state 7, sub_825E46B0 picks the song).
    Player,
}

impl MusicMode {
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::Default,
            1 => Self::Ambient,
            2 => Self::Player,
            _ => return None,
        })
    }
    pub fn code(self) -> i64 {
        match self {
            Self::Default => 0,
            Self::Ambient => 1,
            Self::Player => 2,
        }
    }
    /// The soundtrack plays (every mode but Ambient).
    pub fn soundtrack(self) -> bool {
        self != Self::Ambient
    }
}

/// Song `index` of the soundtrack (sub_82D15BA8: table entry index + 1); None past the last song.
pub fn song_id(index: i64, songs: u32) -> Option<u32> {
    u32::try_from(index).ok().filter(|&i| i < songs).map(|i| i + 1)
}

/// `ToggleSongInPlayList` (sub_8260B5D0): a listed song leaves the list (the last entry takes its slot) unless it is the
/// only one; an unlisted song is appended. Returns whether the song is listed afterwards.
pub fn toggle(playlist: &mut Vec<u32>, id: u32) -> bool {
    match playlist.iter().position(|&s| s == id) {
        Some(_) if playlist.len() == 1 => true,
        Some(at) => {
            playlist.swap_remove(at);
            false
        }
        None if playlist.len() < MAX_SONGS => {
            playlist.push(id);
            true
        }
        None => false,
    }
}

/// The playlist a profile starts with: every song (NOT RETAIL YET: a new profile's list is not read).
pub fn default_playlist(songs: u32) -> Vec<u32> {
    (1..=songs.min(MAX_SONGS as u32)).collect()
}

/// A saved playlist kept to songs that exist, without repeats; empty or unusable -> the default.
pub fn sanitise(saved: &[u32], songs: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for &id in saved {
        if (1..=songs).contains(&id) && !out.contains(&id) && out.len() < MAX_SONGS {
            out.push(id);
        }
    }
    if out.is_empty() { default_playlist(songs) } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_removes_with_the_last_entry_taking_the_slot_and_keeps_the_only_song() {
        let mut list = vec![1, 2, 3, 4];
        assert!(!toggle(&mut list, 2));
        assert_eq!(list, [1, 4, 3]);
        assert!(toggle(&mut list, 2));
        assert_eq!(list, [1, 4, 3, 2]);
        let mut one = vec![7];
        assert!(toggle(&mut one, 7));
        assert_eq!(one, [7]);
    }

    #[test]
    fn song_ids_are_one_based_track_numbers() {
        assert_eq!(song_id(0, 3), Some(1));
        assert_eq!(song_id(2, 3), Some(3));
        assert_eq!(song_id(3, 3), None);
        assert_eq!(song_id(-1, 3), None);
    }

    #[test]
    fn saved_lists_drop_unknown_and_repeated_songs_and_fall_back_to_every_song() {
        assert_eq!(sanitise(&[3, 9, 3, 1], 5), [3, 1]);
        assert_eq!(sanitise(&[], 3), [1, 2, 3]);
        assert_eq!(sanitise(&[42], 3), [1, 2, 3]);
    }

    #[test]
    fn modes_round_trip_their_codes() {
        for mode in [MusicMode::Default, MusicMode::Ambient, MusicMode::Player] {
            assert_eq!(MusicMode::from_code(mode.code()), Some(mode));
        }
        assert_eq!(MusicMode::from_code(5), None);
    }
}
