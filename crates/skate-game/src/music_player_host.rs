//! FE 26 Music Player (template 31 `musicplayer/musicplayertest.swf`): the `MusicPlayer` natives (registrar
//! sub_825AC8B8) and the screen's input (sub_8260B6F0) while it is on top. Details:
//! docs/hails-additions/11-audio.md#music-player-screen
use crate::apt_vm::{to_integer, Value, Vm};
use skate_core::music_player::{song_id, toggle, MusicMode};
use std::collections::BTreeMap;

/// `fe` record keys the screen plays (sub_8260B6F0): Back (core_b_button), a song toggled, volume down / up, a tab.
pub const SOUND_BACK: u64 = 0x4D92_7CD6_4CC8_B818;
pub const SOUND_TOGGLE: u64 = 0x1747_2B6D_55E3_6C95;
pub const SOUND_VOLUME_DOWN: u64 = 0xDC1C_3539_8D55_97BC;
pub const SOUND_VOLUME_UP: u64 = 0x4648_BEBA_06CE_C261;
pub const SOUND_TAB: u64 = 0xFD87_5663_38C1_18E1;
/// `SetEditMode(false)` (sub_825AD4A8).
pub const SOUND_EDIT_DONE: u64 = 0x73D7_CC18_FE21_DBE9;

/// APT key indices the screen reads.
const KEY_SELECT: u32 = 2;
const KEY_BACK: u32 = 3;
const KEY_TOGGLE: u32 = 5;
const KEY_PLAYER_TAB: u32 = 7;
const KEY_AMBIENT_TAB: u32 = 10;
const KEY_LEFT: u32 = 14;
const KEY_RIGHT: u32 = 15;
/// Row kinds per tab and row (table 0x83026E70, tab * 4 + row): 2 = the volume row.
const ROW_KINDS: [[u8; 4]; 2] = [[2, 0, 0, 0], [2, 1, 0, 0]];
const VOLUME_STEP: f32 = 0.1;
/// Volumes below this snap to 0 (0x82165A00).
const VOLUME_SNAP: f32 = 0.05;

/// What the screen asked of the game, in order (applied by the menu stage).
#[derive(Clone, Debug, PartialEq)]
pub enum MusicPlayerAction {
    Mode(MusicMode),
    Volume(f32),
    /// Play this song now (sub_825E44F8).
    PlaySong(u32),
    Next,
    Previous,
    /// Back: close the screen; `save` when anything changed (sub_8260BCF8 saves the profile then).
    Close { save: bool },
}

#[derive(Clone, Default)]
pub struct MusicPlayerHost {
    /// The screen is on top (its natives and keys are ours).
    pub open: bool,
    /// [artist, title, album] per soundtrack song, index order (song id = index + 1).
    pub songs: Vec<[String; 3]>,
    pub playlist: Vec<u32>,
    pub mode: MusicMode,
    pub volume: f32,
    /// The playing soundtrack song's id.
    pub playing: Option<u32>,
    /// Screen +0x44: 0 the Ambient tab, 1 the Music Player tab (its playlist).
    pub tab: i32,
    /// Screen +0x48 (`SetEditMode`).
    pub edit: bool,
    /// Screen +0x4C: the highlighted row (`Game.OnMenuHilight`).
    pub selection: i32,
    /// Screen +0x54: something changed (saved on Back).
    pub dirty: bool,
    pub states: Vec<i32>,
    pub sounds: Vec<u64>,
    pub actions: Vec<MusicPlayerAction>,
}

impl MusicPlayerHost {
    /// Opened: the tab follows the saved mode (NOT RETAIL YET: the screen's starting tab is not read).
    pub fn open_with(&mut self, songs: Vec<[String; 3]>, playlist: Vec<u32>, mode: MusicMode, volume: f32) {
        *self = Self { open: true, tab: i32::from(mode == MusicMode::Player), songs, playlist, mode, volume, ..Default::default() };
    }

    pub fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if !self.open {
            return Ok(None);
        }
        let int = |i: usize| args.get(i).map_or(-1, |v| i64::from(to_integer(v)));
        if object == "Game" && method == "OnMenuHilight" {
            self.selection = int(1) as i32;
            return Ok(Some(Value::Undefined));
        }
        if !object.eq_ignore_ascii_case("MusicPlayer") {
            return Ok(None);
        }
        let count = self.songs.len() as u32;
        let ids = |vm: &mut Vm, ids: Vec<u32>| -> Result<Value, String> {
            Ok(Value::Object(vm.array(ids.into_iter().map(|id| Value::Number(f64::from(id))).collect())?))
        };
        // APT names are case-insensitive (the movie calls GetPlayList, retail registers GetPlaylist).
        Ok(Some(match method.to_ascii_lowercase().as_str() {
            "getdefaultsonglist" => ids(vm, (1..=count).collect())?,
            "getplaylist" => ids(vm, self.playlist.clone())?,
            "getnumplaylists" => Value::Number(1.0),
            "getsonginfo" => self.info(vm, u32::try_from(int(0)).ok())?,
            "getcurrentsonginfo" => self.info(vm, self.playing)?,
            "getmusicplayersong" | "getcurrentsongid" => Value::Number(self.playing.map_or(0.0, f64::from)),
            "getcurrentsongindex" => Value::Number(self.playing.map_or(-1.0, |id| f64::from(id) - 1.0)),
            "issonginplaylist" => Value::Bool(song_id(int(0), count).is_some_and(|id| self.playlist.contains(&id))),
            "togglesonginplaylist" => Value::Bool(self.toggle(int(0))),
            "seteditmode" => {
                let on = args.first().is_some_and(|v| to_integer(v) != 0);
                self.set_edit(on);
                if !on {
                    self.sounds.push(SOUND_EDIT_DONE);
                }
                Value::Undefined
            }
            "setmusicmode" => {
                if let Some(mode) = MusicMode::from_code(int(0)) {
                    self.set_mode(mode);
                }
                Value::Undefined
            }
            "getmusicvolume" => Value::Number(f64::from(self.volume)),
            "setmusicvolume" => {
                let v = args.first().map_or(f64::from(self.volume), Value::number) as f32;
                self.volume = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
                self.actions.push(MusicPlayerAction::Volume(self.volume));
                Value::Undefined
            }
            "playnextsong" => {
                self.actions.push(MusicPlayerAction::Next);
                Value::Undefined
            }
            "playprevsong" => {
                self.actions.push(MusicPlayerAction::Previous);
                Value::Undefined
            }
            // NOT RETAIL YET (not read): Play / Stop / PlayPlaylist, shuffle, playlist order, saved playlists.
            "getisshuffle" => Value::Bool(true),
            "play" | "playplaylist" | "stop" | "setisshuffle" | "setplaylistorder" | "setnumsongsinplaylist" | "restoreplaylist" => Value::Undefined,
            "getcurrentplaylistid" => Value::Number(0.0),
            _ => return Ok(None),
        }))
    }

    /// The screen's key handler (sub_8260B6F0); true = consumed (the movie does not get the key).
    pub fn key(&mut self, key: u32) -> bool {
        match key {
            KEY_BACK => {
                if self.edit {
                    self.set_edit(false);
                }
                self.sounds.push(SOUND_BACK);
                self.actions.push(MusicPlayerAction::Close { save: self.dirty });
                true
            }
            KEY_SELECT => {
                if self.tab != 1 {
                    return false;
                }
                if self.edit {
                    self.sounds.push(SOUND_BACK);
                    if let Some(id) = song_id(i64::from(self.selection), self.songs.len() as u32) {
                        self.playing = Some(id);
                        self.actions.push(MusicPlayerAction::PlaySong(id));
                    }
                }
                true
            }
            KEY_TOGGLE => {
                if self.tab != 1 || !self.edit {
                    return false;
                }
                self.sounds.push(SOUND_TOGGLE);
                self.toggle(i64::from(self.selection));
                // The playlist is kept in soundtrack order after a toggle.
                self.playlist.sort_unstable();
                self.dirty = true;
                false
            }
            KEY_LEFT | KEY_RIGHT if !self.edit => {
                let row = usize::try_from(self.selection).ok().filter(|&r| r < 4);
                let kind = usize::try_from(self.tab).ok().and_then(|t| ROW_KINDS.get(t)).zip(row).map(|(kinds, r)| kinds[r]);
                if kind != Some(2) {
                    return false;
                }
                let up = key == KEY_RIGHT;
                self.sounds.push(if up { SOUND_VOLUME_UP } else { SOUND_VOLUME_DOWN });
                let mut v = (self.volume + if up { VOLUME_STEP } else { -VOLUME_STEP }).clamp(0.0, 1.0);
                if v < VOLUME_SNAP {
                    v = 0.0;
                }
                self.volume = v;
                self.actions.push(MusicPlayerAction::Volume(v));
                self.states.push(match self.tab {
                    1 => 0x3C,
                    0 => 0x3D,
                    _ => 0x3E,
                });
                self.dirty = true;
                true
            }
            KEY_PLAYER_TAB => {
                if self.tab == 1 {
                    return true;
                }
                if self.edit {
                    self.edit = false;
                } else if !self.change_mode(MusicMode::Player) {
                    self.sounds.push(SOUND_TAB);
                    return true;
                }
                self.tab = 1;
                self.states.extend([1, 0x3F]);
                self.sounds.push(SOUND_TAB);
                true
            }
            KEY_AMBIENT_TAB => {
                if self.edit {
                    self.edit = false;
                    self.change_mode(MusicMode::Ambient);
                } else if self.tab == 0 {
                    return true;
                } else if !self.change_mode(MusicMode::Ambient) {
                    self.sounds.push(SOUND_TAB);
                    return true;
                }
                self.tab = 0;
                self.states.push(0);
                self.sounds.push(SOUND_TAB);
                true
            }
            _ => false,
        }
    }

    /// `SetEditMode` (sub_8260BC50): leaving the edit on the Ambient tab sets the Ambient mode; the movie is told
    /// to refresh (state 2 entering, 0x3F leaving the Music Player tab's edit).
    fn set_edit(&mut self, on: bool) {
        self.edit = on;
        if on {
            self.states.push(2);
        } else {
            if self.tab == 0 {
                self.change_mode(MusicMode::Ambient);
            }
            self.states.push(self.tab);
            if self.tab == 1 {
                self.states.push(0x3F);
            }
        }
    }

    /// sub_8260BBE0: the mode changes only when it differs from the saved one.
    fn change_mode(&mut self, mode: MusicMode) -> bool {
        let changed = mode != self.mode;
        if changed {
            self.set_mode(mode);
            self.dirty = true;
        }
        changed
    }

    fn set_mode(&mut self, mode: MusicMode) {
        self.mode = mode;
        self.actions.push(MusicPlayerAction::Mode(mode));
    }

    fn toggle(&mut self, index: i64) -> bool {
        match song_id(index, self.songs.len() as u32) {
            Some(id) => {
                let listed = toggle(&mut self.playlist, id);
                self.dirty = true;
                listed
            }
            None => false,
        }
    }

    /// `GetSongInfo` (sub_82D147F8): [artist, title, album] of song `id`; empty strings for an unknown song.
    fn info(&self, vm: &mut Vm, id: Option<u32>) -> Result<Value, String> {
        let row = id.and_then(|id| self.songs.get((id as usize).checked_sub(1)?)).cloned().unwrap_or_default();
        Ok(Value::Object(vm.array(row.into_iter().map(Value::Text).collect())?))
    }
}

/// The bridge between the screen (menu stage) and the music (game audio): what the screen shows when it opens, and
/// what it asked. A mod can read it too (the saved playlist and mode live in the audio settings).
#[derive(bevy::prelude::Resource, Clone, Default, Debug)]
pub struct MusicPlayerLink {
    /// The saved playlist and mode are loaded (once the soundtrack is known).
    pub loaded: bool,
    /// Soundtrack track count.
    pub songs: u32,
    pub playlist: Vec<u32>,
    pub mode: MusicMode,
    /// Game Settings' music volume (the same float, options block +0x88).
    pub volume: f32,
    pub playing: Option<u32>,
    /// Applied by `game_audio::music_player_link` next frame.
    pub actions: Vec<MusicPlayerAction>,
    /// Save the playlist and mode (Back after a change).
    pub save: bool,
}

/// [artist, title, album] for each soundtrack song from menu-tables `ipod_tracks` and the language table
/// (`IPOD_<NAME>_ARTIST` / `_TITLE` / `_ALBUM`, upper case); a missing title shows the name itself.
pub fn song_rows(tracks: &[String], language: &BTreeMap<String, String>) -> Vec<[String; 3]> {
    tracks
        .iter()
        .map(|name| {
            let key = |part: &str| format!("IPOD_{}_{part}", name.to_ascii_uppercase());
            let text = |part: &str| language.get(&key(part)).cloned();
            [text("ARTIST").unwrap_or_default(), text("TITLE").unwrap_or_else(|| name.clone()), text("ALBUM").unwrap_or_default()]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> MusicPlayerHost {
        let mut h = MusicPlayerHost::default();
        let songs = (1..=3).map(|i| [format!("A{i}"), format!("T{i}"), format!("L{i}")]).collect();
        h.open_with(songs, vec![1, 2, 3], MusicMode::Player, 1.0);
        h.playing = Some(2);
        h
    }

    fn call(h: &mut MusicPlayerHost, vm: &mut Vm, method: &str, args: &[Value]) -> Value {
        h.call(vm, "MusicPlayer", method, args).unwrap().unwrap()
    }

    #[test]
    fn the_toggle_key_in_edit_changes_the_list_in_soundtrack_order_and_back_saves() {
        let mut vm = Vm::new();
        let mut h = host();
        call(&mut h, &mut vm, "SetEditMode", &[Value::Bool(true)]);
        h.call(&mut vm, "Game", "OnMenuHilight", &[Value::Undefined, Value::Number(0.0)]).unwrap();
        assert!(!h.key(KEY_TOGGLE), "the movie also gets the key");
        assert_eq!(h.playlist, [2, 3]);
        assert!(matches!(call(&mut h, &mut vm, "IsSongInPlayList", &[Value::Number(0.0)]), Value::Bool(false)));
        h.key(KEY_TOGGLE);
        assert_eq!(h.playlist, [1, 2, 3]);
        assert!(h.key(KEY_BACK));
        assert_eq!(h.actions.last(), Some(&MusicPlayerAction::Close { save: true }));
        assert_eq!(h.sounds, [SOUND_TOGGLE, SOUND_TOGGLE, SOUND_BACK]);
    }

    #[test]
    fn tabs_set_the_mode_only_when_it_changes() {
        let mut h = host();
        assert!(h.key(KEY_AMBIENT_TAB));
        assert_eq!((h.tab, h.mode), (0, MusicMode::Ambient));
        assert!(h.key(KEY_PLAYER_TAB));
        assert_eq!((h.tab, h.mode), (1, MusicMode::Player));
        assert_eq!(h.actions, [MusicPlayerAction::Mode(MusicMode::Ambient), MusicPlayerAction::Mode(MusicMode::Player)]);
        assert_eq!(h.states, [0, 1, 0x3F]);
    }

    #[test]
    fn left_and_right_step_the_volume_on_the_volume_row_and_snap_to_zero() {
        let mut h = host();
        h.volume = 0.12;
        assert!(h.key(KEY_LEFT));
        assert_eq!(h.volume, 0.0);
        assert!(h.key(KEY_RIGHT));
        assert!((h.volume - 0.1).abs() < 1e-6);
        h.selection = 1;
        assert!(!h.key(KEY_RIGHT), "row 1 of the Music Player tab is the song list");
        assert_eq!(h.states, [0x3C, 0x3C]);
    }

    #[test]
    fn select_in_edit_plays_the_highlighted_song() {
        let mut vm = Vm::new();
        let mut h = host();
        call(&mut h, &mut vm, "SetEditMode", &[Value::Bool(true)]);
        h.selection = 2;
        assert!(h.key(KEY_SELECT));
        assert_eq!(h.actions, [MusicPlayerAction::PlaySong(3)]);
        assert!(matches!(call(&mut h, &mut vm, "GetMusicPlayerSong", &[]), Value::Number(n) if n == 3.0));
    }

    #[test]
    fn names_are_case_insensitive_and_closed_screens_answer_nothing() {
        let mut vm = Vm::new();
        let mut h = host();
        assert!(h.call(&mut vm, "MusicPlayer", "GetPlayList", &[]).unwrap().is_some());
        h.open = false;
        assert!(h.call(&mut vm, "MusicPlayer", "GetPlayList", &[]).unwrap().is_none());
    }

    #[test]
    fn song_rows_read_the_upper_case_language_keys() {
        let language = BTreeMap::from([
            ("IPOD_BAND_SONG_ARTIST".to_string(), "Band".to_string()),
            ("IPOD_BAND_SONG_TITLE".to_string(), "Song".to_string()),
        ]);
        assert_eq!(song_rows(&["Band_Song".into(), "Other".into()], &language), [
            ["Band".to_string(), "Song".into(), String::new()],
            [String::new(), "Other".into(), String::new()],
        ]);
    }
}
