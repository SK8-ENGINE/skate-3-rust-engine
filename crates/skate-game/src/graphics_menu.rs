//! The PC overlay (Guide, F1 or Escape): native-resolution pause UI over a separately scaled 3D render target.
//! Its look, motion and navigation are a port of the recomp's settings overlay (`view`); the rows are ours.
mod view;

use crate::difficulty::Difficulty;
use crate::custom_difficulty::{Tuning, OPTIONS};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat},
    },
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const RESOLUTIONS: &[(u32, u32)] = &[
    (1280, 720),
    (1280, 800),
    (1600, 900),
    (1920, 1080),
    (1920, 1200),
    (2560, 1440),
    (2560, 1600),
    (3840, 2160),
];
const SCALES: &[u32] = &[25, 50, 67, 75, 85, 100];
const DAY_SPEEDS: &[u32] = &[0, 1, 10, 30, 60, 120, 360, 720];
const LIMITS: &[u32] = &[0, 30, 60, 90, 120, 144, 165, 240];
/// Extended Replays lengths, s: the "Replay Length" slider, 2 to 4 minutes in 30 s steps.
const REPLAY_LENGTHS: &[u32] = &[120, 150, 180, 210, 240];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct GraphicsSettings {
    width: u32,
    height: u32,
    scale: u32,
    fps: u32,
    hour: f32,
    day_speed: u32,
    ambient_level: Option<u32>,
    /// On-screen frame-time counter (`frame_timing`); off by default.
    frame_stats: bool,
    /// With the counter on: the small readout (fps, frame time, 1 % low, worst and a tiny spike
    /// graph) instead of the verbose one. Older settings files lack it and stay verbose.
    frame_stats_simple: bool,
    /// The counter in the top right corner (where it sat before 2026-10-23) instead of the top left (ask 404).
    frame_stats_right: bool,
    /// The session timer, bottom left (time since the game started, for reports; USER-ASKS 495). On by default.
    session_timer: bool,
    /// NPC draw distance multiplier (QoL, not retail): 1 = retail ranges.
    npc_draw_distance: f32,
    /// The opt-in living world features (`living_world::opt_ins`); all off by default.
    opt_ins: crate::living_world::opt_ins::OptIns,
    /// Extended Replays (QoL, not retail; user 2026-10-20): off = retail's 90 s instant replay window.
    extended_replays: bool,
    /// The instant replay length with Extended Replays on, s ([`REPLAY_LENGTHS`]).
    replay_seconds: u32,
    /// Better Object Mover (QoL, not retail; user 2026-10-20): every prop can be moved, not only those with authored
    /// grab splines (`prop_dynamics::generated_grab_splines`).
    better_object_mover: bool,
    /// Debug Menus (not retail; user 2026-10-21): shows the menus retail ships hidden (the Challenge Map's debug
    /// listings first; research b479), through the retail menus' `debug_menus` fact.
    debug_menus: bool,
    /// Fly Cam (dev tool, not retail; user 2026-10-27): the D-pad Right + R3 fly cam (`debug_cam`) works only while on.
    fly_cam: bool,
    /// Physics Step Threads (doc 25 "Fixed step executor"): false = the physics step on one thread (the default),
    /// true = Bevy's multi-threaded scheduler. Read at start-up (`physics_threads_multi`); the launcher's Tools page
    /// writes the same field.
    physics_threads_multi: bool,
    /// Which default changes this file has been through (`DEFAULTS_VERSION`; 0 in files from before the field); files
    /// from before 1 get NPC Skater Physics switched on once, so a later "off" sticks.
    #[serde(default)]
    defaults_version: u32,
}
/// 1 (2026-10-25, USER-ASKS 338): NPC Skater Physics on by default.
const DEFAULTS_VERSION: u32 = 1;
impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            scale: 100,
            fps: 0,
            hour: 12.,
            day_speed: 60,
            ambient_level: None,
            frame_stats: false,
            frame_stats_simple: false,
            frame_stats_right: false,
            session_timer: true,
            npc_draw_distance: skate_core::living_world::DrawDistance::RETAIL,
            opt_ins: Default::default(),
            extended_replays: false,
            replay_seconds: REPLAY_LENGTHS[0],
            better_object_mover: false,
            debug_menus: false,
            fly_cam: false,
            physics_threads_multi: false,
            defaults_version: DEFAULTS_VERSION,
        }
    }
}

/// Physics Step Threads as saved (`settings/graphics.json` beside the installation's assets), read once when the app
/// is built; `SKATE_FIXED_EXECUTOR=multi|single` overrides it for a run.
pub(crate) fn physics_threads_multi(asset_root: &std::path::Path) -> bool {
    match std::env::var("SKATE_FIXED_EXECUTOR").as_deref() {
        Ok("multi") => return true,
        Ok("single") => return false,
        _ => {}
    }
    let path = asset_root.parent().unwrap_or(asset_root).join("settings/graphics.json");
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<GraphicsSettings>(&b).ok()).is_some_and(|s| s.physics_threads_multi)
}
impl GraphicsSettings {
    /// A file saved before a default changed takes the new default once; true when it changed (to be saved).
    fn migrated(&mut self) -> bool {
        if self.defaults_version >= DEFAULTS_VERSION {
            return false;
        }
        if self.defaults_version < 1 {
            self.opt_ins.npc_physics = true;
        }
        self.defaults_version = DEFAULTS_VERSION;
        true
    }
    fn validated(mut self) -> Self {
        if !skate_core::living_world::DrawDistance::MENU_STEPS.contains(&self.npc_draw_distance) {
            self.npc_draw_distance = skate_core::living_world::DrawDistance::RETAIL;
        }
        if !REPLAY_LENGTHS.contains(&self.replay_seconds) {
            self.replay_seconds = REPLAY_LENGTHS[0];
        }
        self.ambient_level = self.ambient_level.map(|level| level.min(100));
        self.hour = if self.hour.is_finite() { self.hour.rem_euclid(24.) } else { 12. };
        if !DAY_SPEEDS.contains(&self.day_speed) { self.day_speed = 60; }
        if !RESOLUTIONS.contains(&(self.width, self.height)) {
            (self.width, self.height) = (1280, 800);
        }
        if !SCALES.contains(&self.scale) {
            self.scale = 100;
        }
        if !LIMITS.contains(&self.fps) {
            self.fps = 0;
        }
        self
    }
    fn internal_size(&self, window: UVec2) -> UVec2 {
        (window * self.scale / 100).max(UVec2::ONE)
    }
}
#[derive(Resource)]
pub(crate) struct Menu {
    pub(crate) open: bool,
    /// A retail FE screen that suspends gameplay is open (info record +0; `retail_menu_stage`): gameplay stops
    /// like it does for this menu.
    pub(crate) fe_suspended: bool,
    selected: usize,
    settings: GraphicsSettings,
    path: PathBuf,
    difficulty: Difficulty,
    custom: Tuning,
    custom_defaults: Tuning,
    custom_apply: bool,
    custom_dirty: bool,
    status: String,
    maps: Vec<crate::map_library::Entry>,
    selected_map: usize,
    multiplayer: bool,
    browser: bool,
    network_page: u8,
    browser_count: usize,
    daylight: bool,
    section: usize,
    custom_sections: Vec<(String, Vec<(String,String,String)>)>,
    map_detail: bool,
    destinations: Vec<crate::teleport_menu::Destination>,
    pending_travel: Option<(Option<PathBuf>, [[f32; 4]; 4])>,
    /// Focus, animation and hit-test state of the recomp-style view.
    view: view::ViewState,
}
impl Menu {
    #[cfg(test)]
    pub(crate) fn advance_day(&mut self, seconds: f32) -> f32 {
        if !self.open && self.settings.day_speed > 0 {
            self.settings.hour = (self.settings.hour + seconds * self.settings.day_speed as f32 / 3600.).rem_euclid(24.);
        }
        self.settings.hour
    }
    /// Whether the frame-time counter is shown (graphics menu row, saved).
    pub(crate) fn frame_stats_visible(&self) -> bool {
        self.settings.frame_stats
    }
    /// The counter is on in its simplified form (graphics menu row, saved).
    pub(crate) fn frame_stats_simple(&self) -> bool {
        self.settings.frame_stats && self.settings.frame_stats_simple
    }
    /// The counter sits in the top right corner (graphics menu row, saved).
    pub(crate) fn frame_stats_right(&self) -> bool {
        self.settings.frame_stats_right
    }
    /// The session timer shows (graphics menu row, saved).
    pub(crate) fn session_timer_visible(&self) -> bool {
        self.settings.session_timer
    }
    /// The overlay drew this frame (not hidden behind the customiser, travel or mod menus).
    /// Better Object Mover: every prop can be moved (not retail).
    pub(crate) fn better_object_mover(&self) -> bool {
        self.settings.better_object_mover
    }
    /// Debug Menus: retail's hidden debug menus are shown (not retail).
    pub(crate) fn debug_menus(&self) -> bool {
        self.settings.debug_menus
    }
    /// Whether the fly cam chord works (OPT_INS "Fly Cam", saved).
    pub(crate) fn fly_cam(&self) -> bool {
        self.settings.fly_cam
    }
    /// The player's instant replay length (Extended Replays), s; None = retail's window.
    pub(crate) fn replay_seconds(&self) -> Option<u32> {
        self.settings.extended_replays.then_some(self.settings.replay_seconds)
    }
    pub(crate) fn overlay_visible(&self) -> bool {
        self.view.visible
    }
    pub(crate) fn diagnostic_settings(&self) -> String {
        format!("{:?}", self.settings)
    }
    pub(crate) fn transition_finished(&mut self, status: String, resume: bool) {
        self.status = status;
        self.open = !resume;
    }
}
/// What a key or button does to the PC overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayKey {
    /// Open it, or close it at once.
    Toggle,
    /// One step back (the recomp's `NavigateBack`): text edit, sub-page, row column, rail, then closed.
    Back,
}
/// PC overlay keys: Guide / F1 toggle, Escape opens and backs out, Start only while the retail pause cannot open.
pub(crate) fn overlay_key(escape: bool, f1: bool, pressed: u16, open: bool, retail_pause: bool, fe_suspended: bool) -> Option<OverlayKey> {
    let guide = pressed & 0x0400 != 0;
    let pad_start = pressed & 0x0010 != 0 && !retail_pause && !fe_suspended;
    if f1 || guide || pad_start || (escape && !open) {
        Some(OverlayKey::Toggle)
    } else if open && (escape || pressed & 0x2000 != 0) {
        Some(OverlayKey::Back)
    } else {
        None
    }
}

pub(crate) fn gameplay_active(menu: Option<Res<Menu>>) -> bool {
    menu.is_none_or(|m| !m.open && !m.fe_suspended)
}

/// Retail "Camera Angle" (Game Settings > Control Settings), in the SKATER section.
const CAMERA_ANGLE_ROW: usize = 5;
/// Rail categories: key (`open_section`), shown name, description.
const SECTIONS: &[(&str, &str, &str)] = &[
    ("MAPS", "Maps", "Choose a map, then pick your drop-in spot."),
    ("SKATER", "Skater", "Difficulty, custom tuning, camera angle and your skater's look."),
    ("GRAPHICS", "Graphics", "Display, framerate, world lighting and sound settings."),
    ("MULTIPLAYER", "Multiplayer", "Host or join a session. A session is better with friends."),
    ("EXTRAS", "Extras", "Mods, updates and connected controllers."),
    ("OPT_INS", "Opt-Ins", "Every feature that is off by default or not retail."),
];
/// Section header rows (lime bars, not selectable).
const HEADERS: &[(usize, &str)] = &[
    (900, "Display"), (901, "World"), (902, "Audio"), (903, "Interface"), (904, "Difficulty"), (905, "Custom Tuning"),
    (906, "Camera"), (907, "Character"), (908, "Mods & Updates"), (909, "Controllers"), (910, "Teleport Spots"),
    (912, "Game Files"),
    (911, "Living World"),
    (913, "Opt-In Features"),
    (914, "Water"),
    (915, "Replay"),
    (916, "Objects"),
    (917, "Debug"),
    (918, "Performance"),
];

/// How a row behaves (the recomp's `RowSpec::Kind`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Header,
    /// Left / Right step and stop at the ends (unless `wrap`); Select cycles.
    Enum { index: usize, count: usize, wrap: bool },
    /// Left / Right step; the mouse drags the track. `t` is the fill, 0..1.
    Slider { t: f32 },
    Action,
    /// Select starts typing into it.
    Text,
    /// Read-only: selectable so its description shows.
    Info,
}
#[derive(Clone, Debug)]
struct RowView {
    kind: Kind,
    label: String,
    value: String,
    desc: String,
    enabled: bool,
    danger: bool,
    /// Y / R puts it back to its default.
    reset: bool,
}
impl RowView {
    fn new(kind: Kind, label: impl Into<String>, value: impl Into<String>, desc: impl Into<String>) -> Self {
        Self { kind, label: label.into(), value: value.into(), desc: desc.into(), enabled: true, danger: false, reset: false }
    }
    fn action(label: impl Into<String>, desc: impl Into<String>) -> Self {
        Self::new(Kind::Action, label, "", desc)
    }
    fn reset(mut self) -> Self { self.reset = true; self }
    fn danger(mut self) -> Self { self.danger = true; self }
    fn selectable(&self) -> bool { self.kind != Kind::Header && self.enabled }
    /// Whether a Left (-1) / Right (+1) step does anything (the recomp greys the stepper out at the end).
    fn can_step(&self, direction: i32) -> bool {
        match self.kind {
            Kind::Enum { index, count, wrap } => self.enabled && count > 1 && (wrap || if direction < 0 { index > 0 } else { index + 1 < count }),
            Kind::Slider { t } => self.enabled && if direction < 0 { t > 0. } else { t < 1. },
            _ => false,
        }
    }
}
/// What the rows read beyond the menu itself.
struct RowCtx<'a> {
    audio: Option<&'a crate::game_audio::AudioSettings>,
    camera: &'a crate::camera::CameraAngleSettings,
    net: &'a crate::multiplayer::Multiplayer,
    controllers: Option<&'a crate::input::ControllerInput>,
    debug: &'a [String],
    window: UVec2,
    /// The living world opt-ins in effect (`living_world::opt_ins::in_effect`).
    opt_ins: crate::living_world::opt_ins::OptIns,
    /// The player's Better Water choice (`water_look::WaterLookSettings`) and the mod patching it, if any.
    better_water: (bool, Option<String>),
}
fn pick<T: PartialEq>(values: &[T], value: &T) -> Kind {
    Kind::Enum { index: values.iter().position(|v| v == value).unwrap_or(0), count: values.len(), wrap: false }
}
impl Menu {
    fn rows(&self) -> Vec<usize> {
        if self.daylight { return (0..4).collect(); }
        if self.multiplayer {
            return if self.browser { std::iter::once(0).chain(1..=self.browser_count.min(5)).chain([6,7,8,10]).collect() } else { match self.network_page {
                1 => vec![3,4,10],
                2 => vec![5,7,10],
                3 => vec![0,1,10],
                4 => (20..27).chain([10]).collect(),
                _ => vec![2,6,13,14,15],
            }};
        }
        match self.section {
            0 if self.map_detail => {
                let mut rows = vec![50, 51];
                if let Some(path) = self.maps.get(self.selected_map).and_then(|m| m.path.as_deref()) {
                    rows.extend(self.destinations.iter().enumerate().filter(|(_,d)| d.matrix.is_some() && crate::teleport_menu::same_map(path, &d.map)).map(|(i,_)| 1_000_000 + i));
                }
                rows
            }
            0 => (1000..1000 + self.maps.len()).collect(),
            1 if self.difficulty == Difficulty::Custom => std::iter::once(3).chain(300..337).chain([CAMERA_ANGLE_ROW,8,10]).collect(),
            1 => vec![3, CAMERA_ANGLE_ROW, 8, 10],
            2 => vec![0, 1, 2, 13, 16, 17, 18, FRAME_STATS_ROW, FRAME_STATS_CORNER_ROW, SESSION_TIMER_ROW],
            // Quitting is the rail's Close Game; Refresh Game Files only while the game files are out of date.
            4 => [11, 14, 19].into_iter().chain((!crate::setup::stale_groups().is_empty()).then_some(REFRESH_ROW)).collect(),
            // Every opt-in (off by default or not retail) in one place, in row order (user 2026-10-25, USER-ASKS 336).
            5 => [NPC_DRAW_DISTANCE_ROW, BETTER_WATER_ROW, MORE_AUDIBLE_ROW, SPEECH_ROW, EXTENDED_REPLAYS_ROW, REPLAY_LENGTH_ROW,
                BETTER_MOVER_ROW, DEBUG_MENUS_ROW, FLY_CAM_ROW, PHYSICS_THREADS_ROW].into_iter().chain(OPT_IN_ROWS).collect(),
            i if i >= SECTIONS.len() => self.custom_sections.get(i-SECTIONS.len()).map_or(Vec::new(), |(_,entries)| (200..200+entries.len()).collect()),
            _ => Vec::new(),
        }
    }
    /// The content column: `rows` with the section header bars in front of their groups.
    fn page(&self) -> Vec<usize> {
        let plain = !self.multiplayer && !self.daylight;
        let mut page = Vec::new();
        for id in self.rows() {
            let header = match (plain, self.section, id) {
                (true, 2, 0) => Some(900),
                (true, 2, 13) => Some(901),
                (true, 2, 16) => Some(902),
                (true, 2, FRAME_STATS_ROW) => Some(903),
                (true, 5, NPC_DRAW_DISTANCE_ROW) => Some(911),
                (true, 5, BETTER_WATER_ROW) => Some(914),
                (true, 5, MORE_AUDIBLE_ROW) => Some(902),
                (true, 5, EXTENDED_REPLAYS_ROW) => Some(915),
                (true, 5, BETTER_MOVER_ROW) => Some(916),
                (true, 5, DEBUG_MENUS_ROW) => Some(917),
                (true, 5, PHYSICS_THREADS_ROW) => Some(918),
                (true, 5, id) if id == OPT_IN_ROWS.start => Some(913),
                (true, 1, 3) => Some(904),
                (true, 1, 300) => Some(905),
                (true, 1, CAMERA_ANGLE_ROW) => Some(906),
                (true, 1, 8) => Some(907),
                (true, 4, 11) => Some(908),
                (true, 4, 19) => Some(909),
                (true, 4, REFRESH_ROW) => Some(912),
                (true, 0, id) if id >= 1_000_000 && page.last().is_none_or(|last| *last < 1_000_000) => Some(910),
                _ => None,
            };
            page.extend(header);
            page.push(id);
        }
        page
    }
    fn category_count(&self) -> usize {
        SECTIONS.len() + self.custom_sections.len()
    }
    fn category_name(&self, index: usize) -> &str {
        SECTIONS.get(index).map(|s| s.1).or_else(|| self.custom_sections.get(index - SECTIONS.len()).map(|(n, _)| n.as_str())).unwrap_or("")
    }
    fn category_desc(&self, index: usize) -> &'static str {
        SECTIONS.get(index).map_or("Menus your mods added.", |s| s.2)
    }
    /// The title over the columns: "Settings" like the recomp, or the sub-page's name.
    fn title(&self) -> String {
        if self.map_detail {
            self.maps.get(self.selected_map).map_or("Map".into(), |m| m.label.clone())
        } else if self.daylight {
            "Day & Night".into()
        } else if self.browser {
            "Find a Session".into()
        } else if self.multiplayer && self.network_page != 0 {
            match self.network_page { 1 => "Join a Friend", 2 => "Player & Session", 3 => "Local Testing", _ => "Multiplayer Debug" }.into()
        } else {
            "Settings".into()
        }
    }
    /// Open the menu on a section by its key (`SECTIONS`); the retail pause rows use this until their retail
    /// screens are ported.
    pub(crate) fn open_section(&mut self, name: &str) -> bool {
        let Some(section) = SECTIONS.iter().position(|s| s.0 == name) else { return false };
        self.open = true;
        self.select_section(section);
        true
    }
    fn select_section(&mut self, section: usize) {
        self.section = section;
        self.map_detail = false;
        self.multiplayer = section == 3;
        self.browser = false;
        self.network_page = 0;
        self.daylight = false;
        self.selected = self.rows().first().copied().unwrap_or(100 + section);
        self.status.clear();
    }
    /// One step back (Escape / pad B).
    fn back(&mut self) {
        if self.view.editing {
            self.view.editing = false;
        } else if self.multiplayer && self.browser {
            self.browser = false;
            self.selected = 6;
        } else if self.multiplayer && self.network_page != 0 {
            (self.browser, self.selected) = match self.network_page { 1 => (true, 8), 2 => (false, 13), 3 => (false, 14), _ => (false, 15) };
            self.network_page = 0;
        } else if self.map_detail {
            self.map_detail = false;
            self.selected = 1000 + self.selected_map;
        } else if self.daylight {
            self.daylight = false;
            self.selected = 13;
        } else if self.view.content {
            self.view.content = false;
        } else {
            self.open = false;
        }
        self.status.clear();
    }
    fn describe(&self, id: usize, cx: &RowCtx) -> RowView {
        if let Some((_, label)) = HEADERS.iter().find(|(h, _)| *h == id) {
            return RowView::new(Kind::Header, *label, "", "");
        }
        let s = &self.settings;
        if self.daylight {
            return match id {
                0 => {
                    let minutes = (s.hour * 60.).floor() as u32 % 1440;
                    RowView::new(Kind::Enum { index: (s.hour * 4.).round() as usize % 96, count: 96, wrap: true }, "Time of Day",
                        format!("{:02}:{:02}", minutes / 60, minutes % 60), "The time on custom maps, in 15-minute steps.").reset()
                }
                1 => RowView::new(pick(DAY_SPEEDS, &s.day_speed), "Cycle Speed",
                    if s.day_speed == 0 { "Frozen".into() } else { format!("{}x ({} min/day)", s.day_speed, 1440 / s.day_speed) },
                    "How fast the day passes on custom maps. Frozen keeps the time set above.").reset(),
                2 => RowView::new(Kind::Enum { index: s.ambient_level.map_or(0, |l| l as usize / 5 + 1), count: 22, wrap: false }, "Ambient Light",
                    s.ambient_level.map_or("Auto".into(), |l| format!("{l}%")),
                    "Ambient light on custom maps. Auto follows the time of day; retail maps keep their authored lighting.").reset(),
                _ => RowView::action("Back", "Back to the graphics settings."),
            };
        }
        let net = cx.net;
        if self.multiplayer && self.browser {
            return match id {
                0 => RowView::action("Refresh Sessions", "Fetch the list of public sessions again."),
                1..=5 => match net.browser_rows.get(id - 1) {
                    Some(r) => RowView::new(Kind::Action, r.map.clone(),
                        format!("{}/{}  #{}{}", r.players, r.capacity, r.id % 100000, if r.compatible { "" } else { "  update required" }),
                        if r.compatible { "Join this session." } else { "This session runs another version; update to join it." }),
                    None => RowView { enabled: false, ..RowView::action("--", "") },
                },
                6 => RowView::action("Previous Page", "Show the previous page of sessions."),
                7 => RowView::action("Next Page", "Show the next page of sessions."),
                8 => RowView::action("Join With a Code", "Join a friend's session with their connection code."),
                9 => RowView::action("Quit Game", "Quit to the desktop.").danger(),
                _ => RowView::action("Back to Multiplayer", "Back to the multiplayer menu."),
            };
        }
        if self.multiplayer {
            return match id {
                0 => RowView::action("Host Local Session", "Host a test session on this PC or network, without Steam."),
                1 => RowView::action("Join Local Session", "Join a local test session."),
                2 => RowView::action("Host a Session", "Host a Steam session your friends can join."),
                3 => RowView::new(Kind::Text, "Join Code", format!("{}{}", net.join_code, if self.view.editing && self.selected == 3 { "_" } else { "" }),
                    "Select, type your friend's connection code, then choose Join Session."),
                4 => RowView::action("Join Session", "Join the session with the code above."),
                5 => RowView::action("Leave Multiplayer", "Leave the current session.").danger(),
                6 => RowView::action("Find a Session", "Browse public lobbies and join a crew."),
                7 => RowView::new(Kind::Text, "Your Name", format!("{}{}", net.player_name, if self.view.editing && self.selected == 7 { "_" } else { "" }),
                    "Select to edit the name other players see."),
                8 => RowView::action("Resume", "Back to skating."),
                9 => RowView::action("Quit Game", "Quit to the desktop.").danger(),
                12 => RowView::action("Join With a Code", "Join a friend's session with their connection code."),
                13 => RowView::action("Player & Session", "Change your name or leave your current session."),
                14 => RowView::action("Advanced / Local Testing", "Host or join a local test session without Steam."),
                15 => RowView::action("Debug", "Live network and mod diagnostics. Diagnostics stay in this menu; gameplay shows names and ping only."),
                20..=26 => {
                    let text = cx.debug.get(id - 20).cloned().unwrap_or_default();
                    let (first, rest) = text.split_once('\n').unwrap_or((text.as_str(), ""));
                    RowView { enabled: !text.is_empty(), ..RowView::new(Kind::Info, first.to_owned(), "", rest.to_owned()) }
                }
                _ => RowView::action("Back", "Back to the multiplayer menu."),
            };
        }
        let audio_view = |row: usize, label: &str, desc: &str| {
            let percent = cx.audio.map_or(0, |a| a.percent(audio_row(row)));
            let muted = cx.audio.is_some_and(|a| a.muted()) && row == 16;
            RowView::new(Kind::Slider { t: percent as f32 / 100. }, label, format!("{percent}%"),
                if muted { format!("{desc} Audio is muted by --mute.") } else { desc.to_owned() }).reset()
        };
        match id {
            0 => RowView::new(pick(RESOLUTIONS, &(s.width, s.height)), "Resolution", format!("{} x {}", s.width, s.height),
                "The window size. Alt+Enter switches between windowed and borderless fullscreen.").reset(),
            1 => {
                let size = s.internal_size(cx.window);
                RowView::new(pick(SCALES, &s.scale), "Render Scale", format!("{}%  ({} x {})", s.scale, size.x, size.y),
                    "Renders the world at a share of the window size, then scales it up. Lower runs faster; the menus stay sharp.").reset()
            }
            2 => RowView::new(pick(LIMITS, &s.fps), "Framerate Cap", if s.fps == 0 { "Unlimited".into() } else { format!("{} FPS", s.fps) },
                "Caps the frame rate. Unlimited runs as fast as your PC allows.").reset(),
            3 => RowView::new(pick(&Difficulty::ALL, &self.difficulty), "Difficulty", self.difficulty.label(),
                "Physics difficulty. Custom starts from Easy and lets you tune each value below."),
            CAMERA_ANGLE_ROW => RowView::new(pick(&crate::camera::CameraAngle::ALL, &cx.camera.selected), "Camera Angle", cx.camera.selected.label(),
                match cx.camera.forced_by() {
                    Some(owner) => format!("The retail camera angle (Game Settings > Control Settings). Mod {owner} is forcing the camera right now."),
                    None => "The retail camera angle (Game Settings > Control Settings).".into(),
                }).reset(),
            300..=334 => {
                let (o, v) = (&OPTIONS[id - 300], self.custom.value(id - 300));
                if o.boolean {
                    RowView::new(Kind::Enum { index: (v != 0.) as usize, count: 2, wrap: false }, o.label, if v != 0. { "On" } else { "Off" }, o.description).reset()
                } else {
                    RowView::new(Kind::Slider { t: if o.max > 0. { v / o.max } else { 0. } }, o.label, format!("{v:.2}"), o.description).reset()
                }
            }
            335 => RowView::action("Apply Custom Tuning", "Applies the custom tuning to your skater and saves it."),
            336 => RowView::action("Reset Custom Tuning to Easy", "Restores every custom value to Easy. Apply afterwards to use them."),
            6 => RowView::action("Resume", "Back to skating."),
            7 => RowView::action("Quit Game", "Quit to the desktop.").danger(),
            8 => RowView::action("Character Customiser", "Change your skater's look."),
            9 => RowView::action("Multiplayer", "Host or join a session."),
            10 => RowView::action("Custom Models", "Choose custom character and board models."),
            11 => RowView::action("Updates", "Check for and install a new version."),
            12 => RowView::action("Teleport", "Pick a drop-in spot on this map."),
            13 => RowView::action("Day & Night", "Time of day, day cycle speed and ambient light for custom maps. Retail maps keep their authored lighting."),
            14 => RowView::action("Mods", "Turn mods on or off and open their menus."),
            REFRESH_ROW => RowView::new(Kind::Action, "Refresh Game Files", "Out of date",
                format!("Your game files were converted by an older version of the game ({}), so new features that need them may not work. Refresh converts only what changed, then the game starts again. It takes a few minutes; keep the setup window open.", crate::setup::stale_groups().iter().map(|g| group_name(g)).collect::<Vec<_>>().join(", "))).danger(),
            16 => audio_view(16, "Master Volume", "Overall game volume."),
            17 => audio_view(17, "Ambience Volume", "The world's ambience beds."),
            18 => audio_view(18, "Effects Volume", "Skating and world sound effects."),
            DEBUG_MENUS_ROW => RowView::new(Kind::Enum { index: s.debug_menus as usize, count: 2, wrap: false }, "Debug Menus",
                if s.debug_menus { "On  (not retail)" } else { "Retail" },
                "Shows the debug menus Skate 3 ships hidden, such as the Challenge Map's debug_list_all tab. They are developer leftovers and may not all work.").reset(),
            FLY_CAM_ROW => RowView::new(Kind::Enum { index: s.fly_cam as usize, count: 2, wrap: false }, "Fly Cam",
                if s.fly_cam { "On  (not retail)" } else { "Off" },
                "Lets D-pad Right + R3 switch to a free-flying camera while skating (B drops you at the camera, Y logs its position). A developer tool, off by default.").reset(),
            PHYSICS_THREADS_ROW => RowView::new(Kind::Enum { index: s.physics_threads_multi as usize, count: 2, wrap: false }, "Physics Step Threads",
                if s.physics_threads_multi { "Multi" } else { "Single (default)" },
                "Runs the physics step on one thread (faster and steadier on the test PCs) or on Bevy's multi-threaded scheduler. Applies at the next start; the launcher's Tools page sets it too.").reset(),
            BETTER_MOVER_ROW => RowView::new(Kind::Enum { index: s.better_object_mover as usize, count: 2, wrap: false }, "Better Object Mover",
                if s.better_object_mover { "On  (not retail)" } else { "Retail" },
                "Lets you pick up and move every prop. Retail only lets you move the props that have grab handles.").reset(),
            EXTENDED_REPLAYS_ROW => RowView::new(Kind::Enum { index: s.extended_replays as usize, count: 2, wrap: false }, "Extended Replays",
                if s.extended_replays { "On  (not retail)" } else { "Retail (1:30)" },
                "Keeps more of your session for the instant replay. Retail keeps the last 90 seconds; a longer replay uses more memory (about 85 MB a minute).").reset(),
            REPLAY_LENGTH_ROW => RowView {
                enabled: s.extended_replays,
                ..RowView::new(Kind::Slider { t: s.replay_seconds.saturating_sub(REPLAY_LENGTHS[0]) as f32 / 120. }, "Replay Length",
                    format!("{}:{:02}", s.replay_seconds / 60, s.replay_seconds % 60),
                    "How much the instant replay keeps with Extended Replays on: 2 to 4 minutes.").reset()
            },
            BETTER_WATER_ROW => {
                let (on, owner) = (cx.better_water.0, cx.better_water.1.as_deref());
                RowView::new(Kind::Enum { index: on as usize, count: 2, wrap: false }, "Better Water", if on { "On  (not retail)" } else { "Retail" },
                    match owner {
                        Some(owner) => format!("Not retail: finer ripples, an even blue reflection, an extra wave against tiling, calmer fountains and slower waves. Mod {owner} is changing the water look right now."),
                        None => "Not retail: finer ripples, an even blue reflection, an extra wave against tiling, calmer fountains and slower waves.".into(),
                    }).reset()
            }
            MORE_AUDIBLE_ROW => {
                let on = cx.audio.is_some_and(|a| a.more_audible_world_saved());
                RowView::new(Kind::Enum { index: on as usize, count: 2, wrap: false }, "More Audible World", if on { "On  (not retail)" } else { "Retail" },
                    "Not retail: more cars, peds and NPC skaters heard at once (8 / 24 / 3 instead of retail's 4 / 15 / 1). Takes effect at the next start.").reset()
            }
            row if OPT_IN_ROWS.contains(&row) => {
                let o = opt_in(row);
                let on = cx.opt_ins.get(o);
                RowView::new(Kind::Enum { index: on as usize, count: 2, wrap: false }, o.label(), if on { "On  (testing)" } else { "Off" }, o.hint())
            }
            SPEECH_ROW => {
                let (on, decoded) = cx.audio.map_or((false, false), |a| (a.decode_speech(), a.speech_decoded()));
                RowView::new(Kind::Enum { index: on as usize, count: 2, wrap: false }, "Ped Speech",
                    match (on, decoded) { (true, true) => "On", (true, false) => "On  (refresh to decode)", (false, _) => "Off" },
                    "Peds, pros and the announcer talk. Needs their speech decoded from your disc: about 3.4 GB more in the data folder and a few more minutes of setup. Switching it on offers Extras > Refresh Game Files, which decodes it.")
            }
            19 => {
                let slots = cx.controllers.map(controller_slots).unwrap_or_default();
                RowView::new(Kind::Info, "Controllers", if slots.is_empty() { "None connected".into() } else { format!("{} connected", slots.len()) },
                    if slots.is_empty() { "No controller is connected.".into() } else { slots.join("\n") })
            }
            FRAME_STATS_ROW => RowView::new(
                Kind::Enum { index: if !s.frame_stats { 0 } else if s.frame_stats_simple { 1 } else { 2 }, count: 3, wrap: false },
                "FPS Counter", if !s.frame_stats { "Off" } else if s.frame_stats_simple { "Simple" } else { "Verbose" },
                "On-screen frame-time readout. Simple shows fps, frame time, 1% low and the worst frame; Verbose shows every timing.").reset(),
            FRAME_STATS_CORNER_ROW => RowView::new(Kind::Enum { index: s.frame_stats_right as usize, count: 2, wrap: false },
                "FPS Counter Position", if s.frame_stats_right { "Top Right" } else { "Top Left" },
                "Which top corner the FPS counter sits in.").reset(),
            SESSION_TIMER_ROW => RowView::new(Kind::Enum { index: s.session_timer as usize, count: 2, wrap: false },
                "Session Timer", if s.session_timer { "On" } else { "Off" },
                "A small clock in the bottom left corner: the time since the game started, to name when something happened in a report.").reset(),
            NPC_DRAW_DISTANCE_ROW => RowView::new(pick(&skate_core::living_world::DrawDistance::MENU_STEPS, &s.npc_draw_distance), "NPC Draw Distance",
                draw_distance_label(s.npc_draw_distance), NPC_DRAW_DISTANCE_HINT).reset(),
            200..=263 => {
                let title = self.custom_sections.get(self.section.saturating_sub(SECTIONS.len())).and_then(|(_, e)| e.get(id - 200)).map(|(_, _, t)| t.clone()).unwrap_or_default();
                RowView::action(title, "Open this mod's menu.")
            }
            50 => RowView::action("All Maps", "Back to the map list."),
            51 => RowView::action("Skate From Default Spawn", "Load this map and skate from its default spawn."),
            1_000_000.. => RowView::action(self.destinations.get(id - 1_000_000).map(|d| d.name.clone()).unwrap_or_default(), "Travel to this spot."),
            1000.. => RowView::new(Kind::Action, self.maps.get(id - 1000).map(|m| m.label.clone()).unwrap_or_default(), "Spots",
                "Open this map's teleport spots, or skate it from the default spawn."),
            _ => RowView::action("", ""),
        }
    }
}

/// The image the world renders into (the presentation camera shows it); the replay's photos capture it, so they
/// hold the world without any UI.
#[derive(Resource)]
pub(crate) struct SceneTarget(pub(crate) Handle<Image>);
#[derive(Resource)]
struct FramePacer(Instant);
#[derive(Component)]
pub(crate) struct MenuLayoutRoot;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;

/// The presentation camera must exist before overlays select their UI target.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PresentationSetup;

pub(crate) struct GraphicsMenuPlugin;
impl Plugin for GraphicsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FramePacer(Instant::now()))
            .add_systems(PostStartup, setup.in_set(PresentationSetup))
            .add_systems(PreUpdate, (refresh_sections, interact, apply_custom_difficulty).chain().in_set(MenuInput).after(bevy::input::InputSystems))
            .add_systems(PreUpdate, finish_menu_travel.after(crate::map_transition::MapTransitionSet).before(crate::input::poll_controllers))
            // Challenge Map teleports (retail map screen), applied the same way once their map is loaded.
            .init_resource::<crate::challenge_map::ChallengeMapTravel>()
            .add_systems(PreUpdate, crate::challenge_map::finish_travel.after(crate::map_transition::MapTransitionSet).before(crate::input::poll_controllers))
            .add_systems(Update, crate::challenge_map::start_requested)
            .add_systems(PreUpdate, toggle_fullscreen.after(bevy::input::InputSystems))
            .add_systems(Update, preview_menu.before(view::view))
            .add_systems(Update, (crate::map_render::advance_day, apply, view::view, view::sync_blur, resize_menu).chain())
            // Every UI button prompt (PromptGlyph) shows the connected pad's SDL-labelled glyph.
            .init_resource::<crate::prompt_glyphs::PromptImages>()
            .add_systems(Update, crate::prompt_glyphs::sync_ui_glyphs.after(view::view))
            .add_systems(PostUpdate, crate::map_render::position_celestial_bodies.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, pace);
    }
}
fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    cameras: Query<Entity, (With<Camera3d>, Without<crate::video_screens::FeedCamera>)>,
    mut time: ResMut<Time<Virtual>>,
    living: Option<ResMut<crate::living_world::LivingWorldSettings>>,
) {
    let path = config
        .asset_root
        .parent()
        .unwrap_or(&config.asset_root)
        .join("settings/graphics.json");
    let mut settings = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<GraphicsSettings>(&bytes).unwrap_or_else(|e| {
            warn!("Graphics settings: {e}");
            GraphicsSettings::default()
        }),
        Err(_) => GraphicsSettings::default(),
    }
    .validated();
    if path.is_file() && settings.migrated() {
        info!("GRAPHICS_SETTINGS defaults updated to version {DEFAULTS_VERSION} (NPC Skater Physics on)");
        if let Ok(bytes) = serde_json::to_vec_pretty(&settings) {
            if let Err(e) = std::fs::write(&path, bytes) {
                warn!("Graphics settings: {e}");
            }
        }
    }
    // The session log shows what a run started with (later changes log as PC_OVERLAY).
    info!(
        "GRAPHICS_SETTINGS {}x{} scale {}% framerate cap {} npc draw distance {:.2} frame stats {}",
        settings.width,
        settings.height,
        settings.scale,
        if settings.fps == 0 { "unlimited".to_string() } else { settings.fps.to_string() },
        settings.npc_draw_distance,
        settings.frame_stats
    );
    if let Some(mut living) = living {
        living.set_user_draw_distance(settings.npc_draw_distance);
        // Saved choices, plus any `SKATE_*=1` of this run (not saved).
        living.set_user_opt_ins(settings.opt_ins.or(crate::living_world::opt_ins::OptIns::from_env()));
    }
    window
        .resolution
        .set_physical_resolution(settings.width, settings.height);
    let (present_mode, latency) = swapchain_overrides();
    window.present_mode = present_mode;
    window.desired_maximum_frame_latency = latency;
    let size = settings.internal_size(window.physical_size());
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    // The replay's photos and Save to PC read this image back (a copy source; rendering is unchanged).
    image.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_SRC;
    let target = images.add(image);
    for camera in &cameras {
        commands.entity(camera).insert((
            RenderTarget::Image(target.clone().into()),
            // Fixed policy, not a setting: see `render_capacity`.
            Msaa::Off,
        ));
        // Render the world before the presentation camera consumes its image.
        commands.entity(camera).insert(Camera {
            order: -1,
            ..default()
        });
    }
    let output = commands
        .spawn((Camera2d, Msaa::Off, IsDefaultUiCamera))
        .id();
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            ..default()
        },
        ImageNode::new(target.clone()),
        UiTargetCamera(output),
    ));
    let data = skate_data::collections::Collections::load(&config.asset_root).expect("validated startup collections");
    let custom_defaults = Tuning::defaults(&data).expect("validated Easy difficulty");
    let custom = Tuning::load(&config.asset_root, &data).expect("validated Custom difficulty");
    let maps = crate::map_library::discover(&config.asset_root);
    let mut destinations = crate::teleport_menu::load(&config.asset_root).unwrap_or_else(|e| { warn!("Map destinations: {e}"); Vec::new() });
    destinations.retain(|d| !d.fallback());
    view::spawn(&mut commands, output, &mut fonts, &mut images);
    commands.insert_resource(SceneTarget(target));
    let selected_map = maps.iter().position(|m| m.path.as_ref() == config.map_path.as_ref()).unwrap_or(0);
    if config.start_paused { time.pause(); }
    commands.insert_resource(Menu {
        open: config.start_paused,
        fe_suspended: false,
        selected: 1000,
        settings,
        path,
        difficulty: config.difficulty,
        custom, custom_defaults, custom_apply:false, custom_dirty:false,
        status: String::new(),
        maps,
        selected_map,
        multiplayer: false,
        browser: false, network_page: 0, browser_count: 0,
        daylight: false, section: 0, custom_sections: Vec::new(), map_detail: false, destinations, pending_travel: None,
        view: default(),
    });
}
/// GRAPHICS-section rows for the game_audio volume settings.
const FRAME_STATS_ROW: usize = 28;
const FRAME_STATS_CORNER_ROW: usize = 49;
const SESSION_TIMER_ROW: usize = 58;
/// The setup groups (`tools/asset_pipeline/versions.py`) by what the player knows them as.
fn group_name(group: &str) -> &str {
    match group {
        "core" => "skater and animation data",
        "hud" => "HUD and menus",
        "character" => "characters",
        "environment" => "lighting, water and effects",
        "maps" => "maps",
        "audio" => "audio",
        "livingworld" => "living world",
        other => other,
    }
}
/// EXTRAS row: restart into setup's refresh (shown only while `setup::stale_groups` is not empty).
const REFRESH_ROW: usize = 31;
const AUDIO_ROWS: std::ops::RangeInclusive<usize> = 16..=18;
/// OPT_INS-section rows (every opt-in, doc 26b "Opt-in switches in the PC overlay"): the non-retail "more audible"
/// world layout and setup's speech decode (header "Audio").
const MORE_AUDIBLE_ROW: usize = 32;
const SPEECH_ROW: usize = 33;
/// Physics Step Threads (header "Performance"; `physics_threads_multi`).
const PHYSICS_THREADS_ROW: usize = 39;
/// The living world opt-ins (header "Opt-In Features"), in `OptIn::ALL` order (zombie mode, NPC skater physics,
/// visualize grabs). One row per opt-in: the range follows the list.
const OPT_IN_ROWS: std::ops::Range<usize> = 40..40 + crate::living_world::opt_ins::OptIn::ALL.len();
fn opt_in(row: usize) -> crate::living_world::opt_ins::OptIn {
    crate::living_world::opt_ins::OptIn::ALL[row - OPT_IN_ROWS.start]
}
/// OPT_INS-section row of the NPC draw distance (QoL, not retail).
const NPC_DRAW_DISTANCE_ROW: usize = 29;
/// OPT_INS row: the water look layer (water_look.rs); off = the retail water.
const BETTER_WATER_ROW: usize = 30;
const EXTENDED_REPLAYS_ROW: usize = 34;
const REPLAY_LENGTH_ROW: usize = 35;
const BETTER_MOVER_ROW: usize = 36;
/// EXTRAS row: retail's hidden debug menus (not retail).
const DEBUG_MENUS_ROW: usize = 37;
/// OPT_INS row: the fly cam chord (`debug_cam`; header "Debug").
const FLY_CAM_ROW: usize = 38;
const NPC_DRAW_DISTANCE_HINT: &str = "NPC draw distance is a QoL option, not retail: peds, NPC skaters and cars appear farther out, with more of them to keep the density. Costs frame time. None turns off all NPC skaters, peds and cars.";
fn draw_distance_label(multiplier: f32) -> String {
    if multiplier == skate_core::living_world::DrawDistance::NONE { "None  (no NPCs, not retail)".into() } else if multiplier == skate_core::living_world::DrawDistance::RETAIL { "Retail".into() } else { format!("{multiplier}x  (not retail)") }
}
fn audio_row(row: usize) -> crate::game_audio::AudioRow {
    use crate::game_audio::AudioRow;
    match row { 16 => AudioRow::Master, 17 => AudioRow::Ambience, _ => AudioRow::Effects }
}
fn cycle<T: PartialEq + Copy>(values: &[T], value: T, direction: i32) -> T {
    let index = values.iter().position(|x| *x == value).unwrap_or(0) as i32;
    values[(index + direction).rem_euclid(values.len() as i32) as usize]
}
fn refresh_sections(mods: Res<crate::modding::Mods>, mut menu: ResMut<Menu>) {
    let mut groups = std::collections::BTreeMap::<String,Vec<(String,String,String)>>::new();
    for ((owner,key),definition) in &mods.custom_menus {
        if let Some(section)=&definition.section {
            groups.entry(section.clone()).or_default().push((owner.clone(),key.clone(),definition.title.clone()));
        }
    }
    let selected=menu.custom_sections.get(menu.section.saturating_sub(SECTIONS.len())).filter(|_|menu.section>=SECTIONS.len()).map(|(name,_)|name.clone());
    menu.custom_sections=groups.into_iter().collect();
    if let Some(name)=selected {
        if let Some(i)=menu.custom_sections.iter().position(|(n,_)|n==&name) {menu.section=SECTIONS.len()+i;} else {menu.select_section(0);}
    }
}

/// One row operation from the pad, the keyboard or the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    /// Left / Right on an enum or slider (already checked against its ends).
    Step(usize, i32),
    /// Select: actions run, enums cycle with wrap-around, text rows start typing.
    Activate(usize),
    /// Y / R.
    Reset(usize),
    /// Mouse drag on a slider track, 0..1.
    Drag(usize, f32),
}

pub(crate) fn interact(
    mut config: ResMut<crate::config::Config>,
    mut transition: ResMut<crate::map_transition::MapTransition>,
    mut customiser: ResMut<crate::customiser::Customiser>,
    mut custom_models: ResMut<crate::custom_models::CustomModels>,
    nav: Res<crate::customiser::Navigation>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut time: ResMut<Time<Virtual>>,
    (window, mouse, mut wheel, real, retail_pause): (Single<&Window, With<PrimaryWindow>>, Res<ButtonInput<MouseButton>>, MessageReader<MouseWheel>, Res<Time<Real>>, Option<Res<crate::retail_menu_stage::RetailPause>>),
    mut exit: MessageWriter<AppExit>,
    mut net: ResMut<crate::multiplayer::Multiplayer>,
    mut typing: MessageReader<bevy::input::keyboard::KeyboardInput>,
    (mut updater, mut audio, mut camera_angle, mut living, mut water_look): (ResMut<crate::updater::Updater>, ResMut<crate::game_audio::AudioSettings>, ResMut<crate::camera::CameraAngleSettings>, ResMut<crate::living_world::LivingWorldSettings>, Option<ResMut<crate::water_look::WaterLookSettings>>),
    travel: Res<crate::teleport_menu::Travel>,
    (mut mods, mut logged, cover): (ResMut<crate::modding::ModMenu>, Local<Option<(bool, String, String)>>, Option<Res<crate::map_transition::LoadingCover>>),
) {
    let wheel_notches: f32 = wheel.read().map(|e| if e.unit == MouseScrollUnit::Line { e.y } else { e.y / 40. }).sum();
    if transition.busy() {
        // The retail loading screen (FE 52) covers every load: the overlay closes (a load picked in it too). Without
        // the screen (a set exported before it) the overlay shows the progress.
        menu.open = !cover.is_some_and(|c| c.available);
        time.pause();
        return;
    }
    if travel.open || travel.closed_this_frame || customiser.open || custom_models.open || mods.open {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let enter = (keys.just_pressed(KeyCode::Enter) && !alt) || keys.just_pressed(KeyCode::NumpadEnter);
    let escape = keys.just_pressed(KeyCode::Escape);
    let f1 = keys.just_pressed(KeyCode::F1);
    let retail_pause = retail_pause.is_some_and(|r| r.0);
    match overlay_key(escape, f1, nav.pressed, menu.open, retail_pause, menu.fe_suspended) {
        Some(OverlayKey::Toggle) => {
            if nav.pressed & 0x0400 != 0 {
                info!("PC overlay toggled by the Guide button (open: {})", !menu.open);
            }
            let cause = if nav.pressed & 0x0400 != 0 { "Guide" } else if f1 { "F1" } else if escape { "Escape" } else { "Start" };
            info!("PC_OVERLAY {} by {cause}", if menu.open { "close" } else { "open" });
            menu.open = !menu.open;
        }
        Some(OverlayKey::Back) => {
            info!("PC_OVERLAY back by {}", if escape { "Escape" } else { "B" });
            menu.back();
        }
        None => {}
    }
    let held = nav.held();
    let opened = menu.open && !menu.view.shown;
    menu.view.shown = menu.open;
    if opened {
        menu.view.reset(held);
    }
    // Typing into a text row; Enter ends it (Escape / B already did through `back`).
    for event in typing.read() {
        if !menu.open || !menu.view.editing || !event.state.is_pressed() || !matches!(menu.selected, 3 | 7) {
            continue;
        }
        if menu.selected == 7 {
            let mut name = net.player_name.clone();
            if event.key_code == KeyCode::Backspace { name.pop(); }
            if let Some(text) = &event.text {
                for ch in text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')) {
                    if name.chars().count() < 16 { name.push(ch); }
                }
            }
            if name != net.player_name { net.set_player_name(name); }
            continue;
        }
        if event.key_code == KeyCode::Backspace {
            net.join_code.pop();
        }
        if let Some(text) = &event.text {
            for ch in text.chars().filter(|c| c.is_ascii_hexdigit() || *c == '-') {
                if net.join_code.len() < 40 {
                    net.join_code.push(ch);
                }
            }
        }
    }
    let mut act = None;
    if menu.open && menu.view.editing {
        if enter || held & 0x1000 & !menu.view.prev_pad != 0 {
            menu.view.editing = false;
        }
        menu.view.prev_pad = held;
    } else if menu.open {
        let menu: &mut Menu = &mut menu;
        menu.browser_count = net.browser_rows.len();
        let dt = real.delta_secs();
        let page = menu.page();
        let views: Vec<RowView> = {
            let cx = RowCtx { audio: Some(&*audio), camera: &camera_angle, net: &net, controllers: None, debug: &[], window: UVec2::ZERO, opt_ins: crate::living_world::opt_ins::in_effect(&living), better_water: water_look.as_ref().map_or((false, None), |w| (w.selected.enabled, w.patched_by().map(str::to_owned))) };
            page.iter().map(|id| menu.describe(*id, &cx)).collect()
        };
        let selectable: Vec<usize> = page.iter().zip(&views).filter(|(_, v)| v.selectable()).map(|(id, _)| *id).collect();
        let view_of = |id: usize| page.iter().position(|p| *p == id).map(|i| &views[i]);
        // ---- Intents (the recomp's GatherInput) ----
        let v = &mut menu.view;
        let pressed = held & !v.prev_pad;
        v.prev_pad = held;
        let mut keyboard = 0u16;
        for (key, bit) in [(KeyCode::ArrowUp, 1), (KeyCode::ArrowDown, 2), (KeyCode::ArrowLeft, 4), (KeyCode::ArrowRight, 8)] {
            if keys.pressed(key) { keyboard |= bit; }
        }
        let pad = pressed & !keyboard;
        let dir = |minus: u16, plus: u16| (held & plus != 0) as i32 - (held & minus != 0) as i32;
        let move_x = v.repeat_x.step(dir(4, 8), dt);
        let move_y = v.repeat_y.step(dir(1, 2), dt);
        let select = pad & 0x1000 != 0 || enter || keys.just_pressed(KeyCode::Space);
        let category_prev = pad & 0x0100 != 0 || keys.just_pressed(KeyCode::KeyQ) || (keys.just_pressed(KeyCode::Tab) && shift);
        let category_next = pad & 0x0200 != 0 || keys.just_pressed(KeyCode::KeyE) || (keys.just_pressed(KeyCode::Tab) && !shift);
        let reset = pad & 0x8000 != 0 || keys.just_pressed(KeyCode::KeyR);
        let apply_tuning = pad & 0x4000 != 0 || keys.just_pressed(KeyCode::KeyF);
        if pad != 0 {
            v.pad_active = true;
        }
        if keyboard & !v.prev_keyboard != 0 || enter || escape || keys.any_just_pressed([KeyCode::Space, KeyCode::KeyQ, KeyCode::KeyE, KeyCode::KeyR, KeyCode::KeyF, KeyCode::Tab]) {
            v.pad_active = false;
        }
        v.prev_keyboard = keyboard;
        // ---- Mouse: hover focuses rows, clicks hit last frame's layout, the wheel steps whole rows ----
        let cursor = window.cursor_position();
        let moved = match (cursor, v.mouse_last) {
            (Some(c), Some(last)) => (c - last).abs().max_element() > 2.,
            _ => false,
        };
        if cursor.is_some() && (moved || v.mouse_last.is_none()) {
            v.mouse_last = cursor;
        }
        if moved {
            v.pad_active = false;
        }
        let hit = cursor.and_then(|c| v.hit(c));
        let over_content = cursor.is_some_and(|c| v.over_content(c));
        if over_content {
            v.wheel += wheel_notches;
            let whole = v.wheel.trunc();
            v.scroll_rows -= whole as i32;
            v.wheel -= whole;
        } else {
            v.wheel = 0.;
        }
        if !mouse.pressed(MouseButton::Left) {
            v.drag = None;
        }
        let clicked = mouse.just_pressed(MouseButton::Left);
        if moved && let Some(view::Hit::Row(id) | view::Hit::Left(id) | view::Hit::Right(id) | view::Hit::Slider(id, ..)) = hit {
            if selectable.contains(&id) {
                v.content = true;
                menu.selected = id;
            }
        }
        let v = &mut menu.view;
        if let Some((id, x0, x1)) = v.drag.and_then(|id| v.slider_track(id).map(|(a, b)| (id, a, b))) {
            if let Some(c) = cursor {
                act = Some(Act::Drag(id, ((c.x - x0) / (x1 - x0).max(1.)).clamp(0., 1.)));
            }
        }
        if clicked {
            match hit {
                Some(view::Hit::Rail(i)) => {
                    menu.view.rail_quit = false;
                    menu.view.content = false;
                    if i != menu.section { menu.select_section(i); }
                }
                Some(view::Hit::Quit) => { exit.write(AppExit::Success); }
                Some(view::Hit::Left(id) | view::Hit::Right(id)) if selectable.contains(&id) => {
                    let direction = if matches!(hit, Some(view::Hit::Left(_))) { -1 } else { 1 };
                    menu.view.content = true;
                    menu.selected = id;
                    // A click on a greyed-out stepper is swallowed (the recomp's rule), not a row cycle.
                    if view_of(id).is_some_and(|r| r.can_step(direction)) { act = Some(Act::Step(id, direction)); }
                }
                Some(view::Hit::Slider(id, x0, x1)) if selectable.contains(&id) => {
                    menu.view.content = true;
                    menu.selected = id;
                    menu.view.drag = Some(id);
                    if let Some(c) = cursor { act = Some(Act::Drag(id, ((c.x - x0) / (x1 - x0).max(1.)).clamp(0., 1.))); }
                }
                Some(view::Hit::Row(id)) if selectable.contains(&id) => {
                    menu.view.content = true;
                    menu.selected = id;
                    if !matches!(view_of(id).map(|r| r.kind), Some(Kind::Slider { .. })) { act = Some(Act::Activate(id)); }
                }
                _ => {}
            }
        }
        // ---- Apply the intents (the recomp's OnDraw navigation) ----
        let categories = menu.category_count();
        if category_prev || category_next {
            let next = (menu.section + if category_next { 1 } else { categories - 1 }) % categories;
            menu.select_section(next);
            menu.view.rail_quit = false;
        } else if !menu.view.content && move_y != 0 {
            let at = if menu.view.rail_quit { categories } else { menu.section };
            let to = (at as i32 + move_y).clamp(0, categories as i32) as usize;
            menu.view.rail_quit = to == categories;
            if to < categories && to != menu.section {
                menu.select_section(to);
            }
        }
        // A page change above makes `selectable` stale; the next frame acts on the new page.
        let page_changed = menu.page() != page;
        if menu.view.content && !page_changed {
            // Keep focus on a selectable row: the nearest one, like the recomp's clamp_focus_to_selectable.
            if !selectable.contains(&menu.selected) {
                let at = page.iter().position(|p| *p == menu.selected).unwrap_or(0);
                match page.iter().enumerate().filter(|(_, id)| selectable.contains(id)).min_by_key(|(i, _)| i.abs_diff(at)) {
                    Some((_, id)) => menu.selected = *id,
                    None => menu.view.content = false,
                }
            }
        }
        if page_changed {
        } else if !menu.view.content {
            if menu.view.rail_quit {
                if select { exit.write(AppExit::Success); }
            } else if (select || move_x > 0) && !selectable.is_empty() {
                menu.view.content = true;
                menu.selected = selectable[0];
                menu.view.follow = true;
            }
        } else if act.is_none() {
            if move_y != 0 {
                let at = selectable.iter().position(|id| *id == menu.selected).unwrap_or(0) as i32;
                menu.selected = selectable[(at + move_y).clamp(0, selectable.len() as i32 - 1) as usize];
                menu.view.follow = true;
            }
            if move_x != 0 && view_of(menu.selected).is_some_and(|r| r.can_step(move_x)) {
                act = Some(Act::Step(menu.selected, move_x));
            } else if select {
                act = Some(Act::Activate(menu.selected));
                menu.view.follow = true;
            } else if reset && view_of(menu.selected).is_some_and(|r| r.reset) {
                act = Some(Act::Reset(menu.selected));
            }
        }
        if apply_tuning && menu.custom_dirty {
            menu.custom_apply = true;
        }
        if let Some(a) = act {
            let id = match a { Act::Step(id, _) | Act::Activate(id) | Act::Reset(id) | Act::Drag(id, _) => id };
            let row = view_of(id);
            // Drags repeat every frame; their result is logged as the status / save line.
            if !matches!(a, Act::Drag(..)) {
                info!("PC_OVERLAY {a:?} {:?} was {:?} (section {}, page {:?})", row.map_or("", |r| r.label.as_str()),
                    row.map_or("", |r| r.value.as_str()), menu.category_name(menu.section), page.first());
            }
        }
        if let Some(Act::Activate(id)) = act {
            match view_of(id).map(|r| r.kind) {
                Some(Kind::Text) => { menu.view.editing = true; act = None; }
                Some(Kind::Slider { .. } | Kind::Info | Kind::Header) | None => act = None,
                _ => {}
            }
        }
    }

    // Rows that move between pages.
    if let Some(Act::Activate(row)) = act {
        if (200..264).contains(&row) {
            if let Some((_,entries))=menu.custom_sections.get(menu.section.saturating_sub(SECTIONS.len())) {
                if let Some((owner,key,_))=entries.get(row-200) {mods.open_registered(owner.clone(),key.clone());}
            }
            act=None;
        } else if row == 50 {
            menu.map_detail = false;
            menu.selected = 1000 + menu.selected_map;
            menu.status.clear();
            act = None;
        } else if row == 51 || row >= 1_000_000 {
            if let Some(entry) = menu.maps.get(menu.selected_map).cloned() {
                let matrix = row.checked_sub(1_000_000).and_then(|i| menu.destinations.get(i)).and_then(|d| d.matrix);
                if net.active() && entry.path != config.map_path {
                    menu.status = "Leave multiplayer before switching maps".into();
                } else if let Some(matrix) = matrix {
                    menu.pending_travel = Some((entry.path.clone(), matrix));
                    if entry.path != config.map_path { transition.request(entry); }
                    menu.status = "Travelling to your spot...".into();
                } else if row == 51 {
                    if net.active() { menu.status = "Leave multiplayer before reloading a map".into(); }
                    else { transition.request(entry); menu.status = "Loading map...".into(); }
                }
            }
            act = None;
        } else if row >= 1000 {
            menu.selected_map = row - 1000;
            menu.map_detail = true;
            menu.selected = 51;
            menu.status.clear();
            act = None;
        }
    }

    let day_action = menu.daylight;
    let saved_row = match act {
        Some(Act::Reset(row)) => {
            if row == BETTER_WATER_ROW && !menu.daylight && !menu.multiplayer {
                if let Some(look) = water_look.as_mut() { menu.status = set_better_water(look, false); }
            } else {
                reset_row(&mut menu, row, &mut audio, &mut camera_angle, &mut living);
            }
            Some(row)
        }
        Some(Act::Drag(row, fraction)) => {
            // Only the replay length saves on a drag (audio saves itself).
            let mut return_saved = None;
            if (300..335).contains(&row) {
                let before = menu.custom.value(row - 300);
                menu.custom.set_fraction(row - 300, fraction);
                if menu.custom.value(row - 300) != before { menu.custom_dirty = true; }
            } else if AUDIO_ROWS.contains(&row) {
                let percent = (fraction * 100.).round() as u32;
                if audio.percent(audio_row(row)) != (percent + 2) / 5 * 5 {
                    menu.status = audio.set_percent(audio_row(row), percent);
                }
            } else if row == REPLAY_LENGTH_ROW && menu.settings.extended_replays {
                let seconds = REPLAY_LENGTHS[((fraction.clamp(0., 1.) * (REPLAY_LENGTHS.len() - 1) as f32).round()) as usize];
                if seconds != menu.settings.replay_seconds {
                    menu.settings.replay_seconds = seconds;
                    return_saved = Some(row);
                }
            }
            return_saved
        }
        Some(Act::Step(row, _) | Act::Activate(row)) if row == BETTER_WATER_ROW && !menu.daylight && !menu.multiplayer => {
            if let Some(look) = water_look.as_mut() {
                let on = !look.selected.enabled;
                menu.status = set_better_water(look, on);
            }
            Some(row)
        }
        Some(Act::Step(row, _) | Act::Activate(row)) => {
            let direction = if let Some(Act::Step(_, d)) = act { d } else { 1 };
            run_row(&mut menu, row, direction, (&mut *config, &mut *physics, &mut *net, &mut exit, &mut *customiser,
                &mut *custom_models, &mut *updater, &mut *audio, &mut *camera_angle, &mut *mods, &mut *living));
            Some(row)
        }
        None => None,
    };
    if let (Some(row), Some(Act::Step(..) | Act::Reset(..))) = (saved_row, act) {
        let cx = RowCtx { audio: Some(&*audio), camera: &camera_angle, net: &net, controllers: None, debug: &[], window: UVec2::ZERO, opt_ins: crate::living_world::opt_ins::in_effect(&living), better_water: water_look.as_ref().map_or((false, None), |w| (w.selected.enabled, w.patched_by().map(str::to_owned))) };
        let now = menu.describe(row, &cx);
        info!("PC_OVERLAY now {:?} = {:?}", now.label, now.value);
    }
    if let Some(row) = saved_row {
        if ((row < 3 || row == FRAME_STATS_ROW || row == FRAME_STATS_CORNER_ROW || row == SESSION_TIMER_ROW || row == NPC_DRAW_DISTANCE_ROW || row == EXTENDED_REPLAYS_ROW || row == REPLAY_LENGTH_ROW || row == BETTER_MOVER_ROW || row == DEBUG_MENUS_ROW || row == FLY_CAM_ROW || row == PHYSICS_THREADS_ROW || OPT_IN_ROWS.contains(&row)) && !menu.multiplayer && !menu.daylight && !day_action) || (day_action && row < 3) {
            let save = (|| -> Result<(), String> {
                std::fs::create_dir_all(menu.path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(
                    &menu.path,
                    serde_json::to_vec_pretty(&menu.settings).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())
            })();
            menu.status = match save {
                Ok(()) => "Saved".into(),
                Err(e) => format!("Could not save: {e}"),
            };
        }
    }
    // Session log: open / closed, where it is (category, sub-page title), and every status message (saves, errors).
    let now = (menu.open, format!("{} / {}", menu.category_name(menu.section), menu.title()), menu.status.clone());
    if logged.as_ref() != Some(&now) {
        let before = logged.replace(now.clone());
        if now.0 && before.as_ref().is_none_or(|b| b.1 != now.1 || !b.0) {
            info!("PC_OVERLAY at {}", now.1);
        }
        if !now.2.is_empty() && before.as_ref().is_none_or(|b| b.2 != now.2) {
            info!("PC_OVERLAY status: {}", now.2.replace('\n', " | "));
        }
    }
    if (menu.open || menu.fe_suspended) && !net.active() {
        time.pause();
    } else {
        time.unpause();
    }
}

type RowResources<'a, 'w> = (
    &'a mut crate::config::Config,
    &'a mut crate::physics::GamePhysics,
    &'a mut crate::multiplayer::Multiplayer,
    &'a mut MessageWriter<'w, AppExit>,
    &'a mut crate::customiser::Customiser,
    &'a mut crate::custom_models::CustomModels,
    &'a mut crate::updater::Updater,
    &'a mut crate::game_audio::AudioSettings,
    &'a mut crate::camera::CameraAngleSettings,
    &'a mut crate::modding::ModMenu,
    &'a mut crate::living_world::LivingWorldSettings,
);

/// Step or activate one row (`direction` +1 for Select, which cycles enums with wrap-around).
fn run_row(menu: &mut Menu, row: usize, direction: i32, r: RowResources) {
    let (config, physics, net, exit, customiser, custom_models, updater, audio, camera_angle, mods, living) = r;
    if menu.daylight {
        match row {
            0 => menu.settings.hour = ((menu.settings.hour * 4.).round() + direction as f32).rem_euclid(96.) / 4.,
            1 => menu.settings.day_speed = cycle(DAY_SPEEDS, menu.settings.day_speed, direction),
            2 => {
                // Auto, 0%, 5%, ... 100%, then Auto again.
                let index = menu.settings.ambient_level.map_or(0, |level| level as i32 / 5 + 1);
                let next = (index + direction).rem_euclid(22);
                menu.settings.ambient_level = if next == 0 { None } else { Some((next as u32 - 1) * 5) };
            }
            _ => { menu.daylight = false; menu.selected = 13; }
        }
    } else if menu.browser {
        match row {
            0 => net.browse(0),
            1..=5 => net.join_row(row - 1),
            6 => {
                let page = net.browser_page.saturating_sub(1);
                net.browse(page);
            }
            7 => {
                let page = net.browser_page + 1;
                if page * 5 < net.browser_total {
                    net.browse(page);
                }
            }
            8 => { menu.browser=false;menu.network_page=1;menu.selected=3; },
            9 => {
                exit.write(AppExit::Success);
            }
            10 => {
                menu.browser = false;
                menu.selected = 6;
            }
            _ => {}
        }
    } else if menu.multiplayer {
        match row {
            0 => net.local(true),
            1 => net.local(false),
            2 => net.steam(true),
            3 => {}
            4 => net.steam(false),
            5 => net.leave(),
            6 => {
                net.browse(0);
                menu.browser = true;
                menu.selected = 0;
            }
            12 => { menu.browser=false;menu.network_page=1;menu.selected=3; },
            8 => menu.open = false,
            9 => {
                exit.write(AppExit::Success);
            }
            10 => menu.back(),
            13 => { menu.network_page = 2; menu.selected = 7; }
            14 => { menu.network_page = 3; menu.selected = 0; }
            15 => { menu.network_page = 4; menu.selected = 20; }
            _ => {}
        }
    } else {
        match row {
            300..=334 => { menu.custom.adjust(row-300,direction); menu.custom_dirty=true; },
            335 => menu.custom_apply=true,
            336 => { menu.custom=menu.custom_defaults.clone(); menu.custom_dirty=true; menu.status="Easy values restored. Select Apply Custom Tuning to use them.".into(); },
            0 => {
                let size = cycle(
                    RESOLUTIONS,
                    (menu.settings.width, menu.settings.height),
                    direction,
                );
                (menu.settings.width, menu.settings.height) = size;
            }
            1 => menu.settings.scale = cycle(SCALES, menu.settings.scale, direction),
            2 => menu.settings.fps = cycle(LIMITS, menu.settings.fps, direction),
            3 => {
                menu.difficulty = cycle(&Difficulty::ALL, menu.difficulty, direction);
                physics.set_difficulty(menu.difficulty);
                config.difficulty = menu.difficulty;
                menu.status = match menu.difficulty.save(&config.asset_root) {
                    Ok(()) => "Difficulty saved".into(),
                    Err(e) => format!("Applied, but could not save: {e}"),
                };
            }
            CAMERA_ANGLE_ROW => {
                camera_angle.selected = cycle(&crate::camera::CameraAngle::ALL, camera_angle.selected, direction);
                menu.status = camera_saved(camera_angle);
            }
            6 => menu.open = false,
            7 => {
                exit.write(AppExit::Success);
            }
            8 => { custom_models.request_stock(); customiser.begin(); },
            9 => {
                menu.multiplayer = true;
                menu.selected = 0;
            }
            10 => custom_models.begin(),
            11 => menu.status = updater.open(false),
            12 => {
                menu.select_section(0);
                menu.selected_map = menu.maps.iter().position(|m| m.path == config.map_path).unwrap_or(0);
                menu.map_detail = true;
                menu.selected = 51;
            },
            13 => { menu.daylight = true; menu.selected = 0; },
            14 => mods.begin(),
            REFRESH_ROW => match crate::setup::restart_with_refresh() {
                Ok(()) => {
                    info!("PC_OVERLAY refresh game files: restarting into setup");
                    exit.write(AppExit::Success);
                }
                Err(e) => menu.status = e,
            },
            16..=18 => menu.status = audio.adjust(audio_row(row), direction),
            MORE_AUDIBLE_ROW => {
                let on = !audio.more_audible_world_saved();
                crate::living_world::opt_ins::log_opt_in_changed("more_audible_world", !on, on, "player");
                menu.status = format!("{}. Takes effect at the next start.", audio.set_more_audible_world(on));
            }
            SPEECH_ROW => {
                let on = !audio.decode_speech();
                crate::living_world::opt_ins::log_opt_in_changed("decode_speech", !on, on, "player");
                let saved = audio.set_decode_speech(on);
                menu.status = match (on, audio.speech_decoded()) {
                    (true, false) => format!("{saved}. Extras > Refresh Game Files decodes the speech (about 3.4 GB, a few minutes)."),
                    (false, true) => format!("{saved}. The decoded speech stays; setup skips it from the next refresh."),
                    _ => saved,
                };
            }
            EXTENDED_REPLAYS_ROW => menu.settings.extended_replays = !menu.settings.extended_replays,
            BETTER_MOVER_ROW => menu.settings.better_object_mover = !menu.settings.better_object_mover,
            PHYSICS_THREADS_ROW => {
                menu.settings.physics_threads_multi = !menu.settings.physics_threads_multi;
                crate::living_world::opt_ins::log_opt_in_changed("physics_threads_multi", !menu.settings.physics_threads_multi, menu.settings.physics_threads_multi, "player");
            }
            DEBUG_MENUS_ROW => menu.settings.debug_menus = !menu.settings.debug_menus,
            FLY_CAM_ROW => menu.settings.fly_cam = !menu.settings.fly_cam,
            FRAME_STATS_CORNER_ROW => menu.settings.frame_stats_right = !menu.settings.frame_stats_right,
            SESSION_TIMER_ROW => menu.settings.session_timer = !menu.settings.session_timer,
            REPLAY_LENGTH_ROW => menu.settings.replay_seconds = cycle(REPLAY_LENGTHS, menu.settings.replay_seconds, direction),
            FRAME_STATS_ROW => {
                // Off, Simple, Verbose (the original readout).
                let modes = [(false, false), (true, true), (true, false)];
                let s = &mut menu.settings;
                let now = if s.frame_stats { (true, s.frame_stats_simple) } else { (false, false) };
                (s.frame_stats, s.frame_stats_simple) = cycle(&modes, now, direction);
            }
            NPC_DRAW_DISTANCE_ROW => {
                menu.settings.npc_draw_distance = cycle(&skate_core::living_world::DrawDistance::MENU_STEPS, menu.settings.npc_draw_distance, direction);
                living.set_user_draw_distance(menu.settings.npc_draw_distance);
            }
            row if OPT_IN_ROWS.contains(&row) => {
                // Flips what is on now (a mod's value included); the player's choice is saved.
                let o = opt_in(row);
                let on = !crate::living_world::opt_ins::in_effect(living).get(o);
                let mut choices = living.user_opt_ins;
                choices.set(o, on);
                living.set_user_opt_ins(choices);
                menu.settings.opt_ins.set(o, on);
            }
            _ => {}
        }
    }
}
/// EXTRAS "Better Water": switches the water look layer and saves `settings/water-look.json`.
fn set_better_water(look: &mut crate::water_look::WaterLookSettings, on: bool) -> String {
    crate::living_world::opt_ins::log_opt_in_changed("better_water", look.selected.enabled, on, "player");
    look.selected.enabled = on;
    info!("WATER_LOOK better_water={on} owner={:?}", look.patched_by());
    match look.save() {
        Ok(()) if look.patched_by().is_some() => format!(
            "Better Water saved. Mod {} is changing the water look right now.", look.patched_by().unwrap_or_default()),
        Ok(()) => "Better Water saved".into(),
        Err(e) => format!("Applied, but could not save: {e}"),
    }
}
fn camera_saved(camera_angle: &crate::camera::CameraAngleSettings) -> String {
    match camera_angle.save() {
        Ok(()) if camera_angle.forced_by().is_some() => format!(
            "Camera angle saved. Mod {} is forcing the camera right now.",
            camera_angle.forced_by().unwrap_or_default()),
        Ok(()) => "Camera angle saved".into(),
        Err(e) => format!("Applied, but could not save: {e}"),
    }
}
/// Y / R: a row back to its default (the rows whose `RowView::reset` is set).
fn reset_row(menu: &mut Menu, row: usize, audio: &mut crate::game_audio::AudioSettings, camera_angle: &mut crate::camera::CameraAngleSettings,
    living: &mut crate::living_world::LivingWorldSettings) {
    let defaults = GraphicsSettings::default();
    let s = &mut menu.settings;
    if menu.daylight {
        match row {
            0 => s.hour = defaults.hour,
            1 => s.day_speed = defaults.day_speed,
            2 => s.ambient_level = defaults.ambient_level,
            _ => {}
        }
        return;
    }
    match row {
        0 => (s.width, s.height) = (defaults.width, defaults.height),
        1 => s.scale = defaults.scale,
        2 => s.fps = defaults.fps,
        FRAME_STATS_ROW => (s.frame_stats, s.frame_stats_simple) = (defaults.frame_stats, defaults.frame_stats_simple),
        FRAME_STATS_CORNER_ROW => s.frame_stats_right = defaults.frame_stats_right,
        SESSION_TIMER_ROW => s.session_timer = defaults.session_timer,
        EXTENDED_REPLAYS_ROW => s.extended_replays = defaults.extended_replays,
        BETTER_MOVER_ROW => s.better_object_mover = defaults.better_object_mover,
        PHYSICS_THREADS_ROW => s.physics_threads_multi = defaults.physics_threads_multi,
        DEBUG_MENUS_ROW => s.debug_menus = defaults.debug_menus,
        FLY_CAM_ROW => s.fly_cam = defaults.fly_cam,
        REPLAY_LENGTH_ROW => s.replay_seconds = defaults.replay_seconds,
        NPC_DRAW_DISTANCE_ROW => {
            s.npc_draw_distance = defaults.npc_draw_distance;
            living.set_user_draw_distance(s.npc_draw_distance);
        }
        CAMERA_ANGLE_ROW => {
            camera_angle.selected = crate::camera::CameraAngle::default();
            menu.status = camera_saved(camera_angle);
        }
        300..=334 => {
            let key = OPTIONS[row - 300].key;
            menu.custom.values.insert(key.into(), menu.custom_defaults.value(row - 300));
            menu.custom_dirty = true;
        }
        16..=18 => menu.status = audio.reset(audio_row(row)),
        MORE_AUDIBLE_ROW => {
            crate::living_world::opt_ins::log_opt_in_changed("more_audible_world", audio.more_audible_world_saved(), false, "player");
            menu.status = audio.set_more_audible_world(false);
        }
        _ => {}
    }
}
/// Read-only troubleshooting row: who is in each controller slot.
fn controller_slots(input: &crate::input::ControllerInput) -> Vec<String> {
    input.kinds.iter().enumerate()
        .filter_map(|(slot, kind)| kind.as_deref().map(|kind| format!("Slot {}: {}", slot + 1, kind.summary())))
        .collect()
}

fn toggle_fullscreen(
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<Menu>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !alt || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    match window.mode {
        WindowMode::Windowed => {
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        }
        WindowMode::BorderlessFullscreen(_) | WindowMode::Fullscreen(_, _) => {
            window.mode = WindowMode::Windowed;
            window
                .resolution
                .set_physical_resolution(menu.settings.width, menu.settings.height);
        }
    }
}
fn apply(
    menu: Res<Menu>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<SceneTarget>,
    mut images: ResMut<Assets<Image>>,
    mut previous: Local<Option<GraphicsSettings>>,
) {
    if window.mode == WindowMode::Windowed
        && previous
            .as_ref()
            .is_none_or(|p| p.width != menu.settings.width || p.height != menu.settings.height)
    {
        window
            .resolution
            .set_physical_resolution(menu.settings.width, menu.settings.height);
    }
    let size = menu.settings.internal_size(window.physical_size());
    if let Some(image) = images.get(&target.0) {
        if image.size() != size {
            images.get_mut(&target.0).unwrap().resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        }
    }
    *previous = Some(menu.settings.clone());
}
// Opt-in screenshot coverage for overlays; never changes an ordinary session.
fn preview_menu(config: Res<crate::config::Config>, mut menu: ResMut<Menu>, mut mods: ResMut<crate::modding::ModMenu>, mut done: Local<bool>) {
    if *done || config.verification_capture.is_none() { return; }
    *done = true;
    // `shown` keeps `interact` from resetting the focus as the menu opens.
    match std::env::var("SKATE_VERIFY_MENU").as_deref() {
        Ok("custom") => { menu.open = true; menu.select_section(1); menu.selected=300; menu.view.content = true; menu.view.shown = true; },
        Ok("graphics") => { menu.open = true; menu.select_section(2); menu.view.content = true; menu.view.shown = true; },
        Ok("npc") => { menu.open = true; menu.select_section(5); menu.selected = NPC_DRAW_DISTANCE_ROW; menu.view.content = true; menu.view.follow = true; menu.view.shown = true; },
        Ok("optin") => { menu.open = true; menu.select_section(5); menu.selected = menu.rows().last().copied().unwrap_or(SPEECH_ROW); menu.view.content = true; menu.view.follow = true; menu.view.shown = true; },
        Ok("stale") => { crate::setup::preview_stale(); menu.open = true; menu.select_section(4); menu.selected = REFRESH_ROW; menu.view.content = true; menu.view.follow = true; menu.view.shown = true; },
        Ok("maps") => { menu.open = true; menu.select_section(0); menu.view.content = true; menu.view.shown = true; },
        Ok("rail") => { menu.open = true; menu.select_section(2); },
        Ok("mods") => { menu.open = true; mods.begin(); }
        Ok("multiplayer") => { menu.open = true; menu.select_section(3); }
        _ => {}
    }
}
fn finish_menu_travel(
    mut menu: ResMut<Menu>, transition: Res<crate::map_transition::MapTransition>,
    current: Res<crate::map_transition::CurrentMap>, mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut time: ResMut<Time<Virtual>>,
) {
    if transition.busy() { return; }
    let Some((path, matrix)) = menu.pending_travel.take() else { return; };
    // A failed map transaction retains the previous world: never apply another map's coordinates there.
    if current.path != path { return; }
    match skater.travel_to(matrix) {
        Ok(()) => { menu.open = false; time.unpause(); }
        Err(e) => { menu.open = true; menu.status = format!("Could not travel: {e}"); }
    }
}
/// Scales the mod menu (`modding::menu`), which is laid out in 1280 x 800 design pixels.
fn resize_menu(
    window: Single<&Window, With<PrimaryWindow>>, roots: Query<Entity, With<MenuLayoutRoot>>,
    children: Query<&Children>, mut nodes: Query<(&mut Node, Option<&mut TextFont>)>,
    mut previous: Local<Option<f32>>,
) {
    let scale = (window.width() / 1280.).min(window.height() / 800.).max(0.25);
    let factor = scale / previous.unwrap_or(1.);
    if (factor - 1.).abs() < 0.0001 { return; }
    fn resize(value: &mut Val, factor: f32) { if let Val::Px(px) = value { *px *= factor; } }
    for root in &roots {
        for entity in children.iter_descendants(root) {
            if let Ok((mut node, font)) = nodes.get_mut(entity) {
                let node = &mut *node;
                for value in [&mut node.width, &mut node.height, &mut node.min_width, &mut node.min_height,
                    &mut node.max_width, &mut node.max_height, &mut node.row_gap, &mut node.column_gap,
                    &mut node.padding.left, &mut node.padding.right, &mut node.padding.top, &mut node.padding.bottom,
                    &mut node.margin.left, &mut node.margin.right, &mut node.margin.top, &mut node.margin.bottom] {
                    resize(value, factor);
                }
                if let Some(mut font) = font { font.font_size *= factor; }
            }
        }
    }
    *previous = Some(scale);
}
/// Swapchain diagnostics (frame-time stalls, 2026-10-17): `SKATE_PRESENT_MODE=fifo|mailbox|immediate` (default
/// AutoNoVsync) and `SKATE_FRAME_LATENCY=1..3` (default wgpu's); logged when either is set.
fn swapchain_overrides() -> (PresentMode, Option<std::num::NonZero<u32>>) {
    let mode = match std::env::var("SKATE_PRESENT_MODE").ok().as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("fifo") => PresentMode::Fifo,
        Some("mailbox") => PresentMode::Mailbox,
        Some("immediate") => PresentMode::Immediate,
        _ => PresentMode::AutoNoVsync,
    };
    let latency = std::env::var("SKATE_FRAME_LATENCY").ok().and_then(|v| v.parse::<u32>().ok()).and_then(std::num::NonZero::new);
    if mode != PresentMode::AutoNoVsync || latency.is_some() {
        info!("SWAPCHAIN_OVERRIDE present mode {mode:?}, frame latency {}", latency.map_or("default".to_string(), |l| l.to_string()));
    }
    (mode, latency)
}

fn pace(menu: Option<Res<Menu>>, mut pacer: ResMut<FramePacer>) {
    let Some(menu) = menu else {
        return;
    };
    if menu.settings.fps > 0 {
        let period = Duration::from_secs_f64(1. / f64::from(menu.settings.fps));
        if let Some(wait) = period.checked_sub(pacer.0.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    pacer.0 = Instant::now();
}
fn apply_custom_difficulty(mut menu: ResMut<Menu>, config: Res<crate::config::Config>,
    mut skater: ResMut<crate::physics::SkaterRuntime>) {
    if !std::mem::take(&mut menu.custom_apply) {return;}
    let result=(|| -> Result<(),String> {
        let mut data=skate_data::collections::Collections::load(&config.asset_root)?;
        menu.custom.overlay(&mut data)?;
        skater.reload_difficulty(&data)?;
        menu.custom.save(&config.asset_root)?;
        Ok(())
    })();
    menu.status=match result {
        Ok(())=> {menu.custom_dirty=false;"Custom tuning applied and saved.".into()},
        Err(e)=>format!("Custom tuning: {e}"),
    };
}

#[cfg(test)]
mod tests {
    /// Every opt-in row names an opt-in (removing two opt-ins once left rows that panicked when the overlay opened).
    #[test]
    fn every_opt_in_row_has_an_opt_in() {
        assert_eq!(OPT_IN_ROWS.len(), crate::living_world::opt_ins::OptIn::ALL.len());
        for row in OPT_IN_ROWS {
            let _ = opt_in(row);
        }
    }

    use super::*;
    fn menu() -> Menu {
        Menu {
            open: true, fe_suspended: false, selected: 1000, settings: GraphicsSettings::default(),
            path: PathBuf::new(), difficulty: Difficulty::Easy, custom:Tuning::default(), custom_defaults:Tuning::default(), custom_apply:false, custom_dirty:false, status: String::new(),
            maps: (0..40).map(|i| crate::map_library::Entry { label: format!("Map {i}"), path: None, world: None }).collect(),
            selected_map: 0, multiplayer: false, browser: false, network_page: 0, browser_count: 0, daylight: false, section: 0, custom_sections: Vec::new(), map_detail: false, destinations: Vec::new(), pending_travel: None,
            view: default(),
        }
    }
    #[test]
    fn guide_and_f1_toggle_escape_and_b_back_out_and_start_only_without_retail_menus() {
        const GUIDE: u16 = 0x0400;
        const START: u16 = 0x0010;
        const B: u16 = 0x2000;
        let toggle = Some(OverlayKey::Toggle);
        let back = Some(OverlayKey::Back);
        // Guide and F1 toggle it closed or open, with or without the retail menus, also over a retail screen.
        for (retail, suspended) in [(false, false), (true, false), (true, true)] {
            assert_eq!(overlay_key(false, false, GUIDE, false, retail, suspended), toggle);
            assert_eq!(overlay_key(false, false, GUIDE, true, retail, suspended), toggle);
            assert_eq!(overlay_key(false, true, 0, true, retail, suspended), toggle);
        }
        // Escape opens it and backs out while open, like pad B (the recomp's NavigateBack).
        assert_eq!(overlay_key(true, false, 0, false, true, true), toggle);
        assert_eq!(overlay_key(true, false, 0, true, true, false), back);
        assert_eq!(overlay_key(false, false, B, true, true, false), back);
        assert_eq!(overlay_key(false, false, B, false, true, false), None);
        assert_eq!(overlay_key(false, false, 0, true, true, false), None);
        // Start: the overlay only without the retail menus; with them it is the retail pause menu's.
        assert_eq!(overlay_key(false, false, START, false, false, false), toggle);
        assert_eq!(overlay_key(false, false, START, false, true, false), None);
        assert_eq!(overlay_key(false, false, START, false, false, true), None, "not over a retail screen");
    }
    #[test]
    fn back_steps_out_level_by_level() {
        let mut m = menu();
        m.select_section(0);
        m.map_detail = true;
        m.selected = 51;
        m.view.content = true;
        m.view.editing = true;
        m.back();
        assert!(!m.view.editing && m.map_detail, "a text edit first");
        m.back();
        assert!(!m.map_detail && m.selected == 1000 && m.view.content, "then the sub-page");
        m.back();
        assert!(!m.view.content && m.open, "then the row column");
        m.back();
        assert!(!m.open, "then the rail closes it");
        m.open = true;
        m.select_section(3);
        m.network_page = 1;
        m.back();
        assert!(m.browser && m.selected == 8, "join-by-code goes back to the browser");
        m.back();
        assert!(!m.browser && m.selected == 6, "the browser back to its row");
    }
    #[test]
    fn steppers_stop_at_the_ends_unless_the_row_wraps() {
        let row = |index, count, wrap| RowView::new(Kind::Enum { index, count, wrap }, "", "", "");
        assert!(!row(0, 3, false).can_step(-1) && row(0, 3, false).can_step(1));
        assert!(row(2, 3, false).can_step(-1) && !row(2, 3, false).can_step(1));
        assert!(row(0, 96, true).can_step(-1) && row(95, 96, true).can_step(1));
        let slider = |t| RowView::new(Kind::Slider { t }, "", "", "");
        assert!(!slider(0.).can_step(-1) && slider(0.).can_step(1) && !slider(1.).can_step(1));
        assert!(!RowView::action("", "").can_step(1));
        assert!(!RowView::new(Kind::Header, "", "", "").selectable());
    }
    #[test]
    fn pages_put_headers_in_front_of_their_groups() {
        let mut m = menu();
        m.select_section(2);
        assert_eq!(m.page(), vec![900, 0, 1, 2, 901, 13, 902, 16, 17, 18, 903, FRAME_STATS_ROW, FRAME_STATS_CORNER_ROW, SESSION_TIMER_ROW]);
        m.select_section(5);
        assert_eq!(m.page(), vec![911, NPC_DRAW_DISTANCE_ROW, 914, BETTER_WATER_ROW, 902, MORE_AUDIBLE_ROW, SPEECH_ROW, 915, EXTENDED_REPLAYS_ROW,
            REPLAY_LENGTH_ROW, 916, BETTER_MOVER_ROW, 917, DEBUG_MENUS_ROW, FLY_CAM_ROW, 918, PHYSICS_THREADS_ROW, 913, 40, 41, 42, 43, 44]);
        m.select_section(1);
        assert_eq!(m.page(), vec![904, 3, 906, CAMERA_ANGLE_ROW, 907, 8, 10]);
        m.select_section(4);
        assert_eq!(m.page(), vec![908, 11, 14, 909, 19]);
        m.select_section(3);
        assert_eq!(m.page(), m.rows(), "no headers in the multiplayer pages");
        m.select_section(0);
        m.maps[0].path = Some(PathBuf::from("University.skate"));
        m.destinations = vec![crate::teleport_menu::Destination { id: "uni".into(), name: "Campus".into(), map: "University".into(), world: None, label_key: None, matrix: Some([[0.;4];4]), source: None }];
        m.map_detail = true;
        assert_eq!(m.page(), vec![50, 51, 910, 1_000_000]);
    }
    #[test]
    fn culling_can_toggle_with_msaa_and_render_scale_changes() {
        let mut app = App::new();
        let mut images = Assets::<Image>::default();
        let target = images.add(Image::new_target_texture(
            1280,
            800,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        app.insert_resource(SceneTarget(target.clone()))
            .insert_resource(images)
            .insert_resource(Menu {
                open: false, selected: 0,
                maps: vec![crate::map_library::Entry { label: "Test world".into(), path: None, world: None }],
                ..menu()
            })
            .add_systems(Update, apply);
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.hour = 23.5;
            menu.settings.day_speed = 60;
            assert!((menu.advance_day(60.) - 0.5).abs() < 0.0001);
            menu.open = true;
            assert_eq!(menu.advance_day(60.), 0.5);
            menu.open = false;
            menu.settings.day_speed = 0;
            assert_eq!(menu.advance_day(60.), 0.5);
        }
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let camera = app.world_mut().spawn((Camera3d::default(), Msaa::Off)).id();
        app.update();
        // MSAA and occlusion culling are no longer settings, so the only thing
        // `apply` still adapts is the internal render target size.
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.scale = 67;
        }
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&target)
                .unwrap()
                .size(),
            UVec2::new(857, 536)
        );
    }
    #[test]
    fn sections_expose_only_real_rows_and_all_maps() {
        let mut menu = menu();
        for section in 0..SECTIONS.len() {
            menu.select_section(section);
            let rows = menu.rows();
            assert!(rows.contains(&menu.selected));
            assert!(rows.windows(2).all(|pair| pair[0] < pair[1]));
            // Audio, controller identity, and frame-time rows are spawned.
            assert!(rows.iter().all(|id| *id < 20 || *id == FRAME_STATS_ROW || *id == FRAME_STATS_CORNER_ROW || *id == SESSION_TIMER_ROW || *id == NPC_DRAW_DISTANCE_ROW || *id == MORE_AUDIBLE_ROW || *id == SPEECH_ROW || *id == BETTER_WATER_ROW || *id == EXTENDED_REPLAYS_ROW || *id == REPLAY_LENGTH_ROW || *id == BETTER_MOVER_ROW || *id == DEBUG_MENUS_ROW || *id == FLY_CAM_ROW || *id == PHYSICS_THREADS_ROW || OPT_IN_ROWS.contains(id) || *id >= 1000));
        }
        menu.select_section(1);
        assert_eq!(menu.rows(),vec![3,CAMERA_ANGLE_ROW,8,10]);
        // The opt-ins have their own section, not Graphics.
        menu.select_section(2);
        assert!(!menu.rows().iter().any(|id| *id == MORE_AUDIBLE_ROW || *id == SPEECH_ROW || OPT_IN_ROWS.contains(id)));
        menu.select_section(5);
        assert_eq!(menu.category_name(5), "Opt-Ins");
        assert_eq!(menu.rows(), [NPC_DRAW_DISTANCE_ROW, BETTER_WATER_ROW, MORE_AUDIBLE_ROW, SPEECH_ROW, EXTENDED_REPLAYS_ROW, REPLAY_LENGTH_ROW,
            BETTER_MOVER_ROW, DEBUG_MENUS_ROW, FLY_CAM_ROW, PHYSICS_THREADS_ROW].into_iter().chain(OPT_IN_ROWS).collect::<Vec<_>>());
        assert_eq!(menu.page()[0], 911);
        // Every opt-in is in the Opt-Ins section and nowhere else.
        menu.select_section(4);
        assert!(!menu.rows().iter().any(|id| [BETTER_WATER_ROW, EXTENDED_REPLAYS_ROW, BETTER_MOVER_ROW, DEBUG_MENUS_ROW, FLY_CAM_ROW].contains(id)));
        menu.select_section(5);
        menu.custom_sections=vec![("Challenges".into(),vec![("test.mod".into(),"race".into(),"Race".into())])];
        menu.select_section(SECTIONS.len());
        assert_eq!(menu.rows(),vec![200]);
        assert_eq!(menu.category_name(SECTIONS.len()), "Challenges");
        menu.select_section(0);
        assert_eq!(menu.rows(), (1000..1040).collect::<Vec<_>>());
        menu.maps[0].path = Some(PathBuf::from("University.skate"));
        menu.destinations = vec![
            crate::teleport_menu::Destination { id: "uni".into(), name: "Campus".into(), map: "University".into(), world: None, label_key: None, matrix: Some([[0.;4];4]), source: None },
            crate::teleport_menu::Destination { id: "dt".into(), name: "Downtown".into(), map: "DownTown".into(), world: None, label_key: None, matrix: Some([[0.;4];4]), source: None },
        ];
        menu.map_detail = true;
        assert_eq!(menu.rows(), vec![50,51,1_000_000]);
        menu.select_section(0);
        menu.maps.clear();
        menu.select_section(0);
        assert_eq!(menu.selected, 100);
        assert!(menu.rows().is_empty());
        menu.select_section(3);
        assert!(menu.multiplayer);
        assert_eq!(menu.rows(), vec![2,6,13,14,15]);
        menu.network_page = 4;
        assert_eq!(menu.rows(), (20..27).chain([10]).collect::<Vec<_>>());
        menu.network_page = 1;
        assert_eq!(menu.rows(), vec![3,4,10]);
        menu.network_page = 3;
        assert_eq!(menu.rows(), vec![0,1,10]);
        menu.browser = true;
        assert!(menu.rows().contains(&8));
        assert!(!SECTIONS.iter().any(|(name,_,_)| matches!(*name,"SESSION"|"WORLD")));
        menu.select_section(2);
        assert!(!menu.multiplayer && !menu.browser);
        assert_eq!(menu.rows(), vec![0,1,2,13,16,17,18,FRAME_STATS_ROW,FRAME_STATS_CORNER_ROW,SESSION_TIMER_ROW]);
        menu.daylight = true;
        assert_eq!(menu.rows(), vec![0,1,2,3]);
    }
    /// Physics Step Threads: single by default and for settings files written before the row; the saved choice is read
    /// back at start-up.
    /// USER-ASKS 338: NPC Skater Physics is on by default; a file from before the change gets it once, a later "off"
    /// stays off.
    #[test]
    fn npc_physics_defaults_on_once_and_off_sticks() {
        assert!(GraphicsSettings::default().opt_ins.npc_physics);
        let mut old: GraphicsSettings = serde_json::from_str(r#"{"opt_ins":{"zombie":false,"npc_physics":false,"visualize_grabs":false}}"#).unwrap();
        assert!(old.migrated() && old.opt_ins.npc_physics && old.defaults_version == DEFAULTS_VERSION);
        old.opt_ins.npc_physics = false;
        let again: GraphicsSettings = serde_json::from_slice(&serde_json::to_vec(&old).unwrap()).unwrap();
        let mut again = again;
        assert!(!again.migrated() && !again.opt_ins.npc_physics);
    }
    #[test]
    fn physics_threads_default_single_and_load_saved() {
        let old: GraphicsSettings = serde_json::from_str(r#"{"width":1920,"height":1080}"#).unwrap();
        assert!(!old.physics_threads_multi);
        let dir = std::env::temp_dir().join(format!("physics-threads-{}", std::process::id()));
        let assets = dir.join("assets");
        std::fs::create_dir_all(dir.join("settings")).unwrap();
        assert!(!physics_threads_multi(&assets) || std::env::var_os("SKATE_FIXED_EXECUTOR").is_some());
        std::fs::write(dir.join("settings/graphics.json"), r#"{"physics_threads_multi":true}"#).unwrap();
        if std::env::var_os("SKATE_FIXED_EXECUTOR").is_none() {
            assert!(physics_threads_multi(&assets));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn frame_counter_setting_defaults_off_and_round_trips() {
        assert!(!GraphicsSettings::default().frame_stats);
        // Settings files written before the row existed keep working.
        let old: GraphicsSettings = serde_json::from_str(r#"{"width":1920,"height":1080,"scale":100,"fps":0}"#).unwrap();
        assert!(!old.validated().frame_stats);
        let on = GraphicsSettings { frame_stats: true, ..default() };
        let saved: GraphicsSettings = serde_json::from_slice(&serde_json::to_vec(&on).unwrap()).unwrap();
        assert!(saved.validated().frame_stats);
        // A counter saved as on before Simple existed opens as Verbose.
        let old_on: GraphicsSettings = serde_json::from_str(r#"{"frame_stats":true}"#).unwrap();
        assert!(old_on.frame_stats && !old_on.frame_stats_simple);
        let simple = GraphicsSettings { frame_stats: true, frame_stats_simple: true, ..default() };
        let saved: GraphicsSettings = serde_json::from_slice(&serde_json::to_vec(&simple).unwrap()).unwrap();
        assert!(saved.frame_stats && saved.frame_stats_simple);
    }
    #[test]
    fn npc_draw_distance_defaults_to_retail_and_keeps_menu_steps_only() {
        assert_eq!(GraphicsSettings::default().npc_draw_distance, skate_core::living_world::DrawDistance::RETAIL);
        let odd: GraphicsSettings = serde_json::from_str(r#"{"npc_draw_distance":1.25}"#).unwrap();
        assert_eq!(odd.validated().npc_draw_distance, skate_core::living_world::DrawDistance::RETAIL);
        let none: GraphicsSettings = serde_json::from_str(r#"{"npc_draw_distance":0.0}"#).unwrap();
        assert_eq!(none.validated().npc_draw_distance, skate_core::living_world::DrawDistance::NONE);
        assert_eq!(draw_distance_label(1.0), "Retail");
        assert_eq!(draw_distance_label(2.0), "2x  (not retail)");
        assert_eq!(cycle(&skate_core::living_world::DrawDistance::MENU_STEPS, 3.0, 1), skate_core::living_world::DrawDistance::NONE);
    }
    #[test]
    fn invalid_saved_values_fall_back() {
        let settings: GraphicsSettings =
            serde_json::from_str(r#"{"width":0,"height":999999,"scale":0,"fps":1}"#)
                .unwrap();
        assert_eq!(settings.validated(), GraphicsSettings { defaults_version: 0, ..GraphicsSettings::default() });
    }
    #[test]
    fn scaled_target_and_cycle_boundaries() {
        let s = GraphicsSettings {
            scale: 50,
            ..default()
        };
        assert_eq!(
            s.internal_size(UVec2::new(1920, 1080)),
            UVec2::new(960, 540)
        );
        assert_eq!(s.internal_size(UVec2::ZERO), UVec2::ONE);
        assert_eq!(cycle(LIMITS, 0, -1), 240);
        assert_eq!(cycle(LIMITS, 240, 1), 0);
    }
}
