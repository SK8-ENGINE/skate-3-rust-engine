//! A rolling presentation recording, never a second simulation or a rewind of
//! live state. Root, skin (including the board), and camera share one playhead.
use crate::{
    app::SimulationSet,
    presentation::{Snapshot, blend},
};
use bevy::prelude::*;
use std::collections::VecDeque;

/// Retail records one frame per 60 Hz sim step, so its 5400-frame cap is a 90 s window (b345, REPLAYREC count run).
const RECORD_HZ: f64 = 60.0;
const SELECT: u16 = 0x0020;
const B: u16 = 0x2000;
const RB: u16 = 0x0200;
const DPAD_LEFT: u16 = 0x0004;
const DPAD_RIGHT: u16 = 0x0008;
const START: u16 = 0x0010;
const L3: u16 = 0x0040;
const A: u16 = 0x1000;
const LB: u16 = 0x0100;
const DPAD_UP: u16 = 0x0001;
const DPAD_DOWN: u16 = 0x0002;
/// The quit warning's callback id (engine popup channel, `popup_host::PopupMods`).
const REPLAY_POPUP_PREFIX: &str = "engine:replay.";
const QUIT_POPUP: &str = "engine:replay.quit";
const PHOTO_POPUP: &str = "engine:replay.photo";
const PHOTO_DONE_POPUP: &str = "engine:replay.photo_saved";
const MARKER_POPUP: &str = "engine:replay.marker";
const X: u16 = 0x4000;
const Y: u16 = 0x8000;
/// The fe record retail plays when the instant replay opens (key 0x394015B68783BE9E; data, a mod may remap it).
const OPEN_SOUND: &str = "editor_select";
/// Retail opens the replay only with more than 149 frames recorded (`8250B8F8`); read as frames of the editor's 30 fps
/// timeline, that is 5 s (the unit is inferred, research b343 / b389).
pub(crate) const MIN_OPEN_SECONDS: f64 = 150.0 / 30.0;

/// Retail X (Take Photo, `826147C8`, b386): stop, then a confirm popup (callback `82614940`). NOT RETAIL YET: the
/// popup's title and options are ours (Take Photo / Save Photo?, No / Yes); the strings are retail's.
fn photo_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAYEDITOR_TAKE_PHOTO".into(),
        description: "ID_REPLAY_EDITOR_POPUP_PHOTOSAVE".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: PHOTO_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// The film pack's Finished menu (b387 rows 2..6, b557): X opens it on the timeline; B goes to the quit warning.
const FILM_MENU_POPUP: &str = "engine:replay.film_menu";
const REPLAY_MODE_POPUP: &str = "engine:replay.replay_mode";
const RESET_POPUP: &str = "engine:replay.reset";
const TAKE_PHOTO_ROW: &str = "ID_REPLAYEDITOR_TAKE_PHOTO";
const REPLAY_MODE_ROW: &str = "ID_REPLAY_EDITOR_REPLAY_MODE";
const RESET_CHANNEL_ROW: &str = "ID_REPLAY_EDITOR_RESET_TIMELINE_CURRENT_CHANNEL";
const RESET_ALL_ROW: &str = "ID_REPLAY_EDITOR_RESET_TIMELINE";
/// The replay mode popup's toggles in retail order (`82615728`): greenscreen, player visible, skaters visible.
const REPLAY_MODE_ROWS: [&str; 3] = [
    "ID_REPLAY_EDITOR_REPLAY_MODE_GREENSCREEN",
    "ID_REPLAY_EDITOR_REPLAY_MODE_PLAYER_VISIBLE",
    "ID_REPLAY_EDITOR_REPLAY_MODE_SKATERS_VISIBLE",
];

/// The film menu's rows (data, chosen by id): our Save to PC in retail's Save slot when the exporter is there, then
/// Take Photo, Replay Mode and the two resets (retail shows the last three only offline, which ours always is).
fn film_menu_rows(export: bool) -> Vec<&'static str> {
    let mut rows = Vec::new();
    if export {
        rows.push(export_run::SAVE_TO_PC);
    }
    rows.extend([TAKE_PHOTO_ROW, REPLAY_MODE_ROW, RESET_CHANNEL_ROW, RESET_ALL_ROW]);
    rows
}

fn film_menu_popup(export: bool) -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAYEDITOR_TITLE".into(),
        description: "#".into(),
        options: film_menu_rows(export).into_iter().map(Into::into).collect(),
        callback: FILM_MENU_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

fn replay_mode_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: REPLAY_MODE_ROW.into(),
        description: REPLAY_MODE_ROW.into(),
        options: REPLAY_MODE_ROWS.iter().map(|r| (*r).into()).collect(),
        callback: REPLAY_MODE_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// A reset's confirm (b387 rows 5 / 6: No reopens the Finished menu, Yes resets).
fn reset_popup(all: bool) -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: (if all { RESET_ALL_ROW } else { RESET_CHANNEL_ROW }).into(),
        description: (if all { "ID_REPLAY_EDITOR_RESET_TIMELINE_CONFRIM" } else { "ID_REPLAY_EDITOR_RESET_TIMELINE_CURRENT_CHANNEL_CONFIRM" }).into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: RESET_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// The film pack's replay mode (b557: retail renderer flags +0x347AC / +0x347B4 / +0x347B0, set only through events
/// 0x6BD9CB5A / 0xF81BE84E / 0x02217C9B): local view state, back to the defaults when the replay closes (`82610828`).
/// Greenscreen draws only the characters over a flat color; the other two hide the player or every other character.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ReplayViewMode {
    pub greenscreen: bool,
    pub player: bool,
    pub skaters: bool,
    /// The greenscreen clear color (retail's value at the renderer's +0x7DF0 is not read yet; data).
    pub greenscreen_colour: [f32; 3],
}

impl Default for ReplayViewMode {
    fn default() -> Self {
        Self { greenscreen: false, player: true, skaters: true, greenscreen_colour: [0.0, 1.0, 0.0] }
    }
}

impl ReplayViewMode {
    /// One toggle of the replay mode popup (retail flips the flag: new = old == 0).
    pub(crate) fn toggle(&mut self, row: usize) {
        match row {
            0 => self.greenscreen = !self.greenscreen,
            1 => self.player = !self.player,
            2 => self.skaters = !self.skaters,
            _ => return,
        }
        info!("REPLAY_VIEW greenscreen={} player={} skaters={}", self.greenscreen, self.player, self.skaters);
    }

    /// Whether an actor of `kind` (the top byte of its replay id) is drawn: characters (NPC skaters 0, peds 1) follow
    /// the skaters flag, everything else is hidden under greenscreen (`827F9D40`, b557).
    pub(crate) fn shows(&self, kind: u64) -> bool {
        if kind <= 1 { self.skaters } else { !self.greenscreen }
    }
}

/// Which film popup is waiting for its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilmAsk {
    Menu,
    ReplayMode,
    Reset { all: bool },
}

fn photo_saved_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAYEDITOR_TAKE_PHOTO".into(),
        description: "ID_REPLAY_EDITOR_SAVE_PHOTO_COMPLETE".into(),
        options: vec!["ID_COMMON_OK".into()],
        callback: PHOTO_DONE_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// The photo flow: asked (confirm popup up), then the capture is requested once Yes was chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Photo {
    #[default]
    Idle,
    Confirm,
    Capture,
}

/// Where photos go: `SKATE_PHOTO_DIR`, else `Pictures\Skate 3 Rust` in the user's profile (ours on PC; retail saves
/// to My Media).
fn photo_path() -> std::path::PathBuf {
    let dir = std::env::var_os("SKATE_PHOTO_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Pictures").join("Skate 3 Rust")
    });
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    dir.join(format!("photo-{stamp}.png"))
}

/// Saves the world image (no UI: `graphics_menu::SceneTarget`) once a photo was confirmed.
fn take_photo(
    mut commands: Commands,
    mut replay: ResMut<Replay>,
    target: Option<Res<crate::graphics_menu::SceneTarget>>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
) {
    if replay.photo != Photo::Capture {
        return;
    }
    replay.photo = Photo::Idle;
    let Some(target) = target else { return };
    let path = photo_path();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!("REPLAY_PHOTO could not create {}: {e}", dir.display());
            return;
        }
    }
    info!("REPLAY_PHOTO saving {} at {:.2}s", path.display(), replay.cursor);
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::image(target.0.clone()))
        .observe(bevy::render::view::screenshot::save_to_disk(path));
    popups.engine_requests.push(photo_saved_popup());
}

/// Retail's quit warning (research b387: ID_COMMON_WARNING_TITLE / ID_REPLAY_EDITOR_QUIT, No / Yes).
fn quit_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: "ID_REPLAY_EDITOR_QUIT".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: QUIT_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// A marker change waiting for its confirm popup (b407): D-pad Up's delete (the kind decides the question) or D-pad
/// Down's drop inside a run of soft keys; Yes applies it at `time`.
#[derive(Clone, Copy, Debug)]
struct MarkerAsk {
    delete: Option<editor::MarkerKind>,
    time: f64,
    shown: bool,
    /// The film pack's channels are on (a clip delete resets only the lane values at the cursor, b460).
    channels: bool,
}

/// Retail's marker questions (b407, strings from the movie's language table; No / Yes): delete a clip marker
/// (`82613808`), delete a key marker (`826138C0`), drop a clip marker inside soft keys (`82613878`).
fn marker_popup(delete: Option<editor::MarkerKind>) -> skate_core::fe_popup::PopupRequest {
    let (title, description) = match delete {
        Some(editor::MarkerKind::Clip) => ("ID_REPLAY_EDITOR_DELETE_CLIP", "ID_REPLAY_EDITOR_DELETE_CLIP_DESC"),
        Some(editor::MarkerKind::Key) => ("ID_REPLAY_EDITOR_DELETE_KEY", "ID_REPLAY_EDITOR_DELETE_KEY_DESC"),
        None => ("ID_REPLAY_SAVEWARNING_TITLE", "ID_REPLAY_EDITOR_POPUP_CLIPMARKER"),
    };
    skate_core::fe_popup::PopupRequest {
        title: title.into(),
        description: description.into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: MARKER_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

const CAMERA_POPUP: &str = "engine:replay.change_camera";

#[derive(Clone, Copy, Debug)]
struct CameraAsk {
    row: usize,
    time: f64,
    view: Transform,
    anchors: camera_key::Anchors,
    shown: bool,
}

/// Retail's replace confirm (`82614148`, callback `826142F8`, b609): "Change Camera", No / Yes; Yes replaces.
fn camera_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_REPLAY_EDITOR_REPLACE_CAMERA_TITLE".into(),
        description: "ID_REPLAY_EDITOR_REPLACE_CAMERA_DESC".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: CAMERA_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// A camera row's pick, then that key's edit mode.
fn pick_camera(editor: &mut editor::Editor, row: usize, time: f64, view: &Transform, anchors: &camera_key::Anchors) {
    if let Some(mode) = editor.pick_camera(row, time, view, anchors) {
        info!("REPLAY_CAMERA key new row={row} {} view={:?} root={:?}", editor.editing_line().unwrap_or_default(),
            view.translation.to_array(), anchors.root.translation.to_array());
        use skate_core::replay_edit::session::EditorMode;
        editor.session.mode = match mode {
            7 => EditorMode::FollowKey,
            8 => EditorMode::TripodKey,
            9 => EditorMode::GameKey,
            _ => EditorMode::FreeKey,
        };
    }
}

/// Yes to a marker question: the delete or the drop at the cursor it was asked at.
fn apply_marker_ask(replay: &mut Replay, ask: MarkerAsk) {
    let Some(editor) = replay.editor.as_mut() else { return };
    if ask.delete.is_some() {
        editor.remove_marker(ask.time, ask.channels);
    } else if let Err(e) = editor.drop_marker(ask.time, ask.channels) {
        info!("REPLAY_EDITOR marker refused: {e:?}");
    }
}

struct Frame {
    time: f64,
    pose: Snapshot,
    cut: bool,
    /// The player's audio step of this frame (`Cues::raw`): playback publishes it again, as retail's audio runs on
    /// the recorded skater state (provider id 4, b375).
    riding: Option<crate::game_audio::skate_events::Riding>,
}

/// At most this many recorded audio steps go to the host in one frame (a fast scrub merges the rest's pulses into
/// these, like the host's own step cap).
const MAX_FED_STEPS: usize = 8;

#[derive(Resource, Default)]
pub(crate) struct Replay {
    frames: VecDeque<Frame>,
    pub active: bool,
    pub free_camera: Option<Transform>,
    /// The panel is hidden (retail L3: kHideEditor / kShowEditor), the help screen is open (retail Start: FE 13) and
    /// how many help page turns were asked (A); all reset when the replay opens.
    pub panel_hidden: bool,
    pub help_open: bool,
    pub help_turns: u32,
    /// The Take Photo flow (retail X).
    pub(crate) photo: Photo,
    /// The editor (A from the instant replay); None = instant replay.
    pub(crate) editor: Option<editor::Editor>,
    /// A "Save to PC" export is rendering: the cursor is the exporter's, the controls are off.
    pub(crate) exporting: bool,
    /// Retail's shake clips (setup: `private/hud/replay-shake/replay_shake.json`); empty until a refresh adds them,
    /// which leaves the Shake row without effect.
    pub(crate) shake_clips: std::sync::Arc<skate_core::replay_edit::shake::ShakeClips>,
    /// The shake levels (retail's by default; data).
    pub(crate) shake_levels: Option<[Option<skate_core::replay_edit::shake::ShakeLevel>; 4]>,
    cursor: f64,
    playing: bool,
    previous_buttons: u16,
    release_controls: bool,
    /// Every other drawn actor ([`world`]), on the same clock as `frames`.
    pub(crate) world: skate_core::replay_buffer::ReplayBuffer,
    world_time: Option<f64>,
    /// Mod markers on the recording clock (retail provider id 10 holds up to 50).
    markers: VecDeque<Marker>,
    /// The mod whose `sdk.replay.set_limits` is in force (last writer wins).
    limits_owner: Option<String>,
    /// The editor's limits (retail defaults; a mod's `share_max_seconds` lands here). The editor and the instant
    /// replay's share check read the same value (b470 item 5).
    pub(crate) edit_limits: skate_core::replay_edit::EditLimits,
    /// The player's Extended Replays length, s (PC overlay, not retail); None = retail's window. A mod's limits win.
    user_seconds: Option<u32>,
    /// The cursor the audio feed last published up to, and whether it moved forward this frame.
    fed: Option<f64>,
    audible: bool,
    /// The replay cameras' leash and world collision (retail defaults; `sdk.replay.set_camera`).
    pub(crate) camera_collision: camera_collision::CameraCollision,
    /// The frame a Follow key's eye rides (retail: the gameplay camera, m346; `sdk.replay.set_camera{follow_eye}`).
    pub(crate) follow_eye: camera_key::FollowEye,
    /// A mod's `fill_section` (None: retail's, on; `CameraRules::fill_section`).
    pub(crate) fill_section: Option<bool>,
    /// The mod whose `sdk.replay.set_camera` is in force (last writer wins).
    camera_owner: Option<String>,
    /// The quit warning is up (waiting for its answer).
    quit_pending: bool,
    /// A film pack popup waiting for its answer.
    film_ask: Option<FilmAsk>,
    /// The film pack's replay mode (greenscreen, player / skaters visible).
    pub(crate) view: ReplayViewMode,
    /// A marker delete / drop waiting for its confirm popup.
    marker_ask: Option<MarkerAsk>,
    /// A camera pick that replaces a keyed section, waiting for "Change Camera" (b609).
    camera_ask: Option<CameraAsk>,
    /// B / Back asked for the quit warning; it opens when the button is released.
    quit_on_release: bool,
    /// Shortest recording the replay opens on (s); None = retail's [`MIN_OPEN_SECONDS`]. Data, mod-settable.
    pub(crate) min_open_seconds: Option<f64>,
    /// The trigger scrub (retail rates as data) and its eased rate.
    pub(crate) scrub_spec: skate_core::replay_edit::ScrubSpec,
    scrub: skate_core::replay_edit::ScrubFilter,
    /// The cursor jumped (entered, rewound, or skipped more than [`MAX_FED_STEPS`]): the host's camera velocity resets.
    jumped: bool,
}

struct Marker {
    time: f64,
    label: String,
    owner: String,
}

/// Retail's marker cap (provider id 10: up to 50 x 48 B, b363).
const MAX_MARKERS: usize = 50;

impl Replay {
    pub fn record(&mut self, pose: Snapshot, dt: f64, cut: bool) {
        if self.active || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let time = self.frames.back().map_or(0.0, |f| f.time + dt);
        self.frames.push_back(Frame { time, pose, cut, riding: None });
        // Keep one bracketing sample at the start for fractional-time scrubs.
        let window = self.window();
        while self.frames.len() > 2 && self.frames[1].time <= time - window {
            self.frames.pop_front();
        }
        let start = self.bounds().0;
        while self.markers.front().is_some_and(|m| m.time < start) {
            self.markers.pop_front();
        }
    }

    fn window(&self) -> f64 {
        f64::from(self.world.limits.max_frames) / RECORD_HZ
    }

    /// The scrubbable span: the player's frames, cut to the actors' window when the byte budget evicted it first.
    pub(crate) fn bounds(&self) -> (f64, f64) {
        let end = self.frames.back().map_or(0.0, |f| f.time);
        let world_start = self.world.bounds().map_or(f64::MIN, |(start, _)| start);
        (
            self.frames
                .front()
                .map_or(0.0, |f| f.time)
                .max(end - self.window())
                .max(world_start.min(end)),
            end,
        )
    }

    /// The player's audio step for the newest frame (once per frame, after the capture).
    pub(crate) fn attach_riding(&mut self, riding: crate::game_audio::skate_events::Riding) {
        if let Some(frame) = self.frames.back_mut().filter(|f| f.riding.is_none()) {
            frame.riding = Some(riding);
        }
    }

    /// Recorded audio steps crossed going forward since the last call; nothing while paused (retail mutes it).
    pub(crate) fn audio_steps(&mut self) -> Vec<crate::game_audio::skate_events::Riding> {
        if !self.active {
            self.fed = None;
            self.audible = false;
            return Vec::new();
        }
        let cursor = self.cursor;
        let previous = self.fed.replace(cursor);
        self.audible = previous.is_some_and(|p| cursor > p);
        self.jumped = previous.is_none_or(|p| cursor < p);
        let Some(previous) = previous.filter(|p| *p != cursor) else { return Vec::new() };
        if cursor < previous {
            let at = self.frames.partition_point(|f| f.time <= cursor).saturating_sub(1);
            return self.frames.get(at).and_then(|f| f.riding).map(|mut r| {
                r.audio.push_trigger = false;
                vec![r]
            }).unwrap_or_default();
        }
        let from = self.frames.partition_point(|f| f.time <= previous);
        let to = self.frames.partition_point(|f| f.time <= cursor);
        let steps: Vec<_> = self.frames.range(from..to).filter_map(|f| f.riding).collect();
        if steps.len() <= MAX_FED_STEPS {
            return steps;
        }
        // Keep the newest steps; the skipped ones' pushes land on the first one kept.
        self.jumped = true;
        let (skipped, kept) = steps.split_at(steps.len() - MAX_FED_STEPS);
        let mut kept = kept.to_vec();
        kept[0].audio.push_trigger |= skipped.iter().any(|r| r.audio.push_trigger);
        kept
    }

    pub(crate) fn jumped(&self) -> bool {
        self.jumped
    }

    /// The replay moved forward this frame (its sound plays); false while paused or rewinding.
    pub(crate) fn audible(&self) -> bool {
        self.audible
    }

    /// `sdk.replay.marker`: a marker at the newest frame; refused when nothing is recorded or the cap is reached.
    pub(crate) fn add_marker(&mut self, owner: &str, label: String) -> Result<(), String> {
        let time = self.frames.back().ok_or("replay_marker: nothing recorded yet")?.time;
        if self.markers.len() >= MAX_MARKERS {
            return Err(format!("replay_marker: {MAX_MARKERS} markers reached"));
        }
        self.markers.push_back(Marker { time, label, owner: owner.to_string() });
        Ok(())
    }

    /// The limits without a mod: retail's, or the player's Extended Replays length with the byte budget scaled by
    /// the same factor (90 s of a busy world filled ~125 MB of the 128 MB budget, laptop session 2026-10-20).
    fn base_limits(&self) -> skate_core::replay_buffer::ReplayLimits {
        let retail = skate_core::replay_buffer::ReplayLimits::default();
        match self.user_seconds {
            Some(seconds) => {
                let factor = f64::from(seconds) * RECORD_HZ / f64::from(retail.max_frames);
                skate_core::replay_buffer::ReplayLimits {
                    max_frames: (f64::from(seconds) * RECORD_HZ) as u32,
                    byte_budget: (retail.byte_budget as f64 * factor) as usize,
                }
            }
            None => retail,
        }
    }

    /// Extended Replays: the player's length (None = retail). Applies at once unless a mod's limits are in force.
    pub(crate) fn set_user_length(&mut self, seconds: Option<u32>) {
        if self.user_seconds == seconds {
            return;
        }
        self.user_seconds = seconds;
        if self.limits_owner.is_none() {
            self.world.limits = self.base_limits();
        }
        info!(
            "REPLAY_LENGTH user={seconds:?} max_frames={} byte_budget={} mod={:?}",
            self.world.limits.max_frames, self.world.limits.byte_budget, self.limits_owner
        );
    }

    /// `sdk.replay.set_limits`: both absent restores the defaults (retail, or the player's Extended Replays).
    pub(crate) fn set_limits(&mut self, owner: &str, max_frames: Option<u32>, byte_budget: Option<u64>, share_max_seconds: Option<u32>) {
        let defaults = self.base_limits();
        if max_frames.is_none() && byte_budget.is_none() && share_max_seconds.is_none() {
            if self.limits_owner.as_deref() == Some(owner) {
                self.world.limits = defaults;
                self.edit_limits = Default::default();
                self.limits_owner = None;
            }
            return;
        }
        self.edit_limits.share_max_seconds = share_max_seconds.unwrap_or(skate_core::replay_edit::EditLimits::default().share_max_seconds);
        self.world.limits = skate_core::replay_buffer::ReplayLimits {
            max_frames: max_frames.unwrap_or(defaults.max_frames),
            byte_budget: byte_budget.map_or(defaults.byte_budget, |b| b as usize),
        };
        self.limits_owner = Some(owner.to_string());
    }

    /// `sdk.replay.set_camera`: the camera collision switch, radius and leash, and the Follow eye frame; all absent
    /// restores retail's.
    pub(crate) fn set_camera(&mut self, owner: &str, collide: Option<bool>, radius: Option<f32>, leash: Option<f32>,
        follow_eye: Option<camera_key::FollowEye>, fill_section: Option<bool>, playback_collide: Option<bool>) {
        let retail = camera_collision::CameraCollision::default();
        if collide.is_none() && radius.is_none() && leash.is_none() && follow_eye.is_none() && fill_section.is_none()
            && playback_collide.is_none() {
            if self.camera_owner.as_deref() == Some(owner) {
                self.camera_collision = retail;
                self.follow_eye = Default::default();
                self.fill_section = None;
                self.camera_owner = None;
            }
            return;
        }
        self.follow_eye = follow_eye.unwrap_or_default();
        self.fill_section = fill_section;
        self.camera_collision = camera_collision::CameraCollision {
            enabled: collide.unwrap_or(retail.enabled),
            radius: radius.unwrap_or(retail.radius),
            leash: leash.unwrap_or(retail.leash),
            playback: playback_collide.unwrap_or(retail.playback),
            ..retail
        };
        self.camera_owner = Some(owner.to_string());
    }

    /// A mod stopped: its markers, its limits and its camera rules go.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.markers.retain(|m| m.owner != owner);
        self.set_limits(owner, None, None, None);
        self.set_camera(owner, None, None, None, None, None, None);
    }

    /// Every mod stopped.
    pub(crate) fn clear_mods(&mut self) {
        self.markers.clear();
        self.world.limits = self.base_limits();
        self.edit_limits = Default::default();
        self.limits_owner = None;
        self.camera_collision = Default::default();
        self.follow_eye = Default::default();
        self.fill_section = None;
        self.camera_owner = None;
    }

    /// Read-only state for the mod snapshot (`replay_state`).
    pub(crate) fn state(&self) -> serde_json::Value {
        let (start, end) = self.bounds();
        serde_json::json!({
            "active": self.active,
            "cursor": if self.active { self.cursor } else { end },
            "start": start,
            "end": end,
            "frames": self.world.frame_count(),
            "bytes": self.world.bytes(),
            "max_frames": self.world.limits.max_frames,
            "byte_budget": self.world.limits.byte_budget,
            "markers": self.markers.iter()
                .map(|m| serde_json::json!({"time": m.time, "label": m.label, "owner": m.owner}))
                .collect::<Vec<_>>(),
        })
    }

    /// Retail's open check for Back and `sdk.replay.open`: enough recorded (`8250B8F8`).
    pub(crate) fn can_open(&self) -> bool {
        let (start, end) = self.bounds();
        !self.frames.is_empty() && end - start >= self.min_open_seconds.unwrap_or(MIN_OPEN_SECONDS)
    }

    pub(crate) fn enter(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        self.active = true;
        self.playing = false;
        self.free_camera = None;
        self.panel_hidden = false;
        self.help_open = false;
        self.help_turns = 0;
        self.photo = Photo::Idle;
        self.editor = None;
        self.cursor = self.bounds().1;
        // Logs must show when a reported replay happened and what it held.
        let (start, end) = self.bounds();
        info!(
            "REPLAY_OPEN span={:.1}s frames={} actors={} bytes={}",
            end - start,
            self.world.frame_count(),
            self.world.ids().count(),
            self.world.bytes()
        );
    }

    fn exit(&mut self) {
        if self.active {
            let (start, _) = self.bounds();
            info!("REPLAY_CLOSE at={:.1}s", self.cursor - start);
        }
        self.active = false;
        self.editor = None;
        self.marker_ask = None;
        self.camera_ask = None;
        self.quit_pending = false;
        self.film_ask = None;
        self.view = ReplayViewMode::default();
        self.quit_on_release = false;
        self.playing = false;
        self.free_camera = None;
        self.release_controls = true;
    }

    /// The editor's look at the playhead over the world grade (None: no replay or no editor open).
    pub(crate) fn look(&self, base: &skate_core::colour_matrix::ColourMatrixRecord) -> Option<skate_core::replay_edit::ReplayLook> {
        self.active.then_some(())?;
        self.editor.as_ref()?.look(self.cursor, base)
    }

    pub(crate) fn seek(&mut self, time: f64) {
        let (start, end) = self.bounds();
        self.cursor = time.clamp(start, end);
    }

    /// One recorded frame back or forward (retail's editor frame step, mode 0xD; the editor binds it).
    #[cfg_attr(not(test), allow(dead_code))]
    fn step(&mut self, direction: i32) {
        self.playing = false;
        let next = if direction > 0 {
            self.frames.iter().find(|f| f.time > self.cursor + 1e-6)
        } else {
            self.frames
                .iter()
                .rev()
                .find(|f| f.time < self.cursor - 1e-6)
        };
        if let Some(next) = next {
            self.seek(next.time);
        }
    }

    pub fn sample(&self) -> Option<(&Snapshot, &Snapshot, f32)> {
        if !self.active {
            return None;
        }
        let upper = self.frames.partition_point(|f| f.time <= self.cursor);
        let a = self.frames.get(upper.saturating_sub(1))?;
        let b = self.frames.get(upper).unwrap_or(a);
        // Teleports, camera cuts and cadence changes remain discrete.
        if b.cut || a.pose.bones.len() != b.pose.bones.len() {
            return Some((&a.pose, &a.pose, 0.0));
        }
        let alpha = if b.time > a.time {
            ((self.cursor - a.time) / (b.time - a.time)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Some((&a.pose, &b.pose, alpha))
    }

    fn toggle_camera(&mut self) {
        self.free_camera = if self.free_camera.is_some() {
            None
        } else {
            self.sample().map(|(a, b, t)| blend(a.camera, b.camera, t))
        };
    }
}

/// `REPLAY_EDITOR play` / `pause` when the replay starts or stops playing (laptop 2026-10-25 asked for it).
fn log_transport(replay: Res<Replay>, mut last: Local<Option<bool>>) {
    let now = replay.active.then_some(replay.playing);
    if now != *last {
        if let Some(playing) = now {
            info!("REPLAY_EDITOR {} cursor={:.2}", if playing { "play" } else { "pause" }, replay.cursor);
        }
        *last = now;
    }
}

pub(crate) struct ReplayPlugin;
impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        world::plugin(app);
        // The world actors' audio state follows the recording (b378 channels A1..A3), ghosts included (b670).
        components::track_on_ghosts::<crate::world_audio::TrafficAudio>(app);
        components::track_on_ghosts::<crate::world_audio::PedAudio>(app);
        components::track_on_ghosts::<crate::world_audio::NpcSkaterAudio>(app);
        components::track_on_ghosts::<crate::world_audio::AudioVelocity>(app);
        // The NPC skaters' and peds' fade (spawn fade-in, leave fade, camera-distance fade).
        components::track::<crate::living_world::npc_skaters::NpcFade>(app);
        // Their one-shot sounds (speech stays out: retail blocks speech in replay).
        // The horn / alarm as a recorded STATE, not one-shots (b409): a seek or rewind into a sounding one plays it.
        components::track_on_ghosts::<crate::world_audio::VehicleHornState>(app);
        messages::track::<crate::world_audio::PedTazerEvent>(app);
        messages::track::<crate::world_audio::PedBodyFallEvent>(app);
        app.init_resource::<Replay>()
            .add_systems(Update, apply_user_length)
            .configure_sets(
                FixedUpdate,
                (
                    SimulationSet::Input,
                    SimulationSet::Controls,
                    SimulationSet::Physics,
                )
                    .run_if(live),
            )
            .add_systems(PreUpdate, log_transport.after(controls))
            .add_systems(
                PreUpdate,
                controls
                    .after(crate::input::poll_controllers)
                    .after(bevy::input::InputSystems),
            )
            .add_systems(Startup, (setup_ui, load_shake))
            .init_resource::<ReplayFeed>()
            .init_resource::<RetailReplayUi>()
            .init_resource::<ReplayLookScreen>()
            .init_resource::<export_run::ReplayExport>()
            .add_systems(Update, (publish_feed, update_ui, take_photo, export_run::drive));
    }
}

fn live(replay: Res<Replay>) -> bool {
    !replay.active
}

/// Run condition: the living world's fixed steps freeze during a replay, as retail plays recorded data.
impl Replay {
    /// The playback cursor (s on the recording's clock).
    pub(crate) fn cursor(&self) -> f64 {
        self.cursor
    }
}

/// The PC overlay's Extended Replays setting into the recorder.
fn apply_user_length(menu: Option<Res<crate::graphics_menu::Menu>>, mut replay: ResMut<Replay>) {
    if let Some(menu) = menu.filter(|m| m.is_changed()) {
        replay.set_user_length(menu.replay_seconds());
    }
}

/// The editor's own keys (b386, modes 6 / 2 / 0xB); true = input used up: docs/hails-additions/37-instant-replay.md#editor-keys
fn editor_controls(
    replay: &mut Replay,
    channels: bool,
    pressed: u16,
    released: u16,
    pad: &crate::input::RawInput,
    keys: &ButtonInput<KeyCode>,
    dt: f32,
    sound: &mut Option<&'static str>,
    world: Option<&skate_core::physics::board_world::BoardWorld>,
) -> bool {
    use skate_core::replay_edit::session::EditorMode;
    let cursor = replay.cursor;
    let collision = replay.camera_collision;
    let playing = replay.playing;
    // The recorded camera and the frames the keys ride (Follow eye, skater root).
    let follow_eye = replay.follow_eye;
    let fill_section = replay.fill_section.unwrap_or(true);
    let view_root = replay.sample().map(|(a, b, t)| {
        let view = blend(a.camera, b.camera, t);
        (view, camera_key::Anchors::new(view, blend(a.root, b.root, t), follow_eye))
    });
    // Decided with the editor borrowed, applied to the replay after.
    let mut seek: Option<f64> = None;
    let mut play: Option<bool> = None;
    let mut ask: Option<MarkerAsk> = None;
    let mut camera_ask: Option<CameraAsk> = None;
    let Some(editor) = replay.editor.as_mut() else { return false };
    let (mode_before, channel_before) = (editor.session.mode, editor.channel);
    let used = match editor.session.mode {
        EditorMode::Effects => {
            if pressed & DPAD_UP != 0 || keys.just_pressed(KeyCode::ArrowUp) {
                editor.move_row(-1, channels);
            }
            if pressed & DPAD_DOWN != 0 || keys.just_pressed(KeyCode::ArrowDown) {
                editor.move_row(1, channels);
            }
            let step = i32::from(pressed & DPAD_RIGHT != 0 || keys.just_pressed(KeyCode::ArrowRight))
                - i32::from(pressed & DPAD_LEFT != 0 || keys.just_pressed(KeyCode::ArrowLeft));
            if step != 0 {
                editor.adjust_row(step);
            }
            // B closes the menu: the base game commits the live speed / shake there (`82507438(E, 0)`); the film pack
            // cancels (b461).
            if pressed & B != 0 || keys.just_pressed(KeyCode::Escape) {
                if !channels {
                    editor.commit_base(cursor);
                }
                editor.session.mode = EditorMode::Timeline;
            }
            // The film pack's commits on a lane row: A (a soft key next to soft keys, else the segment) and X (always a
            // soft key, not on shake); both back to the timeline (b461). Refused over a hard key.
            let lane_row = channels && editor.row_lane(editor.row).is_some();
            if lane_row && (pressed & X != 0 || keys.just_pressed(KeyCode::KeyX)) && editor.commit_pack(cursor, true) {
                editor.session.mode = EditorMode::Timeline;
            }
            if lane_row && (pressed & A != 0 || keys.just_pressed(KeyCode::Enter)) {
                if editor.commit_pack(cursor, false) {
                    editor.session.mode = EditorMode::Timeline;
                } else {
                    *sound = Some("core_error");
                }
            }
            // A on a basic camera row: that camera from the current view, then its key edit mode (b401).
            if !lane_row && (pressed & A != 0 || keys.just_pressed(KeyCode::Enter)) {
                if let Some((view, root)) = view_root {
                    editor.camera_rules.fill_section = fill_section;
                    if editor.pick_asks(editor.row, cursor) {
                        camera_ask = Some(CameraAsk { row: editor.row, time: cursor, view, anchors: root, shown: false });
                    } else {
                        let row = editor.row;
                        pick_camera(editor, row, cursor, &view, &root);
                    }
                }
            }
            true
        }
        // The camera key edit modes (7 / 8 / 9 / 10): the sticks and D-pad edit the key, A commits (b386).
        EditorMode::FollowKey | EditorMode::TripodKey | EditorMode::GameKey | EditorMode::FreeKey => {
            let dpad = |plus: u16, minus: u16| f32::from(pad.buttons & plus != 0) - f32::from(pad.buttons & minus != 0);
            let input = camera_key::CameraInput {
                // The screen's prompts (kFollowEditMode etc.): the right stick rotates, the left stick moves in / out.
                turn: Vec2::new(axis(pad.right[0]), axis(pad.right[1])),
                move_: Vec2::new(axis(pad.left[0]), axis(pad.left[1])),
                height: dpad(DPAD_UP, DPAD_DOWN),
                zoom: dpad(DPAD_RIGHT, DPAD_LEFT),
            };
            editor.edit_key(input, view_root.as_ref().map(|(_, root)| root), dt);
            if let Some((_, root)) = view_root.as_ref() {
                editor.constrain_editing(root, &collision, |from, to, radius| world.and_then(|w| camera_collision::sweep(w, from, to, radius)));
            }
            // Leaving a key edit runs the base game's commit too (`82508930` leave states 7..10 -> `82507438(E, 0)`,
            // b462); it lands only when no camera key is under the cursor.
            if pressed & A != 0 || keys.just_pressed(KeyCode::Enter) {
                info!("REPLAY_CAMERA key done {}", editor.editing_line().unwrap_or_default());
                if !channels {
                    editor.commit_base(cursor);
                }
                editor.session.mode = EditorMode::Timeline;
            }
            true
        }
        EditorMode::Trim => {
            if pad.buttons & LB == 0 && !keys.pressed(KeyCode::KeyL) {
                editor.session.mode = EditorMode::Timeline;
            } else {
                editor.trim_sticks(axis(pad.left[0]), axis(pad.right[0]), dt);
                seek = Some(editor.trim_cursor());
            }
            true
        }
        // Y cycles the edit channel on the Create pack's screen (b405 / b411, `82612C28` case 4: camera, speed, shake,
        // brightness, contrast, saturation, tint). K on the keyboard.
        _ if channels && (pressed & Y != 0 || keys.just_pressed(KeyCode::KeyK)) => {
            editor.cycle_channel();
            true
        }
        _ if pressed & LB != 0 || keys.just_pressed(KeyCode::KeyL) => {
            play = Some(false);
            editor.session.mode = EditorMode::Trim;
            true
        }
        // A (b462 / b463, `82612C28` case 2) on the active channel: a soft camera key re-edits it; a soft lane key
        // snaps the cursor to it and opens the effects menu; a hard key, or the clip's ends, is refused (`core_error`);
        // elsewhere the effects menu on the camera in effect. The menu loads its live values as it opens.
        _ if pressed & A != 0 || keys.just_pressed(KeyCode::Enter) => {
            play = Some(false);
            match editor.a_target(cursor, channels) {
                editor::ATarget::CameraKey => {
                    if let Some(mode) = editor.reopen_at(cursor) {
                        info!("REPLAY_CAMERA key reopen {}", editor.editing_line().unwrap_or_default());
                        editor.session.mode = match mode {
                            7 => EditorMode::FollowKey,
                            8 => EditorMode::TripodKey,
                            9 => EditorMode::GameKey,
                            _ => EditorMode::FreeKey,
                        };
                    }
                }
                editor::ATarget::Effects(snap) => {
                    let at = snap.unwrap_or(cursor);
                    seek = snap;
                    editor.row = editor.active_row(at);
                    if channels && editor.channel != 0 {
                        editor.row = editor::CHANNELS[editor.channel].1[0];
                    }
                    editor.load_live(at, channels);
                    editor.session.mode = EditorMode::Effects;
                }
                editor::ATarget::Refused => *sound = Some("core_error"),
            }
            true
        }
        _ => {
            // D-pad Down drops a marker, asking first inside a run of soft keys; D-pad Up deletes the marker at the
            // cursor after retail's question (b407: key ids 0x21 / 0x20 in `82612C28`).
            if released & DPAD_DOWN != 0 || keys.just_pressed(KeyCode::KeyM) {
                if editor.drop_needs_confirm(cursor, channels) {
                    ask = Some(MarkerAsk { delete: None, time: cursor, shown: false, channels });
                } else if let Err(e) = editor.drop_marker(cursor, channels) {
                    info!("REPLAY_EDITOR marker refused: {e:?}");
                }
            }
            // Retail's sounds (b459, `82613650`): `editor_up_marker` as the question opens (none on Yes),
            // `core_error` when no marker is under the cursor.
            if released & DPAD_UP != 0 || keys.just_pressed(KeyCode::KeyN) {
                match editor.marker_kind(cursor, channels) {
                    Some(kind) => {
                        ask = Some(MarkerAsk { delete: Some(kind), time: cursor, shown: false, channels });
                        *sound = Some("editor_up_marker");
                    }
                    None => {
                        *sound = Some("core_error");
                        info!("REPLAY_EDITOR delete refused: no marker at the cursor");
                    }
                }
            }
            let (start, end) = editor.heads();
            if released & DPAD_LEFT != 0 || keys.just_pressed(KeyCode::Comma) {
                seek = Some(editor.skip(cursor, false));
            }
            if released & DPAD_RIGHT != 0 || keys.just_pressed(KeyCode::Period) {
                seek = Some(editor.skip(cursor, true));
                play = Some(false);
            }
            if pressed & RB != 0 || keys.just_pressed(KeyCode::Space) {
                if !playing && (cursor >= end || cursor < start) {
                    seek = Some(start);
                }
                play = Some(!playing);
                true
            } else {
                false
            }
        }
    };
    if editor.session.mode != mode_before || editor.channel != channel_before {
        info!("REPLAY_EDITOR mode {:?} -> {:?} channel {} -> {}", mode_before, editor.session.mode, channel_before, editor.channel);
    }
    if ask.is_some() {
        replay.marker_ask = ask;
        replay.playing = false;
    }
    if camera_ask.is_some() {
        replay.camera_ask = camera_ask;
        replay.playing = false;
    }
    if let Some(t) = seek {
        replay.seek(t);
    }
    if let Some(p) = play {
        replay.playing = p;
    }
    used
}

/// Reads the shake clips setup exported (`tools/prepare_replay_shake.py`); a missing file leaves shake off.
fn load_shake(config: Option<Res<crate::config::Config>>, mut replay: ResMut<Replay>) {
    let Some(config) = config else { return };
    let path = config.asset_root.join("private/hud/replay-shake/replay_shake.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        info!("REPLAY_SHAKE no clips ({}): the Shake row has no effect until Refresh Game Files", path.display());
        return;
    };
    let parsed = (|| -> Option<skate_core::replay_edit::shake::ShakeClips> {
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        (v["format"] == "skate3-replay-shake" && v["version"] == 1).then_some(())?;
        let clips = v["clips"].as_array()?.iter().map(|c| {
            c.as_array().into_iter().flatten().filter_map(|k| {
                let k = k.as_array()?;
                let mut out = [0.0f32; 6];
                for (o, x) in out.iter_mut().zip(k) {
                    *o = x.as_f64()? as f32;
                }
                Some(out)
            }).collect()
        }).collect();
        Some(skate_core::replay_edit::shake::ShakeClips { key_rate: v["key_rate"].as_f64().unwrap_or(30.0) as f32, clips })
    })();
    match parsed {
        Some(clips) => {
            info!("REPLAY_SHAKE {} clips", clips.clips.iter().filter(|c| !c.is_empty()).count());
            replay.shake_clips = std::sync::Arc::new(clips);
        }
        None => warn!("REPLAY_SHAKE unreadable {}", path.display()),
    }
}

impl Replay {
    /// The editor's camera at the cursor (keys on the recorded camera `camera` and the skater root, lens as vertical
    /// FOV); None = recorded camera.
    pub(crate) fn editor_camera(&self, camera: &Transform, root: &Transform) -> (Option<Transform>, Option<f32>) {
        let Some(editor) = self.editor.as_ref() else { return (None, None) };
        editor.pose(self.cursor, &camera_key::Anchors::new(*camera, *root, self.follow_eye))
    }

    /// [`Self::editor_camera`] for the view. Retail constrains the keyed camera every frame in the instant view and
    /// not in editor playback (the gate per editor mode, b615, m353); doc 37 "Keyed cameras in the instant view".
    pub(crate) fn guarded_editor_camera(&self, camera: &Transform, root: &Transform, guard: &mut CameraGuard,
        sweep: impl FnMut(Vec3, Vec3, f32) -> Option<camera_collision::Hit>) -> (Option<Transform>, Option<f32>) {
        let (pose, fov) = self.editor_camera(camera, root);
        let shot = self.editor.as_ref().and_then(|e| e.camera_at(self.cursor));
        let (Some(mut pose), Some((index, key, lens))) = (pose, shot) else {
            *guard = CameraGuard::default();
            return (pose, fov);
        };
        let cut = guard.shot != Some(index) || (self.cursor - guard.cursor).abs() > CameraGuard::CUT_SECONDS;
        // REPLAY_CAMERA shot: each new shot (a cut) with the key's eye and the skater root (USER-ASKS 339, 341).
        if cut {
            info!("REPLAY_CAMERA shot {index} {} at={:?} root={:?}", editor::log_key(&key, lens), pose.translation.to_array(),
                root.translation.to_array());
        }
        let mode = self.editor.as_ref().map(|e| e.session.mode);
        use skate_core::replay_edit::session::EditorMode as M;
        let editing_key = matches!(mode, Some(M::FollowKey | M::TripodKey | M::GameKey | M::FreeKey));
        let constrain = mode == Some(M::Instant) || (self.camera_collision.playback && mode.is_some() && !editing_key);
        let mut last = None;
        if constrain {
            let free = matches!(key, camera_key::CameraKey::Free { .. });
            let c = camera_collision::constrain(if cut { None } else { guard.last }, pose.translation, root.translation, free,
                &self.camera_collision, sweep);
            if c.position != pose.translation {
                pose = match key {
                    camera_key::CameraKey::Follow { look_height, .. } | camera_key::CameraKey::Tripod { look_height, .. } => {
                        let anchors = camera_key::Anchors::new(*camera, *root, self.follow_eye);
                        Transform::from_translation(c.position).looking_at(anchors.look(look_height), Vec3::Y)
                    }
                    _ => pose.with_translation(c.position),
                };
            }
            last = Some(c.position);
        }
        *guard = CameraGuard { shot: Some(index), cursor: self.cursor, last };
        (Some(pose), fov)
    }

    pub(crate) fn shake(&self) -> Option<Transform> {
        let editor = self.editor.as_ref()?;
        // The level's clip steps at the segment's start; amplitude and frequency ramp to the next soft key's level
        // (SHAKE_TYPE stepped, SHAKE_AMPLITUDE / FREQUENCY interpolated, b464).
        let levels = self.shake_levels.unwrap_or_else(skate_core::replay_edit::shake::retail_levels);
        let level_of = |v: i32| usize::try_from(v).ok().and_then(|i| levels.get(i).copied().flatten());
        let frame = editor.frame(self.cursor);
        let segment = editor.edit.clips[editor.clip].segment(skate_core::replay_edit::Lane::Shake, frame);
        let u = if segment.end > segment.start { (frame.clamp(segment.start, segment.end) - segment.start) as f32 / (segment.end - segment.start) as f32 } else { 0.0 };
        let level = level_of(segment.from).map(|mut l| {
            let (amplitude, frequency) = level_of(segment.to).map_or((0.0, 0.0), |n| (n.amplitude, n.frequency));
            l.amplitude += (amplitude - l.amplitude) * u;
            l.frequency += (frequency - l.frequency) * u;
            l
        });
        let t = self.cursor - editor.heads().0;
        let (q, p) = skate_core::replay_edit::shake::sample(&self.shake_clips, level, t)?;
        Some(Transform::from_translation(Vec3::from_array(p)).with_rotation(Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize()))
    }
}

/// The clip camera on view and the cursor it was seen at (new shots for the `REPLAY_CAMERA shot` log).
#[derive(Default)]
pub(crate) struct CameraGuard {
    shot: Option<usize>,
    cursor: f64,
    /// The constrained eye last frame (the constraint's sweep start); None snaps, as retail on a camera change.
    last: Option<Vec3>,
}

impl CameraGuard {
    /// OURS: a cursor step longer than this (s) is a jump (scrub to a marker, rewind), not camera motion.
    const CUT_SECONDS: f64 = 0.25;
}

pub(crate) fn world_live(replay: Option<Res<Replay>>) -> bool {
    replay.is_none_or(|r| !r.active)
}

fn axis(value: f32) -> f32 {
    value.signum() * ((value.abs() - 0.18) / 0.82).clamp(0.0, 1.0)
}

pub(crate) fn controls(
    mut replay: ResMut<Replay>,
    mut input: ResMut<crate::input::ControllerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    net: Option<Res<crate::multiplayer::Multiplayer>>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
    retail_ui: Option<Res<RetailReplayUi>>,
    look_screen: Option<Res<ReplayLookScreen>>,
    mut sounds: MessageWriter<crate::ui_audio::FrontendSound>,
    mut export: Option<ResMut<export_run::ReplayExport>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
) {
    if replay.exporting || export.as_ref().is_some_and(|e| e.busy()) {
        input.discard_gameplay();
        return;
    }
    if net.is_some_and(|n| n.active()) {
        if replay.active {replay.exit();}
        return;
    }
    let pad = input.raw_input();
    let pressed = pad.buttons & !replay.previous_buttons;
    let released = replay.previous_buttons & !pad.buttons;
    replay.previous_buttons = pad.buttons;
    // B / Back reach the Finished menu only in playback, the instant replay and the timeline (retail modes 1 / 5 / 6,
    // `82614E58`, b555); the trim, key edit and effects modes ignore them (effects commits on B itself).
    // The film pack's screen (its look channels): B goes to the quit warning, X opens its Finished menu (b387 / b557).
    let film = look_screen.as_ref().is_some_and(|s| s.0);
    let finish_mode = replay.editor.as_ref().is_none_or(|e| {
        use skate_core::replay_edit::session::EditorMode;
        matches!(e.session.mode, EditorMode::Playback | EditorMode::Instant | EditorMode::Timeline)
    });
    let menu_open = !crate::graphics_menu::gameplay_active(menu);
    // Only the replay's own answers: other engine popups (the teleporter's confirm) are read by their systems.
    let (ours, others) = std::mem::take(&mut popups.engine_choices).into_iter()
        .partition::<Vec<_>, _>(|(callback, _)| callback.starts_with(REPLAY_POPUP_PREFIX));
    popups.engine_choices = others;
    // A popup answered this frame took the press that answered it (USER-ASKS 342).
    let answered = !ours.is_empty();
    // The quit warning's answer (No / Yes, Yes = 1).
    for (callback, choice) in ours {
        if callback == QUIT_POPUP && replay.quit_pending {
            replay.quit_pending = false;
            if choice == 1 {
                replay.exit();
            }
        }
        // The Finished menu (ours on PC): 0 Save to PC (the export), 1 Exit (retail's Exit = the quit, b387).
        if callback == export_run::FINISHED_POPUP && replay.quit_pending {
            replay.quit_pending = false;
            match choice {
                0 => {
                    popups.engine_requests.push(export_run::save_confirm_popup());
                    replay.quit_pending = true;
                }
                1 => replay.exit(),
                _ => {}
            }
        }
        if callback == export_run::SAVE_CONFIRM_POPUP && replay.quit_pending {
            replay.quit_pending = false;
            if choice == 1 && let Some(export) = export.as_mut() {
                export.choose();
            }
        }
        if callback == FILM_MENU_POPUP && replay.film_ask == Some(FilmAsk::Menu) {
            replay.film_ask = None;
            let rows = film_menu_rows(export.is_some());
            match usize::try_from(choice).ok().and_then(|i| rows.get(i)).copied() {
                Some(export_run::SAVE_TO_PC) => {
                    popups.engine_requests.push(export_run::save_confirm_popup());
                    replay.quit_pending = true;
                }
                Some(TAKE_PHOTO_ROW) => {
                    // The same take photo as the base game's X (`826147C8`, b557).
                    replay.playing = false;
                    replay.photo = Photo::Confirm;
                    popups.engine_requests.push(photo_popup());
                }
                Some(REPLAY_MODE_ROW) => {
                    popups.engine_requests.push(replay_mode_popup());
                    replay.film_ask = Some(FilmAsk::ReplayMode);
                }
                Some(RESET_CHANNEL_ROW) | Some(RESET_ALL_ROW) => {
                    let all = rows.get(choice as usize) == Some(&RESET_ALL_ROW);
                    popups.engine_requests.push(reset_popup(all));
                    replay.film_ask = Some(FilmAsk::Reset { all });
                }
                _ => {}
            }
        }
        // The replay mode popup stays up after a toggle (`82615728`); cancel closes it.
        if callback == REPLAY_MODE_POPUP && replay.film_ask == Some(FilmAsk::ReplayMode) {
            match usize::try_from(choice).ok().filter(|&i| i < REPLAY_MODE_ROWS.len()) {
                Some(row) => {
                    replay.view.toggle(row);
                    popups.engine_requests.push(replay_mode_popup());
                }
                None => replay.film_ask = None,
            }
        }
        if callback == RESET_POPUP && let Some(FilmAsk::Reset { all }) = replay.film_ask {
            replay.film_ask = None;
            match choice {
                1 => {
                    if let Some(editor) = replay.editor.as_mut() {
                        editor.reset_channels(all);
                    }
                    info!("REPLAY_EDITOR reset {}", if all { "timeline" } else { "channel" });
                }
                0 => {
                    popups.engine_requests.push(film_menu_popup(export.is_some()));
                    replay.film_ask = Some(FilmAsk::Menu);
                }
                _ => {}
            }
        }
        if callback == MARKER_POPUP {
            if let Some(ask) = replay.marker_ask.take().filter(|_| choice == 1) {
                apply_marker_ask(&mut *replay, ask);
            }
        }
        if callback == CAMERA_POPUP {
            if let (Some(ask), Some(editor)) = (replay.camera_ask.take().filter(|_| choice == 1), replay.editor.as_mut()) {
                pick_camera(editor, ask.row, ask.time, &ask.view, &ask.anchors);
            }
        }
        if callback == PHOTO_POPUP && replay.photo == Photo::Confirm {
            replay.photo = if choice == 1 { Photo::Capture } else { Photo::Idle };
        }
    }
    // Retail help (FE 13 over the replay screen, handler 82616B40, b500): A turns the page, B closes it; nothing else
    // reaches the replay meanwhile. Needs the retail screen (no help without it).
    if replay.active && replay.help_open && !menu_open {
        if pressed & A != 0 || keys.just_pressed(KeyCode::Enter) {
            replay.help_turns += 1;
            info!("REPLAY_HELP page {}", replay.help_turns);
        }
        if pressed & B != 0 || keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) {
            replay.help_open = false;
            info!("REPLAY_HELP close");
        }
        input.discard_gameplay();
        return;
    }
    if !menu_open {
        let back = pressed & SELECT != 0 || keys.just_pressed(KeyCode::F6);
        if !replay.active {
            // Retail: Back during play opens the instant replay (`825EB998`), only with enough recorded
            // (`8250B8F8`); a refusal is silent.
            if back && replay.can_open() {
                replay.enter();
                // Retail plays UI sound 0x394015B68783BE9E as it opens (`825EB998`, b389): the fe record
                // `editor_select`.
                sounds.write(crate::ui_audio::FrontendSound::named(OPEN_SOUND));
            }
        } else if replay.quit_pending {
            // OURS (safety until play-tested): Back again while the warning is up leaves at once.
            if back {
                replay.quit_pending = false;
                replay.exit();
            }
        } else if (back || pressed & B != 0) && finish_mode && replay.film_ask.is_none() {
            replay.quit_on_release = true;
        }
        // Retail: B / Back opens the Finished menu or the quit warning (b387 / b389), raised once the button is up.
        if replay.quit_on_release && pad.buttons & (B | SELECT) == 0 && !keys.pressed(KeyCode::F6) {
            replay.quit_on_release = false;
            if popups.stage_ready {
                // With the exporter: the Finished menu (Save to PC / Exit); without it retail's quit warning (not
                // signed in, b387).
                popups.engine_requests.push(if export.is_some() && !film { export_run::finished_popup() } else { quit_popup() });
                replay.quit_pending = true;
            } else {
                replay.exit();
            }
        }
    }
    // Continue reading the UI controller while fixed gameplay ticks are gated.
    // On exit, require release so a held scrub/flight control cannot become a
    // grab, push or flick in the preserved live simulation.
    if replay.active || replay.release_controls {
        input.discard_gameplay();
        if !replay.active
            && pad.buttons == 0
            && pad.triggers.iter().all(|v| *v < 0.05)
            && pad.left.iter().chain(&pad.right).all(|v| v.abs() < 0.18)
        {
            replay.release_controls = false;
        }
    }
    // A marker question raised by the editor last frame: retail's popup, or straight through without the retail stage.
    if let Some(ask) = replay.marker_ask.filter(|a| !a.shown) {
        if popups.stage_ready {
            popups.engine_requests.push(marker_popup(ask.delete));
            replay.marker_ask = Some(MarkerAsk { shown: true, ..ask });
        } else {
            replay.marker_ask = None;
            apply_marker_ask(&mut *replay, ask);
        }
    }
    if let Some(ask) = replay.camera_ask.filter(|a| !a.shown) {
        if popups.stage_ready {
            popups.engine_requests.push(camera_popup());
            replay.camera_ask = Some(CameraAsk { shown: true, ..ask });
        } else {
            replay.camera_ask = None;
            if let Some(editor) = replay.editor.as_mut() {
                pick_camera(editor, ask.row, ask.time, &ask.view, &ask.anchors);
            }
        }
    }
    // An open replay popup owns the pad: no editor or transport input underneath it (USER-ASKS 342).
    if !replay.active || menu_open || answered || replay.quit_pending || replay.marker_ask.is_some() || replay.camera_ask.is_some() || replay.film_ask.is_some()
        || replay.photo != Photo::Idle {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    // OURS (PC only): C toggles a free camera. Retail's instant replay has none (its cameras are the editor's);
    // retail Y cycles the edit channel with the film DLC only, A opens the editor (not built yet).
    if keys.just_pressed(KeyCode::KeyC) {
        replay.toggle_camera();
    }
    let x = pressed & X != 0 || keys.just_pressed(KeyCode::KeyP);
    // The film pack: X opens its Finished menu (b387); in its effects menu X commits a soft key (editor_controls).
    if film && x {
        if finish_mode && replay.film_ask.is_none() && popups.stage_ready {
            replay.playing = false;
            popups.engine_requests.push(film_menu_popup(export.is_some()));
            replay.film_ask = Some(FilmAsk::Menu);
            return;
        }
    } else if x && retail_ui.as_ref().is_some_and(|r| r.0) {
        // Retail: X takes a photo (CanSaveMedia; ours: always on PC, with the retail screen); P on the keyboard.
        replay.playing = false;
        replay.photo = Photo::Confirm;
        popups.engine_requests.push(photo_popup());
        return;
    }
    // Retail: Start opens the help (FE 13, research b389 / b391), only with the retail screen (`retail_ui`).
    if (pressed & START != 0) && retail_ui.as_ref().is_some_and(|r| r.0) {
        replay.help_open = true;
        info!("REPLAY_HELP open");
        return;
    }
    // Retail: L3 hides / shows the panel (kHideEditor / kShowEditor; the movie's Hide / Show prompt follows). H on
    // the keyboard. With our free camera on, L3 stays its speed modifier.
    if (pressed & L3 != 0 && replay.free_camera.is_none()) || keys.just_pressed(KeyCode::KeyH) {
        replay.panel_hidden = !replay.panel_hidden;
    }
    // Retail: A with the panel shown leaves the instant replay for the editor's timeline (`82613988`; mode 6), only
    // with the retail screen and a recording of at least 5 s (150 timeline frames). Enter on the keyboard.
    if replay.editor.is_none() {
        if (pressed & A != 0 || keys.just_pressed(KeyCode::Enter)) && !replay.panel_hidden && retail_ui.as_ref().is_some_and(|r| r.0) {
            let (start, end) = replay.bounds();
            replay.editor = editor::Editor::open(start, end, replay.edit_limits.clone());
            if replay.editor.is_some() {
                replay.playing = false;
                info!("REPLAY_EDITOR open at={:.1}s", replay.cursor - start);
            }
            return;
        }
    } else {
        let mut sound = None;
        let used = editor_controls(&mut replay, look_screen.as_ref().is_some_and(|s| s.0), pressed, released, &pad, &keys, dt, &mut sound,
            physics.as_deref().map(crate::physics::GamePhysics::world));
        if let Some(name) = sound {
            sounds.write(crate::ui_audio::FrontendSound::named(name));
        }
        if used {
            return;
        }
    }
    // Retail: RB starts / stops playback (`82C7F0A8` stops); stick or D-pad input resumes a stopped replay
    // (`82C7FFB8`, research b386 / b387). Space is the PC key for RB.
    let stick = replay.editor.is_none() && pad.left.iter().chain(&pad.right).any(|v| v.abs() >= 0.18);
    let dpad = pressed & (DPAD_LEFT | DPAD_RIGHT | 0x0001 | 0x0002) != 0;
    if pressed & RB != 0 || keys.just_pressed(KeyCode::Space) {
        if !replay.playing && replay.cursor >= replay.bounds().1 {
            let start = replay.bounds().0;
            replay.seek(start);
        }
        replay.playing = !replay.playing;
    } else if !replay.playing && replay.free_camera.is_none() && (stick || dpad) && replay.cursor < replay.bounds().1 {
        replay.playing = true;
    }
    // Retail: D-pad Left / Right pressed jump to the start / end (mode 5; the editor's timeline jumps on release,
    // `editor_controls`); comma / period on the keyboard.
    if replay.editor.is_none() && (pressed & DPAD_LEFT != 0 || keys.just_pressed(KeyCode::Comma)) {
        let start = replay.bounds().0;
        replay.seek(start);
    }
    if replay.editor.is_none() && (pressed & DPAD_RIGHT != 0 || keys.just_pressed(KeyCode::Period)) {
        let end = replay.bounds().1;
        replay.seek(end);
        replay.playing = false;
    }
    let trigger = |v: f32| ((v - 0.05) / 0.95).clamp(0.0, 1.0);
    let scrub = trigger(pad.triggers[1]) - trigger(pad.triggers[0])
        + f32::from(keys.pressed(KeyCode::ArrowRight))
        - f32::from(keys.pressed(KeyCode::ArrowLeft));
    // Retail's scrub: eased, at most 0.005 of the recording per console frame (`ScrubSpec`, research b387 / b388);
    // a moving scrub stops playback, and it eases out after the triggers are released.
    let spec = replay.scrub_spec;
    let moved = replay.scrub.step(&spec, scrub.clamp(-1.0, 1.0), dt);
    if moved != 0.0 {
        replay.playing = false;
        let (start, end) = replay.bounds();
        let cursor = replay.cursor + f64::from(moved) * (end - start);
        replay.seek(cursor);
    } else if replay.playing {
        // The editor plays the clip at its speed lane and stops at the out point (mode 1 -> 6).
        let rate = replay.editor.as_ref().map_or(1.0, |e| e.speed(replay.cursor));
        let end = replay.editor.as_ref().map_or(replay.bounds().1, |e| e.heads().1);
        let cursor = (replay.cursor + f64::from(dt) * rate).min(end);
        replay.seek(cursor);
        if replay.cursor >= end {
            replay.playing = false;
        }
    }
    let player = replay.sample().map(|(a, b, t)| blend(a.root, b.root, t).translation);
    let rules = replay.camera_collision;
    if let Some(camera) = replay.free_camera.as_mut() {
        let old = camera.translation;
        let (mut yaw, mut pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        yaw -= axis(pad.right[0]) * dt * 2.0;
        pitch += axis(pad.right[1]) * dt * 2.0;
        if mouse.pressed(MouseButton::Right) {
            yaw -= motion.delta.x * 0.003;
            pitch -= motion.delta.y * 0.003;
        }
        pitch = pitch.clamp(-1.5, 1.5);
        camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        let x = axis(pad.left[0]) + f32::from(keys.pressed(KeyCode::KeyD))
            - f32::from(keys.pressed(KeyCode::KeyA));
        let z = axis(pad.left[1]) + f32::from(keys.pressed(KeyCode::KeyW))
            - f32::from(keys.pressed(KeyCode::KeyS));
        let y = f32::from(pad.buttons & 0x0200 != 0 || keys.pressed(KeyCode::KeyE))
            - f32::from(pad.buttons & 0x0100 != 0 || keys.pressed(KeyCode::KeyQ));
        let heading = Quat::from_rotation_y(yaw);
        let speed = if pad.buttons & 0x0040 != 0 || keys.pressed(KeyCode::ShiftLeft) {
            12.0
        } else {
            3.0
        };
        camera.translation += (heading * Vec3::new(x, 0.0, -z) + Vec3::Y * y) * dt * speed;
        // Retail's leash and world collision (`82683680`, b381 / b449); Free keeps its rotation.
        if let Some(player) = player {
            let world = physics.as_deref().map(crate::physics::GamePhysics::world);
            camera.translation = camera_collision::constrain(Some(old), camera.translation, player, true, &rules,
                |from, to, radius| world.and_then(|w| camera_collision::sweep(w, from, to, radius))).position;
        }
    }
}

#[derive(Component)]
struct ReplayUi;
#[derive(Component)]
struct ReplayLabel;
#[derive(Component)]
struct Playhead;

fn setup_ui(mut commands: Commands) {
    commands.spawn((ReplayUi, GlobalZIndex(5), Node {
        display: Display::None, position_type: PositionType::Absolute,
        bottom: px(24), left: percent(8), width: percent(84),
        padding: UiRect::all(px(16)), flex_direction: FlexDirection::Column,
        row_gap: px(10), ..default()
    }, BackgroundColor(Color::srgba(0.02, 0.035, 0.05, 0.92)))).with_children(|panel| {
        panel.spawn((ReplayLabel, Text::new("REPLAY"), TextFont { font_size: 20., ..default() }));
        panel.spawn((Node { width: percent(100), height: px(6), ..default() },
            BackgroundColor(Color::srgb(0.18, 0.22, 0.26)))).with_children(|bar| {
            bar.spawn((Playhead, Node { width: percent(100), height: percent(100), ..default() },
                BackgroundColor(Color::srgb(0.35, 0.9, 0.7))));
        });
        // Pad hints as the connected pad's glyphs (prompt_glyphs, SDL labels); keyboard hints as text.
        let hint = Color::srgb(0.75, 0.82, 0.86);
        // Retail's instant replay controls (research b386 / b389).
        crate::prompt_glyphs::hint_row(panel, &[(&["L_Trigger", "R_Trigger"], "", "Scrub"), (&["DPad_Left", "DPad_Right"], "", "Start / end"),
            (&["R_Bumper"], "", "Play / stop"), (&["Back", "B"], "", "Finished")], 14., hint);
        panel.spawn((Text::new("Keyboard: F6  Replay / Finished   |   Arrows  Scrub   |   , / .  Start / end   |   Space  Play   |   C  Free camera (WASD / Q / E move, RMB drag look, Shift fast)"),
            TextFont { font_size: 14., ..default() }, TextColor(hint)));
    });
}

/// The replay as the retail replay screen reads it (`replay_menu_host`), published every frame.
#[derive(Resource, Default)]
pub(crate) struct ReplayFeed(pub crate::replay_menu_host::ReplayScreenFeed);

/// The retail replay screen is shown / still loading (set by `retail_menu_stage`); our Bevy panel is the fallback
/// without it (research b500).
#[derive(Resource, Default, PartialEq)]
pub(crate) struct RetailReplayUi(pub bool, pub bool);

/// How long the Bevy panel waits for a loading retail screen before it shows anyway.
const RETAIL_UI_WAIT_S: f32 = 3.0;

/// The retail screen shown is the Create pack's replay editor (replay_editor3, product CCF0007; research b410 /
/// b411): the editor's edit channels and look rows are on. Set by the menu stage.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub(crate) struct ReplayLookScreen(pub bool);

fn publish_feed(replay: Res<Replay>, mut feed: ResMut<ReplayFeed>) {
    let (start, end) = replay.bounds();
    let (length_seconds, length_valid) = match replay.editor.as_ref() {
        Some(e) => (e.edit.encoded_seconds(), e.edit.can_share()),
        None => {
            let seconds = (end - start).max(0.0).round() as u32;
            (seconds, seconds <= replay.edit_limits.share_max_seconds)
        }
    };
    let next = crate::replay_menu_host::ReplayScreenFeed {
        active: replay.active,
        instant: replay.active && replay.editor.is_none(),
        position: if end > start { (replay.cursor - start) / (end - start) } else { 1.0 },
        playing: replay.playing,
        panel: !replay.panel_hidden,
        length_seconds,
        length_valid,
        help: replay.active && replay.help_open,
        help_turns: replay.help_turns,
        mode: replay.editor.as_ref().map_or(5, |e| e.mode()),
        camera_row: replay.editor.as_ref().map_or(editor::GAME_CAMERA_ROW as u32, |e| e.active_row(replay.cursor) as u32),
        segments: replay.editor.as_ref().map_or_else(Vec::new, |e| e.segments()),
        keys: replay.editor.as_ref().map_or_else(Vec::new, |e| e.key_fractions()),
        heads: replay.editor.as_ref().map_or((0, 0), |e| e.head_frames()),
        row: replay.editor.as_ref().map_or(0, |e| e.row as u32),
        speed: replay.editor.as_ref().map_or(100, |e| e.lane_value(skate_core::replay_edit::Lane::Speed, replay.cursor)),
        shake: replay.editor.as_ref().map_or(0, |e| e.lane_value(skate_core::replay_edit::Lane::Shake, replay.cursor)),
        channel: replay.editor.as_ref().map_or(0, |e| e.channel as u32),
        mode_value: replay.editor.as_ref().map_or(-1, |e| e.mode_value(replay.cursor)),
        mode_slider: replay.editor.as_ref().map_or(2, |e| e.mode_slider(replay.cursor)),
        tint: replay.editor.as_ref().map_or(0, |e| e.tint as u32),
        tints: std::array::from_fn(|i| replay.editor.as_ref().map_or(0, |e| e.tint_value(i, replay.cursor))),
    };
    if feed.0 != next {
        feed.0 = next;
    }
}

fn update_ui(
    replay: Res<Replay>,
    retail: Res<RetailReplayUi>,
    time: Res<Time>,
    mut waited: Local<f32>,
    mut roots: Query<&mut Node, With<ReplayUi>>,
    mut labels: Query<&mut Text, With<ReplayLabel>>,
    mut bars: Query<&mut Node, (With<Playhead>, Without<ReplayUi>)>,
) {
    *waited = if replay.active && retail.1 { *waited + time.delta_secs() } else { 0.0 };
    let loading = retail.1 && *waited < RETAIL_UI_WAIT_S;
    for mut node in &mut roots {
        node.display = if replay.active && !retail.0 && !loading && !replay.panel_hidden {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !replay.active {
        return;
    }
    let (start, end) = replay.bounds();
    for mut text in &mut labels {
        **text = format!(
            "REPLAY  {:.2} / {:.2} s   |   {}   |   {}",
            replay.cursor - start,
            end - start,
            if replay.playing { "PLAYING" } else { "PAUSED" },
            if replay.free_camera.is_some() {
                "FREE CAMERA"
            } else {
                "RECORDED CAMERA"
            }
        );
    }
    let fraction = if end > start {
        (replay.cursor - start) / (end - start)
    } else {
        1.0
    };
    for mut node in &mut bars {
        node.width = percent(fraction as f32 * 100.0);
    }
}

mod components;
pub(crate) mod camera_collision;
pub(crate) mod camera_key;
pub(crate) mod editor;
pub(crate) mod export;
pub(crate) mod export_run;
mod messages;
mod world;
pub(crate) use world::{GhostOf, keep_model, retire};

#[cfg(test)]
mod tests;
