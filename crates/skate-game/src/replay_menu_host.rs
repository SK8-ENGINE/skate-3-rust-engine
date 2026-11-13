//! The instant replay screen's natives (FE 12; b385 / b390 / b391) from a per-frame replay snapshot.
//! Details: docs/hails-additions/37-instant-replay.md#replay-screen-natives
use crate::apt_vm::{Value, Vm};

/// Movie states the C++ posts (b386 / b390; names from the movie's SetState): cursor moved (kMovePlayHead), play
/// start / stop (kStartPlaybackReplay / kStopPlaybackReplay), panel show / hide (kShowEditor / kHideEditor).
pub mod movie_state {
    pub const CURSOR: i32 = 999;
    pub const PLAY: i32 = 18;
    pub const STOP: i32 = 19;
    /// kShowEditor / kHideEditor (the replay movie's SetState, read from its actions 2026-10-20): the whole panel; the
    /// movie's own Hide / Show prompt follows.
    pub const SHOW: i32 = 1002;
    pub const HIDE: i32 = 1003;
    /// kShowToggle / kHideToggle: only the Hide / Show prompt itself (`ForceShowHideToggle`).
    pub const SHOW_TOGGLE: i32 = 21;
    pub const HIDE_TOGGLE: i32 = 22;
    /// kEffectsMode, kRefreshEffectsMenu (the row moved), kUpdateEffectValues (a value changed), kMainEditMode.
    pub const EFFECTS: i32 = 4;
    pub const REFRESH_EFFECTS: i32 = 5;
    pub const UPDATE_EFFECTS: i32 = 6;
    pub const MAIN_EDIT: i32 = 9;
    /// kMoveStartMarker / kMoveEndMarker (the in / out head moved), and the LB move state (b386: 1005).
    pub const MOVE_START: i32 = 1000;
    pub const MOVE_END: i32 = 1001;
    pub const LB_MOVE: i32 = 1005;
    /// The Create pack's screen (replay_editor3, b411; the movie sets these values itself): the edit channel
    /// changed (Y, kUpdateMode) and the tint component cursor moved (kRefreshTint).
    pub const UPDATE_MODE: i32 = 23;
    pub const REFRESH_TINT: i32 = 24;
}

/// What the screen reads from the replay each frame (local player only; a serialisable snapshot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayScreenFeed {
    /// The replay is open (instant or editor): the screen shows.
    pub active: bool,
    /// The replay is open in instant mode (retail FE 12 byte +0x82, `IsInstantReplayMode` 825ADFC0).
    pub instant: bool,
    /// Cursor and playhead as a fraction of the recording, 0..1.
    pub position: f64,
    pub playing: bool,
    /// The panel (prompts and timeline) is shown.
    pub panel: bool,
    /// The encoded length, s (`GetEncodedLength` 825AE488 reads it as a whole number): the recording in instant
    /// mode, the edit's `encoded_seconds` in the editor (b472).
    pub length_seconds: u32,
    /// The length is within the share limit (`IsEncodedLengthValid` 825ADF88; the limit is data,
    /// `EditLimits::share_max_seconds`).
    pub length_valid: bool,
    /// The help screen (FE 13, ReplayHelp) is open, and how many page turns were asked since the replay opened.
    pub help: bool,
    pub help_turns: u32,
    /// The editor mode (retail `+0x30`: 5 instant, 6 timeline, 2 trim, 11 effects, 1 playback), the in / out points
    /// in 30 fps frames, the effects row and the speed / shake values at the cursor (b398).
    pub mode: u8,
    /// The clip camera's menu row (`GetCurrentCameraEditIndex`; b401: 0 Follow, 1 Tripod, 2 Game, 3 Free).
    pub camera_row: u32,
    /// The timeline's camera keys as fractions of the recording (b403), and the band's segments (start fraction,
    /// camera type 0 Follow / 1 Tripod / 2 Free / 3 Game).
    pub keys: Vec<f32>,
    pub segments: Vec<(f32, u8)>,
    pub heads: (u32, u32),
    pub row: u32,
    pub speed: i32,
    pub shake: i32,
    /// The Create pack's screen (replay_editor3): the edit channel (`GetMode`), its raw value (`GetModeValue`) and
    /// slider notch (`GetModeSliderValue`), the tint cursor (`GetCurrentTint`) and the four tint components (raw).
    pub channel: u32,
    pub mode_value: i32,
    pub mode_slider: i32,
    pub tint: u32,
    pub tints: [i32; 4],
}

/// A tint component as the slider's notch 0..10 (retail snaps it to steps of 25 with k = 0.04, `8250B0B8`).
fn tint_notch(value: i32) -> f64 {
    (value as f32 * 0.04 + 0.5).trunc() as f64
}

/// The help movie's mode for `GetHelpMode` (its kInstantReplayMode = 0; the editor modes follow from 1).
pub const HELP_INSTANT_REPLAY: f64 = 0.0;
/// The help movie's SetState that turns the page (`NextHelpPage`).
pub const HELP_NEXT_PAGE: i32 = 0;

#[derive(Clone, Debug, Default)]
pub struct ReplayMenuHost {
    /// The screen is open (its natives are ours).
    pub open: bool,
    pub feed: ReplayScreenFeed,
    /// The feed last posted to the movie (edges become states).
    posted: Option<ReplayScreenFeed>,
    /// Screen states to post to the movie, in order.
    pub states: Vec<i32>,
    /// The movie called `FEReplayMenu.Ready`.
    pub ready: bool,
    /// The natives the movie called since the screen opened (diagnostics, tests).
    pub seen: Vec<String>,
    /// The help screen is open / shown (after its IntroComplete), and the page turns already posted to it.
    pub help_open: bool,
    pub help_shown: bool,
    pub help_turns_posted: u32,
}

impl ReplayMenuHost {
    pub fn reset(&mut self) {
        let feed = std::mem::take(&mut self.feed);
        *self = Self { feed, ..Default::default() };
    }

    /// Queue the states for what changed since the last post: play / stop and panel edges (never on the first post:
    /// the movie opens paused with its panel shown; a first-frame panel state hid it, verify run 2026-10-20), then
    /// the cursor.
    pub fn post_changes(&mut self) {
        let last = self.posted.clone();
        let feed = &self.feed;
        if last.as_ref().is_some_and(|l| l.playing != feed.playing) || (last.is_none() && feed.playing) {
            self.states.push(if feed.playing { movie_state::PLAY } else { movie_state::STOP });
        }
        if last.as_ref().is_some_and(|l| l.panel != feed.panel) {
            self.states.push(if feed.panel { movie_state::SHOW } else { movie_state::HIDE });
        }
        // Editor mode changes (b386): A from instant -> 21, 9; LB -> 1005, released -> 21; A -> effects 22, 4; B out
        // of the effects -> 21, 9.
        if let Some(l) = last.as_ref().filter(|l| l.mode != feed.mode) {
            use movie_state::*;
            match (l.mode, feed.mode) {
                (5, 6) | (11, 6) => self.states.extend([SHOW_TOGGLE, MAIN_EDIT]),
                (6, 2) => self.states.push(LB_MOVE),
                (2, 6) => self.states.push(SHOW_TOGGLE),
                (6, 11) => self.states.extend([HIDE_TOGGLE, EFFECTS]),
                // A on a camera row, or on a camera key in the timeline: the camera's key edit state (Follow 1, Tripod 2,
                // Game 10, Free 7; the advanced cameras 12 / 13 / 14, b405), from the camera row in effect; A commits
                // back to the timeline (21, 9).
                (11 | 6, 7..=10) => {
                    if let Some((_, state)) = crate::replay::camera_key::CameraKey::edit_mode_and_state(feed.camera_row as usize) {
                        self.states.push(state);
                    }
                }
                (7..=10, 6) => self.states.extend([SHOW_TOGGLE, MAIN_EDIT]),
                _ => {}
            }
        }
        if let Some(l) = last.as_ref().filter(|l| l.mode == 11 && feed.mode == 11) {
            if l.row != feed.row {
                self.states.push(movie_state::REFRESH_EFFECTS);
            }
            if l.tint != feed.tint {
                self.states.push(movie_state::REFRESH_TINT);
            }
            if l.speed != feed.speed || l.shake != feed.shake || l.mode_value != feed.mode_value || l.tints != feed.tints {
                self.states.push(movie_state::UPDATE_EFFECTS);
            }
        }
        if last.as_ref().is_some_and(|l| l.channel != feed.channel) {
            self.states.push(movie_state::UPDATE_MODE);
        }
        if let Some(l) = last.as_ref() {
            if l.heads.0 != feed.heads.0 {
                self.states.push(movie_state::MOVE_START);
            }
            if l.heads.1 != feed.heads.1 {
                self.states.push(movie_state::MOVE_END);
            }
        }
        if last.as_ref().is_none_or(|l| l.position != feed.position || l.length_seconds != feed.length_seconds) {
            self.states.push(movie_state::CURSOR);
        }
        self.posted = Some(self.feed.clone());
    }

    /// The length text: `ID_REPLAY_EDITOR_LENGTH` ("Estimated Video Length: {0:time}") with the length. Retail's `time`
    /// placeholder (handler 824FE900, b397) is the number (grouped by 3, `8290E370`) + " " + `ID_COMMON_METRIC_TIME`
    /// ("s"), not a clock. NOT RETAIL YET: the default decimals are not proven (0 assumed: "42 s").
    fn length_text(&self, language: &std::collections::BTreeMap<String, String>) -> String {
        let digits = self.feed.length_seconds.to_string();
        let mut grouped = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        let unit = crate::fe_language::template(language, "ID_COMMON_METRIC_TIME");
        let template = crate::fe_language::template(language, "ID_REPLAY_EDITOR_LENGTH");
        template.replacen("{0:time}", &format!("{grouped} {unit}"), 1)
    }

    /// One native call; None = not ours.
    pub fn call(&mut self, _vm: &mut Vm, language: &std::collections::BTreeMap<String, String>, object: &str, method: &str) -> Option<Value> {
        if object != "FEReplayMenu" || !self.open {
            return None;
        }
        if !self.seen.iter().any(|m| m == method) {
            self.seen.push(method.to_owned());
        }
        let feed = &self.feed;
        Some(match method {
            "Ready" => {
                self.ready = true;
                Value::Undefined
            }
            "IsInstantReplayMode" => Value::Bool(feed.instant),
            "RefreshCursor" => Value::Undefined,
            "GetCursorPosition" | "GetPlayheadPosition" => Value::Number(feed.position.clamp(0.0, 1.0)),
            "GetEncodedLength" => Value::Text(self.length_text(language)),
            "IsEncodedLengthValid" => Value::Bool(feed.length_valid),
            // Ours on PC: the photo item shows (b390 table 2).
            "CanSaveMedia" => Value::Bool(true),
            // Retail 825ADF00: in editor modes 5 (instant), 6 (timeline) and 1 (playback) (b470).
            "CanDisplayShowHide" => Value::Bool(matches!(feed.mode, 5 | 6 | 1)),
            // The help screen's mode: instant 0, the timeline 1 (kMainEditMode), the effects menu 2 (kEffectsMode).
            "GetHelpMode" => Value::Number(match feed.mode {
                6 | 1 | 2 => 1.0,
                11 => 2.0,
                _ => HELP_INSTANT_REPLAY,
            }),
            // The editor's natives (b398; AS integers): effects row, speed %, shake 0..3, the key's camera row (only
            // the game camera, row 2, is ported), the in / out points in 30 fps frames.
            "GetEditIndex" => Value::Number(f64::from(feed.row)),
            "GetSimpleSpeedSetting" => Value::Number(f64::from(feed.speed)),
            "GetShakeCamType" => Value::Number(f64::from(feed.shake)),
            "GetCurrentCameraEditIndex" => Value::Number(f64::from(feed.camera_row)),
            "GetStartHeadPosition" => Value::Number(f64::from(feed.heads.0)),
            "GetEndHeadPosition" => Value::Number(f64::from(feed.heads.1)),
            // NOT RETAIL YET: "Zoom {0:d} mm" with the camera lens (b376); the camera keys are not ported.
            "GetCameraZoomString" => Value::Text(String::new()),
            // The Create pack's screen (replay_editor3; b410 / b411): the edit channel and its label id, the channel's
            // raw value and slider notch, the tint cursor, the selected component raw, and each component as a notch
            // (snapped to 25, `8250B0B8`: (v * 0.04 + 0.5) truncated).
            "GetMode" => Value::Number(f64::from(feed.channel)),
            "GetModeText" => Value::Text(
                crate::replay::editor::CHANNELS.get(feed.channel as usize).map_or("ID_REPLAYEDITOR_CHANNEL_SETTING", |c| c.0).into(),
            ),
            "GetModeValue" => Value::Number(f64::from(feed.mode_value)),
            "GetModeSliderValue" => Value::Number(f64::from(feed.mode_slider)),
            "GetCurrentTint" => Value::Number(if feed.channel == 6 { f64::from(feed.tint) } else { -1.0 }),
            "GetCurrentTintValue" => Value::Number(if feed.channel == 6 { f64::from(feed.tints[feed.tint as usize % 4]) } else { -1.0 }),
            "GetCurrentSliderTintValue" => Value::Number(tint_notch(feed.tints[feed.tint as usize % 4])),
            "GetRedTintValue" => Value::Number(tint_notch(feed.tints[0])),
            "GetGreenTintValue" => Value::Number(tint_notch(feed.tints[1])),
            "GetBlueTintValue" => Value::Number(tint_notch(feed.tints[2])),
            "GetAlphaTintValue" => Value::Number(tint_notch(feed.tints[3])),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_pack_natives_and_states() {
        let mut vm = Vm::new();
        let language = std::collections::BTreeMap::new();
        let mut host = ReplayMenuHost { open: true, ..Default::default() };
        host.feed = ReplayScreenFeed { active: true, mode: 11, channel: 6, tint: 2, tints: [0, 0, 130, 250], mode_value: -1, mode_slider: 2, ..Default::default() };
        let call = |host: &mut ReplayMenuHost, vm: &mut Vm, name: &str| host.call(vm, &language, "FEReplayMenu", name);
        assert_eq!(call(&mut host, &mut vm, "GetMode"), Some(Value::Number(6.0)));
        assert_eq!(call(&mut host, &mut vm, "GetModeText"), Some(Value::Text("ID_REPLAYEDITOR_CHANNEL_TINT".into())));
        assert_eq!(call(&mut host, &mut vm, "GetCurrentTint"), Some(Value::Number(2.0)));
        assert_eq!(call(&mut host, &mut vm, "GetCurrentTintValue"), Some(Value::Number(130.0)));
        // 130 snaps to 125 (notch 5); 250 is notch 10.
        assert_eq!(call(&mut host, &mut vm, "GetCurrentSliderTintValue"), Some(Value::Number(5.0)));
        assert_eq!(call(&mut host, &mut vm, "GetAlphaTintValue"), Some(Value::Number(10.0)));
        host.post_changes();
        host.states.clear();
        host.feed.tint = 3;
        host.post_changes();
        assert!(host.states.contains(&movie_state::REFRESH_TINT));
        host.states.clear();
        host.feed.channel = 0;
        host.post_changes();
        assert!(host.states.contains(&movie_state::UPDATE_MODE));
        assert_eq!(call(&mut host, &mut vm, "GetCurrentTint"), Some(Value::Number(-1.0)));
    }

    #[test]
    fn natives_answer_from_the_feed_and_edges_become_states() {
        let mut vm = Vm::new();
        let language = std::collections::BTreeMap::from([
            ("ID_REPLAY_EDITOR_LENGTH".to_owned(), "Estimated Video Length: {0:time}".to_owned()),
            ("ID_COMMON_METRIC_TIME".to_owned(), "s".to_owned()),
        ]);
        let mut host = ReplayMenuHost { open: true, ..Default::default() };
        host.feed = ReplayScreenFeed { instant: true, position: 0.5, playing: false, panel: true, length_seconds: 61, mode: 5, ..Default::default() };
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "CanDisplayShowHide"), Some(Value::Bool(true)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetHelpMode"), Some(Value::Number(0.0)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsInstantReplayMode"), Some(Value::Bool(true)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetCursorPosition"), Some(Value::Number(0.5)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetEncodedLength"), Some(Value::Text("Estimated Video Length: 61 s".into())));
        // 825ADF88: valid under 61 s (decided by the feed from the edit limits).
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsEncodedLengthValid"), Some(Value::Bool(false)));
        host.feed.length_valid = true;
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsEncodedLengthValid"), Some(Value::Bool(true)));
        assert_eq!(host.call(&mut vm, &language, "Game", "IsInstantReplayMode"), None);
        host.post_changes();
        assert_eq!(std::mem::take(&mut host.states), vec![movie_state::CURSOR], "opens paused with its panel shown");
        host.post_changes();
        assert!(host.states.is_empty(), "nothing changed");
        host.feed.playing = true;
        host.feed.position = 0.6;
        host.post_changes();
        assert_eq!(host.states, vec![movie_state::PLAY, movie_state::CURSOR]);
        // L3: the panel hides with kHideEditor (1003) and comes back with kShowEditor (1002).
        host.states.clear();
        host.feed.panel = false;
        host.post_changes();
        assert_eq!(std::mem::take(&mut host.states), vec![movie_state::HIDE]);
        assert_eq!(movie_state::HIDE, 1003);
        host.feed.panel = true;
        host.post_changes();
        assert_eq!(std::mem::take(&mut host.states), vec![movie_state::SHOW]);
        // The editor: A -> timeline (21, 9), A -> effects (22, 4), a row move (5), a value change (6), B back (21, 9).
        host.feed.mode = 5;
        host.post_changes();
        host.states.clear();
        for (mode, row, speed, expect) in [
            (6u8, 0u32, 100i32, vec![movie_state::SHOW_TOGGLE, movie_state::MAIN_EDIT]),
            (11, 0, 100, vec![movie_state::HIDE_TOGGLE, movie_state::EFFECTS]),
            (11, 7, 100, vec![movie_state::REFRESH_EFFECTS]),
            (11, 7, 90, vec![movie_state::UPDATE_EFFECTS]),
            (6, 7, 90, vec![movie_state::SHOW_TOGGLE, movie_state::MAIN_EDIT]),
        ] {
            host.feed.mode = mode;
            host.feed.row = row;
            host.feed.speed = speed;
            host.post_changes();
            assert_eq!(std::mem::take(&mut host.states), expect, "mode {mode} row {row} speed {speed}");
        }
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetSimpleSpeedSetting"), Some(Value::Number(90.0)));
        host.open = false;
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "Ready"), None, "closed: not ours");
    }
}
