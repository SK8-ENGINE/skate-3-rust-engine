//! Dev view of the retail pause menu on the front-end stage (on unless `SKATE3_RETAIL_MENUS=0`).
//! Controls, input and env switches: docs/hails-additions/31-retail-menus.md#retail-menu-stage-dev-view
use crate::{
    apt_stage::Stage,
    apt_vm::Value,
    crossbar_host::FrontEnd,
    retail_menu_movies::{MenuImage, RetailMenuMovies},
    retail_menus::RetailMenus,
    scoring_hud::{render_draws, HudMaterial, Slot},
    ui_audio::FrontendSound,
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use skate_core::crossbar::{CrossbarAction, CrossbarRules, CrossbarState, NavEvent};
use skate_core::fe_input::{fired, parse_apt_keys, AptKey, FeInput, RepeatRule};
use skate_core::fe_popup::PopupKind;
use skate_core::fe_stack::{Effect, FeStack, Layer, Request};
use skate_core::menu_values::Direction;
use skate_core::menus::{EntryRef, MenuAction, Outcome, Owner};
use std::collections::BTreeMap;

const CROSSBAR: &str = "CrossBar";
/// Key index of AptStart (Apt key table record 0).
const APT_START: u32 = 0;
/// AptPrevious (B), AptLeft, AptRight key indices.
const APT_PREVIOUS: u32 = 3;
const APT_LEFT: u32 = 14;
const APT_RIGHT: u32 = 15;
/// The pause menu template's path under `source/screens/` (template table 0x8302F1A8, record 69).
const CROSSBAR_PATH: &str = "main/core_menu.swf";
/// The Game Settings template (record 90) and its path; FE screen 45's init opens it (sub_8260BF58).
const OPTIONS: &str = "Options";
const OPTIONS_PATH: &str = "options/options.swf";
/// The Trick Guide template (record 29) and its path; FE screen 14's init opens it (0x82618A40).
const TRICK_GUIDE: &str = "TrickTutorial";
const TRICK_GUIDE_PATH: &str = "tricks/trickguide.swf";
/// Instant replay screen and help (FE 12 / FE 13, 826102A8; b391, b410 / b411): docs/hails-additions/31-retail-menus.md#replay-screen-choice
struct ReplayScreen {
    template: &'static str,
    path: &'static str,
    movie: &'static str,
    help: &'static str,
    help_path: &'static str,
    help_movie: &'static str,
    channels: bool,
}
const REPLAY_SCREENS: [ReplayScreen; 2] = [
    ReplayScreen {
        template: "Replay3",
        path: "replay2/replay_editor3.swf",
        movie: "source/screens/replay2/replay_editor3",
        help: "ReplayHelp2",
        help_path: "replay2/replay_help2.swf",
        help_movie: "source/screens/replay2/replay_help2",
        channels: true,
    },
    ReplayScreen {
        template: "Replay2",
        path: "replay2/replay_editor2.swf",
        movie: "source/screens/replay2/replay_editor2",
        help: "ReplayHelp",
        help_path: "replay2/replay_help.swf",
        help_movie: "source/screens/replay2/replay_help",
        channels: false,
    },
];
/// The title update's DLC templates (TU3 71 / 74): tables taken from the disc's own executable predate them, so they
/// are registered after the table's (their ids only need to be unique).
const DLC_TEMPLATES: [&str; 2] = ["Replay3", "ReplayHelp2"];
/// The Challenge Map template (record 68) and its path; FE screen 17 (b244 / b253). The pause item that opens it.
const CHALLENGE_MAP: &str = "ChallengeMap";
const CHALLENGE_MAP_PATH: &str = "map/challengemap.swf";
const CHALLENGE_MAP_ITEM: &str = "ChallengeMap";
/// Its tab keys (handler 82646B28, b256): AptRShift2 / AptDRight / AptLStickRight next, AptLShift2 / AptDLeft /
/// AptLStickLeft previous (clamped, no wrap).
const MAP_NEXT_TAB: [u32; 3] = [7, 19, 27];
const MAP_PREVIOUS_TAB: [u32; 3] = [10, 18, 26];
/// The Skate Profile template (record 106) and its path; FE screen 61's init opens it (0x8264C530, b308). The Free
/// Play pause item that opens it.
const OFFLINE_PROFILE: &str = "OfflineProfile";
const OFFLINE_PROFILE_PATH: &str = "profile/offline_skateProfile.swf";
const OFFLINE_PROFILE_ITEM: &str = "SkateProfileOffline";
/// Its keys (handler 0x8264C608): AptRShift2 next tab, AptLShift2 previous tab, Up / Down / LStickUp / LStickDown
/// scroll (left to the movie).
const PROFILE_NEXT_TAB: u32 = 7;
const PROFILE_PREVIOUS_TAB: u32 = 10;
const PROFILE_SCROLL: [u32; 4] = [12, 13, 24, 25];
/// The Extras template (record 15) and its path; FE screen 18's init opens it (0x82655478, b320). The pause item.
const EXTRAS: &str = "Extras";
const EXTRAS_PATH: &str = "main/extras.swf";
const EXTRAS_ITEM: &str = "Extras";
/// The SpawnSkater template (record 107) and its path; FE screen 62's init opens it (0x8264D958, b543). The Free Play
/// and Call Skater pause items.
const SPAWN_SKATER: &str = "SpawnSkater";
const SPAWN_SKATER_PATH: &str = "cas/spawnskater.swf";
/// The Music Player (template 31, FE 26; init 0x8260B400): the Options tab's MusicPlayer row (sub_8261AF30(26, 0)).
const MUSIC_PLAYER: &str = "MusicPlayer";
const MUSIC_PLAYER_PATH: &str = "musicplayer/musicplayertest.swf";
const MUSIC_PLAYER_MOVIE: &str = "source/screens/musicplayer/musicplayertest";
const MUSIC_PLAYER_ITEM: &str = "MusicPlayer";
/// The loading screen (template 131 TipsLoader, FE 52; research b581). Optional in the set.
const TIPS_LOADER: &str = "TipsLoader";
const TIPS_LOADER_PATH: &str = "main/tiploader.swf";
const TIPS_LOADER_MOVIE: &str = "source/screens/main/tiploader";
const SPAWN_SKATER_MOVIE: &str = "source/screens/cas/spawnskater";
/// Its limit popup's callback (the choice changes nothing).
const SPAWN_SKATER_POPUP: &str = "spawn_skater.limit";
/// The Free Play options template (record 13) and its path; FE 34 (X in FE 62 Free Play, flow 0x8A; b548 / b564).
const FREE_SKATE: &str = "FreeSkateOption";
const FREE_SKATE_PATH: &str = "main/freeskate_options.swf";
/// AptMisc (X) and AptOption (Y) key indices.
const APT_MISC: u32 = 5;
const APT_OPTION: u32 = 4;
/// The on-screen keyboard's directions: pad / D-pad / left stick (ours; FE 46 had the system keyboard).
const KEYS_UP: [u32; 3] = [12, 16, 24];
const KEYS_DOWN: [u32; 3] = [13, 17, 25];
const KEYS_LEFT: [u32; 3] = [14, 18, 26];
const KEYS_RIGHT: [u32; 3] = [15, 19, 27];
/// The text viewer template (record 28) and its path; FE screen 3's init opens it (0x8263A190, b322).
const TEXT_VIEWER: &str = "ToSScreen";
const TEXT_VIEWER_PATH: &str = "nucleus/nucleus_tos.swf";
/// The credits template (record 30) and its path; FE screen 25's init opens it (0x82608C00, b489). The lines file
/// sits in the menu set (setup copies `data\credits\credits.txt` out of miscload.big).
const CREDITS: &str = "Credits";
const CREDITS_PATH: &str = "extras/credits.swf";
const CREDITS_MOVIE: &str = "source/screens/extras/credits";
const CREDITS_FILE: &str = "credits.txt";
/// The cheat dialog's popup callback (its choice changes nothing).
const CHEAT_POPUP: &str = "cheat_code.dialog";
/// The Video Calibration template (record 91) and its path; FE screen 51's init opens it (0x8260E868).
const VIDEO_CALIBRATION: &str = "VideoCalibration";
const VIDEO_CALIBRATION_PATH: &str = "options/videocal.swf";
/// The callback id of its "finished" popup (0x8260EA48).
const VIDEO_CALIBRATION_POPUP: &str = "video_calibration.finished";
/// The settings row that pushes it (sub_8260D290 row 25).
const VIDEO_CALIBRATION_ROW: &str = "ID_GAMESETTINGS_VIDEO_CALIBRATION";
/// The pause menu item that opens it (crossbar case 0x8261BF78).
const TRICK_GUIDE_ITEM: &str = "TrickGuide";
/// AptNext (A) key index.
const APT_NEXT: u32 = 2;
/// The reward presentation screens (FE 64 / 65 / 66; research b899 §1): movie key, path, template, in
/// `presentation::Screen` order.
const REWARD_SCREENS: [(&str, &str, &str); 3] = [
    ("source/screens/unlock/unlockoverlay", "unlock/unlockoverlay.swf", "RewardSummary"),
    ("source/screens/unlock/unlock_milestone", "unlock/unlock_milestone.swf", "MilestoneUnlock"),
    ("source/screens/unlock/unlock_teammate", "unlock/unlock_teammate.swf", "TeammateUnlock"),
];
/// The end dialog movies (FE 4, init 825F0CE8; research b920 / b925), in `end_dialog::Movie` order: options table,
/// small, failed.
/// Paths match the exported keys' case: the stage's movie lookup is case sensitive (laptop 2026-11-07: with
/// `hudComponents/` the load failed and the dialog never constructed).
const END_DIALOGS: [(&str, &str, &str); 3] = [
    ("source/screens/hudcomponents/endchallengeoptions", "hudcomponents/endchallengeoptions.swf", "EndChallengeOptions"),
    ("source/screens/hudcomponents/endchallenge_small", "hudcomponents/endchallenge_small.swf", "EndChallengeOptions_Small"),
    ("source/screens/hudcomponents/challengefailed_small", "hudcomponents/challengefailed_small.swf", "EndChallengeOptionsFail_Small"),
];
/// The sign-up poster (FE 0x1B page 0, template 23; research b932, b935): movie key, path, template.
const POSTER: (&str, &str, &str) = ("source/screens/challenge/challenge_signup", "challenge/challenge_signup.swf", "SignUp_Poster");
/// `SKATE_RETAIL_END_SCREENS=1`: the end dialog and reward screens open on the stage. Off by default until they are
/// seen drawing (laptop run 2026-11-07: logged as shown, nothing drawn); off = the old continue / accept at once.
pub(crate) fn screens_switch() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_RETAIL_END_SCREENS").is_ok_and(|v| v == "1"))
}

/// Stage updates after which an opened end dialog / reward screen is shown even without its load or intro call.
const SHOW_FALLBACK_FRAMES: u32 = 30;
/// Pad keys to end dialog slots (825F12E8): Apt key indices 2 (A) / 3 (B) / 5 (X) / 4 (Y) to slots 0 / 1 / 2 / 3.
const END_DIALOG_KEYS: [(u32, usize); 4] = [(2, 0), (3, 1), (5, 2), (4, 3)];
/// The fader bit screen changes hold (sub_825E6B30 / sub_825E6BF8 bit 0).
const FADE_SCREEN: u32 = 0;
/// The fader bit the player skater swap holds (0x89 state 82706938: 825E6B30 bit 7, b562).
const FADE_SWAP: u32 = 7;
/// The fader bit the game state's loading holds (sub_826E08A8 bit 2).
const FADE_LOAD: u32 = 2;
/// Trick Guide state 4 (0x82619A20): updates before it releases the fade and takes input (S+336 > 10).
const TRICK_GUIDE_FADE_IN_TICKS: u32 = 11;

/// What runs when the screen is black (the fade hold's callback).
#[derive(Clone, Debug, PartialEq, Eq)]
enum FadeAction {
    /// Crossbar TrickGuide callback 0x8261CDD8: push the screen.
    Push(usize),
    /// Screen 14 back callback 0x82618EF0: close it.
    Close(usize),
    /// FE 62 flow 0x89 callback 82706AE8: unload and load the player skater (b562).
    SetPlayer(skate_core::spawn_skater::Skater),
}
const CROSSBAR_ENUMS: [&str; 6] =
    ["RefreshMenu", "RefreshOption", "InitialiseMenu", "InitialiseSubOption", "InitSubOptionOnlineTeam", "RefreshDLCBanner"];
/// Above the scoring HUD's draws on the shared layer.
const MENU_Z: f32 = 50.0;

#[derive(Resource)]
struct MenuStage {
    stage: Stage,
    /// The autosave layer's save tag while a save write is in flight (`save_tag`, b948).
    save_tag: crate::save_tag::SaveTag,
    crossbar: CrossbarState,
    rules: CrossbarRules,
    language: BTreeMap<String, String>,
    /// The FE screen stack (skate_core::fe_stack); the pause crossbar is the only screen with a movie so far.
    stack: FeStack,
    textures: BTreeMap<String, Handle<Image>>,
    white: Handle<Image>,
    slots: Vec<Slot>,
    failed: bool,
    /// A stopped stage has given gameplay back and is rebuilt next (`restart_failed_stage`).
    restart_due: bool,
    /// `SKATE3_RETAIL_MENUS_OPEN=1`: open the pause menu once the world has run 4 s (screenshot checks).
    auto_open: bool,
    /// The pause menu's AS screen id (600 + template index; retail 669).
    crossbar_id: i32,
    /// The Game Settings screen's AS id (retail 690) and state; its titles from the menu tables.
    options_id: i32,
    /// The chyron's visibility as last set (vt+48 acts on changes only).
    chyron: bool,
    settings: crate::settings_host::SettingsState,
    settings_titles: Vec<String>,
    /// The Trick Guide screen's AS id (retail 629) and state (empty guide = not available).
    trick_guide_id: i32,
    trick_guide: crate::trick_guide_host::TrickGuideHost,
    /// The instant replay screen's AS id (None: no Replay2 template or movie in this install: our Bevy panel shows),
    /// its host and whether it is shown (after its IntroComplete).
    replay_id: Option<i32>,
    replay_help_id: Option<i32>,
    /// The replay screen in use (`REPLAY_SCREENS`).
    replay_screen: usize,
    replay_menu: crate::replay_menu_host::ReplayMenuHost,
    replay_shown: bool,
    /// The Challenge Map screen's AS id and host (challenge_map_host).
    challenge_map_id: i32,
    challenge_map: crate::challenge_map_host::ChallengeMapHost,
    /// The Skate Profile screen's AS id and host (offline_profile_host), and the mod generation its data was built for.
    offline_profile_id: i32,
    offline_profile: crate::offline_profile_host::OfflineProfileHost,
    /// The reward summary host and the RewardScreens generation its view was copied at.
    reward_summary: crate::reward_summary_host::RewardSummaryHost,
    reward_generation: u64,
    /// Each reward screen's AS id when its movie was exported and its template registered.
    reward_ids: [Option<i32>; 3],
    /// The reward screen open on the stage (screen index, card index) and whether its intro completed.
    reward_open: Option<(usize, u32)>,
    reward_shown: bool,
    /// The end dialog movies' AS ids, and the one open with whether its intro completed.
    end_ids: [Option<i32>; 3],
    end_open: Option<usize>,
    end_shown: bool,
    /// Updates since the end dialog / reward screen opened (the show fallback for movies that report no intro).
    end_frames: u32,
    reward_frames: u32,
    /// The sign-up poster's AS id, whether it is open / shown, and updates since it opened.
    poster_id: Option<i32>,
    poster_open: bool,
    poster_shown: bool,
    poster_frames: u32,
    profile_generation: (u64, u64),
    /// The Extras screen's AS id and host (extras_host), the cheat table, and the text entry FE 46 opens next.
    extras_id: i32,
    extras: crate::extras_host::ExtrasHost,
    cheats: skate_core::cheats::CheatRules,
    pending_entry: Option<skate_core::text_entry::TextEntryRequest>,
    /// The text viewer FE 3's AS id and its `tos` host.
    viewer_id: i32,
    tos: crate::extras_host::TosHost,
    /// The credits FE 25's AS id and its `FECredits` host.
    credits_id: i32,
    credits: crate::extras_host::CreditsHost,
    /// FE 62's AS id, its `SkaterSpawner` host and the mode the next push opens it in (the pause item picks it).
    spawn_skater_id: i32,
    spawn_skater: crate::spawn_skater_host::SpawnSkaterHost,
    spawn_skater_mode: skate_core::spawn_skater::Mode,
    /// FE 26's AS id and its `MusicPlayer` host.
    music_player_id: i32,
    music_player: crate::music_player_host::MusicPlayerHost,
    /// Flow 0x89: the skater to load now that the screen is black, then whether its load is still running.
    swap_due: Option<skate_core::spawn_skater::Skater>,
    swap_loading: bool,
    /// FE 52's AS id, its `TipsLoader` host, the load sequence and whether the screen's intro is done / it closed.
    tips_loader_id: i32,
    tips_loader: crate::tips_loader_host::TipsLoaderHost,
    /// The retail lines, before mods' `sdk.loading` lines join them (set on the first open).
    retail_lines: Option<skate_data::tips_loader::TipsLines>,
    /// Seconds the loading screen has stayed up past the load's own work (mods' holds, capped).
    load_after: f32,
    load: skate_core::tips_loader::LoadScreen,
    load_up: bool,
    load_gone: bool,
    /// The map the open loading screen loads (mods' loading_opened / loading_closed events).
    load_map: Option<String>,
    /// FE 34's AS id and its `FreeSkateOption` host.
    free_skate_id: i32,
    free_skate: crate::free_skate_options_host::FreeSkateOptionsHost,
    /// The image slots the movies draw named images through (image_renderer_host).
    image_slots: crate::image_renderer_host::ImageSlots,
    /// The retail guide (mod layers apply over it) and the mod generation it was built for.
    trick_guide_base: std::sync::Arc<skate_core::trick_guide::TrickGuide>,
    trick_guide_generation: u64,
    /// The FE fader (fade to black around screen changes) and the Trick Guide's ticks left before its release
    /// (None = released / not waiting).
    fade: skate_core::fe_fade::FeFade<FadeAction>,
    fade_params: skate_core::fe_fade::FeFadeParams,
    fade_carry: f32,
    trick_guide_fade_in: Option<u32>,
    /// The Video Calibration screen's AS id (retail 691), its page counter and rules (skate_core::video_calibration).
    video_cal_id: i32,
    video_cal: skate_core::video_calibration::VideoCalibration,
    video_cal_rules: skate_core::video_calibration::CalibrationRules,
    /// The popup list (skate_core::fe_popup), the host's state (popup_host) and the popup movies' AS ids by kind
    /// (preloaded on the "popup" layer; None when the set lacks the movie). `popup_dirty`: show the last entry again
    /// (FE vt+24 runs on every push and after a removal).
    popups: skate_core::fe_popup::PopupList,
    popup: crate::popup_host::PopupHost,
    popup_ids: Vec<(skate_core::fe_popup::PopupKind, i32)>,
    popup_dirty: bool,
    /// The action overlay's AS id (None when the set lacks the movie) and the title it shows now.
    signup_id: Option<i32>,
    signup_shown: Option<(i32, String)>,
    /// Retail Apt keys in key-table order (input.cfg expressions).
    apt_keys: Vec<AptKey>,
    pads: [FeInput; 4],
    repeat: RepeatRule,
    /// `SKATE3_RETAIL_MENUS_KEYS`: key indices to inject, one per frame, while the pause menu, the end dialog or a reward
    /// screen takes input; `_` is a frame without a key (lets a screen load before the next key, e.g.
    /// `15,15,2,_x60,13`: `_xN` = N frames).
    injected: std::collections::VecDeque<Option<u32>>,
}

/// The retail menus are wanted: on unless `SKATE3_RETAIL_MENUS=0`.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE3_RETAIL_MENUS").as_deref() != Ok("0"))
}

/// The retail pause menu can open this frame: the stage loaded and the menu tables carry the FE screen
/// records. False on installs set up before those existed (or with the menus off): Start then opens the PC
/// overlay, the fallback, until setup runs again.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RetailPause(pub bool);

/// The stage's per-frame systems; prompts that write `PopupMods` run before it.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MenuStageSet;

/// The fe sound the sign-up prompt plays when it appears (any prompt kind: the component is shared).
pub(crate) const SIGNUP_SHOW_SOUND: &str = "object_r1_options";

pub(crate) struct RetailMenuStagePlugin;
impl Plugin for RetailMenuStagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RetailPause>()
            .init_resource::<crate::trick_guide_host::TrickGuideMods>()
            .init_resource::<crate::offline_profile_host::ProfileMods>()
            .init_resource::<crate::extras_host::CreditsMods>()
            .init_resource::<crate::tips_loader_host::LoadingMods>()
            .init_resource::<crate::popup_host::PopupMods>()
            .add_message::<crate::extras_host::CheatChanged>()
            .add_message::<crate::extras_host::UnlockGranted>()
        .add_message::<crate::spawn_skater_host::SpawnSkaterRequest>()
            .add_plugins(crate::text_entry_ui::TextEntryUiPlugin);
        if !enabled() {
            return;
        }
        app.add_systems(PostStartup, setup).add_systems(Update, (update, restart_failed_stage, render).chain().in_set(MenuStageSet));
    }
}

/// Drop an open-then-close pair in one stack batch (NOT RETAIL mechanism, same outcome): docs/hails-additions/31-retail-menus.md#reopen-and-close-pair
fn drop_reopen_and_close(effects: Vec<Effect>) -> Vec<Effect> {
    let mut keep = vec![true; effects.len()];
    for (i, effect) in effects.iter().enumerate() {
        if let Effect::Open(screen) = *effect {
            if let Some(j) = effects[i + 1..].iter().position(|e| *e == Effect::Close(screen)) {
                keep[i] = false;
                keep[i + 1 + j] = false;
            }
        }
    }
    effects.into_iter().zip(keep).filter_map(|(e, k)| k.then_some(e)).collect()
}

#[cfg(test)]
mod reopen_tests {
    use super::*;

    #[test]
    fn a_screen_reopened_and_closed_in_one_batch_is_neither() {
        let fx = vec![
            Effect::Close(7),
            Effect::SuspendGameplay(true),
            Effect::Open(3),
            Effect::Close(3),
            Effect::SuspendGameplay(false),
        ];
        assert_eq!(drop_reopen_and_close(fx), [Effect::Close(7), Effect::SuspendGameplay(true), Effect::SuspendGameplay(false)]);
        // A close before the open (back to the screen) stays.
        assert_eq!(drop_reopen_and_close(vec![Effect::Close(3), Effect::Open(3)]), [Effect::Close(3), Effect::Open(3)]);
    }
}

/// Stage rebuilds a session may do after a stage error; past this Start keeps opening the PC overlay.
const MAX_STAGE_RESTARTS: u32 = 3;

/// A stopped stage is rebuilt with the same boot as at startup, once `update` has given gameplay back: dropping the
/// old stage frees every object its VM held (any movie, mods' included), so a leak or a runaway script costs one
/// silent rebuild instead of the retail menus for the rest of the session (laptop session 2026-10-18).
fn restart_failed_stage(mut commands: Commands, stage: Option<Res<MenuStage>>, mut restarts: Local<u32>) {
    if !stage.is_some_and(|s| s.restart_due) || *restarts >= MAX_STAGE_RESTARTS {
        return;
    }
    *restarts += 1;
    warn!("RETAIL_MENUS stage restarted ({}/{MAX_STAGE_RESTARTS})", *restarts);
    commands.run_system_cached(setup);
}

/// Retail ScreenManagerReady handler (825D9450) registrations, in order (partial: NOT RETAIL YET): docs/hails-additions/31-retail-menus.md#screenmanagerready-registrations
fn boot_registration(stage: &mut Stage, fe: &mut FrontEnd, assets: &std::path::Path) -> Result<Vec<String>, String> {
    let text = |s: &str| Value::Text(s.into());
    let tables = std::fs::read_to_string(assets.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
    // All templates in table order (the AS hands out ids from 600); tables extracted before templates were
    // added register CrossBar alone.
    let names: Vec<String> = match tables.as_ref().map(|t| &t.templates).filter(|t| !t.is_empty()) {
        Some(templates) => templates.iter().map(|t| t.name.clone()).collect(),
        None => vec![CROSSBAR.into()],
    };
    let mut names = names;
    for dlc in DLC_TEMPLATES {
        if !names.iter().any(|n| n == dlc) {
            names.push(dlc.into());
        }
    }
    for name in &names {
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![text(name)], fe)?;
    }
    // The action overlay's states (research b426): no movie defines the names and they are not strings in the image;
    // the values are what the C++ component sends (82665E88: Teleport = 2), the order that of the movie's pool.
    for (value, name) in ["Off", "Challenge", "Teleport", "PressStart", "DMOGrab", "OTS", "Disabled"].iter().enumerate() {
        let global = stage.vm.global;
        stage.vm.set(global, format!("kActionOverlayState_{name}"), Value::Number(value as f64))?;
    }
    for (i, key) in tables.iter().flat_map(|t| &t.keys).enumerate() {
        stage.call_path("_level0._root.RegisterAptInputEnum", vec![text(&key.name), text(&i.to_string())], fe)?;
    }
    let manifest: serde_json::Value = std::fs::read_to_string(assets.join("private/audio/audio_manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut sounds: Vec<&str> = manifest["frontend"]["sounds"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|s| s["name"].as_str())
        .collect();
    sounds.sort_unstable();
    for name in sounds {
        stage.call_path(
            "_level0._root.RegisterAudioEnum",
            vec![text(&format!("FE_SOUND_{}", name.to_ascii_uppercase())), text(name)],
            fe,
        )?;
    }
    stage.call_path("_level0._root.SetPlatform", vec![text("0")], fe)?;
    stage.call_path("_level0._root.SetScreenSize", vec![text("1280"), text("720")], fe)?;
    // No SetScreenEdgeOffset: retail sends its SD value "136" only without the HD flag (doc 31 "Screen edge offset").
    stage.call_path("_level0._root.UpdatePALMode", vec![text("false")], fe)?;
    Ok(names)
}

/// The autosave layer's corner wheel while the loading screen is up, as in the recomp's loads (doc 31 "Loading screen
/// (FE 52)"). A menu set without the autosave layer has no such function; the load runs without the wheel.
fn load_tag(stage: &mut Stage, on: bool, fe: &mut FrontEnd) {
    let (path, args) = if on { ("_global.Load_EnableTag", vec![Value::Bool(true)]) } else { ("_global.Load_HideTag", Vec::new()) };
    if let Err(e) = stage.call_path(path, args, fe) {
        debug!("RETAIL_MENUS {path}: {e}");
    }
}

/// The autosave layer's save tag (`825DF4F8`, b948): `Save_EnableTag("false", "false")` in game (FE not open, no
/// variant; the args' meaning is open), `Save_HideTag()`. A menu set without the layer has neither; saving goes on.
fn save_tag(stage: &mut Stage, on: bool, fe: &mut FrontEnd) {
    let (path, args) = if on { ("_global.Save_EnableTag", vec![Value::Text("false".into()), Value::Text("false".into())]) } else { ("_global.Save_HideTag", Vec::new()) };
    match stage.call_path(path, args, fe) {
        Ok(_) => info!("RETAIL_MENUS {path}"),
        Err(e) => debug!("RETAIL_MENUS {path}: {e}"),
    }
}

/// The Apt keys: names from the menu tables' key table, expressions from the install's stock input.cfg.
fn apt_keys(assets: &std::path::Path) -> Vec<AptKey> {
    let names: Vec<String> = std::fs::read_to_string(assets.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
        .map(|t| t.keys.into_iter().map(|k| k.name).collect())
        .unwrap_or_default();
    let cfg = std::fs::read_to_string(assets.join(skate_data::input_config::STOCK_INPUT_PATH)).unwrap_or_default();
    parse_apt_keys(&cfg, &names).unwrap_or_else(|e| {
        warn!("RETAIL_MENUS input.cfg: {e}");
        Vec::new()
    })
}

/// The keys fired this frame per controller (FE poll sub_825DFDC8: every connected controller, keys in table
/// order). The keyboard drives controller 0's slots.
fn poll_keys(ms: &mut MenuStage, keys: &ButtonInput<KeyCode>, seconds: f32) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    for controller in 0..4 {
        let mut values = crate::input::platform::poll(controller)
            .map(|p| skate_core::input::xbox::convert(&p.state, u8::from(p.subtype == 7)))
            .unwrap_or([0.0; 24]);
        if controller == 0 {
            for (key, slot) in [
                (KeyCode::ArrowUp, 0),
                (KeyCode::ArrowDown, 1),
                (KeyCode::ArrowLeft, 2),
                (KeyCode::ArrowRight, 3),
                (KeyCode::F8, 4),
                (KeyCode::Enter, 12),
                (KeyCode::Backspace, 13),
            ] {
                if keys.pressed(key) {
                    values[slot] = 1.0;
                }
            }
        }
        let edges = ms.pads[controller].update(&values, seconds, &ms.repeat);
        out.extend(fired(&ms.apt_keys, &edges).into_iter().map(|k| (k, controller)));
    }
    out
}

fn front_end<'a>(
    menus: &'a RetailMenus,
    language: &'a BTreeMap<String, String>,
    crossbar: CrossbarState,
) -> FrontEnd<'a> {
    let mut fe = FrontEnd::new(&menus.registry, &menus.context, language);
    fe.state = crossbar;
    fe.sub_options = SUB_OPTIONS.get().cloned().unwrap_or_default();
    let counts = crate::crossbar_host::sub_option_counts(&fe.sub_options, &menus.context);
    fe.view = skate_core::crossbar::RegistryView::new(&menus.registry, &menus.context, counts);
    fe
}

/// The crossbar sub-option tables from the menu tables (read once at setup).
static SUB_OPTIONS: std::sync::OnceLock<std::sync::Arc<Vec<crate::crossbar_host::SubOptionRow>>> = std::sync::OnceLock::new();

fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    movies: Option<Res<RetailMenuMovies>>,
    menus: Option<ResMut<RetailMenus>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(movies), Some(mut menus)) = (movies, menus) else {
        warn!("RETAIL_MENUS stage: menu movies or tables missing");
        return;
    };
    let language = movies
        .movies
        .get("source/screens/main/core_menu")
        .map(|m| m.text_assets.language.clone())
        .unwrap_or_default();
    let _ = SUB_OPTIONS.set(std::sync::Arc::new(
        std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
            .ok()
            .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
            .map(|t| t.sub_options)
            .unwrap_or_default(),
    ));
    let result = (|| -> Result<(Stage, Vec<String>), String> {
        let mut fe = front_end(&menus, &language, CrossbarState::default());
        let mut stage = Stage::new(&fe)?;
        // Retail boot (sub_825D7E10): screens/main into _level0; its root loads the screen manager into
        // gScreenManager and the HUD / autosave / fmv levels, which land on the next updates.
        stage.load_root("source/screens/main", &movies.movies, &mut fe)?;
        for _ in 0..4 {
            stage.update(16.0, &movies.movies, &mut fe)?;
        }
        // ScreenManagerReady (825D9450; `.local/research/boot-enums.md`): the C++ answers the screen manager's
        // ready call with the boot registration.
        let mut templates = vec![CROSSBAR.to_owned()];
        if fe.calls.iter().any(|c| c == "Game.ScreenManagerReady()") {
            templates = boot_registration(&mut stage, &mut fe, &config.asset_root)?;
        }
        // Crossbar enums (sub_8261B9C8): RegisterChallengeEnum(name, "%d"), value = index in table 0x830274F0.
        for (i, name) in CROSSBAR_ENUMS.iter().enumerate() {
            stage.call_path(
                "_level0._root.RegisterChallengeEnum",
                vec![Value::Text(format!("FECrossBarScreenMode_{name}")), Value::Text(i.to_string())],
                &mut fe,
            )?;
        }
        // Popup host create (FE screen 29 vt+4, 0x825EF370): templates 17 Sk8Popup, 18 Sk8SmallPopup and 4
        // Sk8AutoSavePopup open once on the AS layer "popup" (OpenScreen bridge 0x82D09140: layer argument, the
        // template's visible flag "0"); they stay loaded and are shown / hidden per popup.
        for kind in [PopupKind::Standard, PopupKind::Small, PopupKind::AutoSave] {
            let template = kind.template();
            let Some(path) = popup_path(&config.asset_root, template) else { continue };
            if movies.movies.contains_key(&format!("source/screens/{}", path.trim_end_matches(".swf"))) {
                open_screen_on(&mut stage, &path, "popup", template, &mut fe)?;
            }
        }
        // The action overlay (template 59 SignUpHUD, the teleporter prompt): opened once like the popups, hidden
        // until a teleporter shows it (state 2).
        if movies.movies.contains_key(SIGNUP_MOVIE) {
            open_screen_on(&mut stage, "hud2/actionoverlay2.swf", "popup", SIGNUP_TEMPLATE, &mut fe)?;
        }
        Ok((stage, templates))
    })();
    match result {
        Ok((stage, templates)) => {
            // AS screen ids: 600 + the template's registration index.
            let as_id = |name: &str| 600 + templates.iter().position(|n| n == name).unwrap_or(0) as i32;
            let popup_ids: Vec<(PopupKind, i32)> = [PopupKind::Standard, PopupKind::Small, PopupKind::AutoSave]
                .into_iter()
                .filter(|k| {
                    popup_path(&config.asset_root, k.template()).is_some_and(|p| {
                        movies.movies.contains_key(&format!("source/screens/{}", p.trim_end_matches(".swf")))
                    })
                })
                .map(|k| (k, as_id(k.template())))
                .collect();
            let tables = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
                .ok()
                .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
            let white = images.add(Image::new(
                Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                TextureDimension::D2,
                vec![255; 4],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
            // The Trick Guide (FE 14): its data from the collections and the tables, its movie in the set; the pause
            // item gets its engine handler (greyed without them).
            let guide_tables = tables.as_ref().map(|t| t.trick_guide.clone()).unwrap_or_default();
            let has_movie = movies.movies.contains_key("source/screens/tricks/trickguide");
            let trick_guide = match skate_data::collections::Collections::load(&config.asset_root) {
                Ok(collections) if !guide_tables.categories.is_empty() && has_movie => {
                    let guide = skate_data::trick_guide::load(&collections, &guide_tables);
                    if menus.registry.data().item(TRICK_GUIDE_ITEM).is_some() {
                        let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::TRICK_GUIDE_SCREEN));
                        let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                        menus.registry.register_handler(EntryRef::item(TRICK_GUIDE_ITEM), handler);
                    }
                    let guide = std::sync::Arc::new(guide);
                    commands.insert_resource(crate::trick_guide_host::TrickGuideData(guide.clone()));
                    crate::trick_guide_host::TrickGuideHost::new(guide)
                }
                _ => {
                    warn!("RETAIL_MENUS: Trick Guide unavailable (tables {}, movie {has_movie}); run setup", !guide_tables.categories.is_empty());
                    Default::default()
                }
            };
            // Video Calibration (FE 51): the settings row gets its engine handler when the movie is in the set (greyed
            // otherwise, e.g. sets exported before it was added).
            if movies.movies.contains_key("source/screens/options/videocal")
                && menus.registry.data().settings.iter().any(|s| s.id == VIDEO_CALIBRATION_ROW)
            {
                let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::VIDEO_CALIBRATION_SCREEN));
                let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                menus.registry.register_handler(EntryRef::setting(VIDEO_CALIBRATION_ROW), handler);
            } else {
                warn!("RETAIL_MENUS: Video Calibration unavailable (no videocal movie); run setup");
            }
            // The Challenge Map (FE 17): with its movie in the set the pause item opens the retail screen (stacked
            // over the PC overlay's MAPS handler, which stays the fallback for sets exported before it was added;
            // test `retail_challenge_map_screen_opens_and_picks_a_location`).
            if movies.movies.contains_key("source/screens/map/challengemap") && menus.registry.data().item(CHALLENGE_MAP_ITEM).is_some() {
                let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::CHALLENGE_MAP_SCREEN));
                let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                menus.registry.register_handler(EntryRef::item(CHALLENGE_MAP_ITEM), handler);
            } else {
                warn!("RETAIL_MENUS: Challenge Map screen unavailable (no challengemap movie); the MAPS overlay stands in; run Refresh Game Files");
            }
            // Skate Profile (FE 61): the same, over the SKATER overlay handler (user 2026-10-19: port it now).
            if movies.movies.contains_key("source/screens/profile/offline_skateProfile") && menus.registry.data().item(OFFLINE_PROFILE_ITEM).is_some() {
                let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::OFFLINE_PROFILE_SCREEN));
                let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                menus.registry.register_handler(EntryRef::item(OFFLINE_PROFILE_ITEM), handler);
            } else {
                warn!("RETAIL_MENUS: Skate Profile screen unavailable (no offline_skateProfile movie); the SKATER overlay stands in; run Refresh Game Files");
            }
            // Extras (FE 18): the same, over the EXTRAS overlay handler.
            if movies.movies.contains_key("source/screens/main/extras") && menus.registry.data().item(EXTRAS_ITEM).is_some() {
                let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::EXTRAS_SCREEN));
                let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                menus.registry.register_handler(EntryRef::item(EXTRAS_ITEM), handler);
            } else {
                warn!("RETAIL_MENUS: Extras screen unavailable (no extras movie); the EXTRAS overlay stands in; run Refresh Game Files");
            }
            // Music Player (FE 26): greyed without the movie (sets exported before it was added).
            if movies.movies.contains_key(MUSIC_PLAYER_MOVIE) && menus.registry.data().item(MUSIC_PLAYER_ITEM).is_some() {
                let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::MUSIC_PLAYER_SCREEN));
                let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                menus.registry.register_handler(EntryRef::item(MUSIC_PLAYER_ITEM), handler);
            }
            // SpawnSkater (FE 62): Free Play (SoloFreeskateOptions) and Call Skater (SkateWith) push it (b307); greyed
            // without the movie (sets before version 14).
            if movies.movies.contains_key(SPAWN_SKATER_MOVIE) {
                for (item, _) in skate_core::spawn_skater::SpawnSkaterRules::default().items {
                    if menus.registry.data().item(&item).is_some() {
                        let owner = Owner::Engine(format!("{}{}", crate::retail_menus::SCREEN, crate::fe_backdrop::SPAWN_SKATER_SCREEN));
                        let handler = skate_core::menus::Handler { owner, select: true, highlight: false, callback: None };
                        menus.registry.register_handler(EntryRef::item(&item), handler);
                    }
                }
            } else {
                warn!("RETAIL_MENUS: SpawnSkater screen unavailable (no spawnskater movie); run Refresh Game Files");
            }
            info!("RETAIL_MENUS stage ready (F8 opens the retail pause menu)");
            // The replay screen: the first whose movie was exported and whose template is registered (usize::MAX: none).
            let replay_screen = REPLAY_SCREENS
                .iter()
                .position(|r| movies.movies.contains_key(r.movie) && templates.iter().any(|n| n == r.template))
                .unwrap_or(usize::MAX);
            commands.insert_resource(MenuStage {
                save_tag: Default::default(),
                stage,
                crossbar: CrossbarState::default(),
                rules: CrossbarRules::default(),
                language,
                stack: FeStack::default(),
                textures: BTreeMap::new(),
                white,
                slots: Vec::new(),
                failed: false,
                restart_due: false,
                auto_open: std::env::var("SKATE3_RETAIL_MENUS_OPEN").as_deref() == Ok("1"),
                crossbar_id: as_id(CROSSBAR),
                options_id: as_id(OPTIONS),
                trick_guide_id: as_id(TRICK_GUIDE),
                replay_id: REPLAY_SCREENS.get(replay_screen).and_then(|r| templates.iter().position(|n| n == r.template)).map(|i| 600 + i as i32),
                replay_help_id: REPLAY_SCREENS
                    .get(replay_screen)
                    .filter(|r| movies.movies.contains_key(r.help_movie))
                    .and_then(|r| templates.iter().position(|n| n == r.help))
                    .map(|i| 600 + i as i32),
                replay_screen,
                replay_menu: Default::default(),
                replay_shown: false,
                trick_guide_base: trick_guide.guide.clone(),
                trick_guide_generation: 0,
                fade: Default::default(),
                fade_params: Default::default(),
                fade_carry: 0.0,
                trick_guide_fade_in: None,
                trick_guide,
                challenge_map_id: as_id(CHALLENGE_MAP),
                challenge_map: Default::default(),
                offline_profile_id: as_id(OFFLINE_PROFILE),
                offline_profile: Default::default(),
                reward_summary: Default::default(),
                reward_generation: 0,
                reward_ids: REWARD_SCREENS.map(|(movie, _, template)| {
                    templates.iter().position(|n| n == template).filter(|_| movies.movies.contains_key(movie)).map(|i| 600 + i as i32)
                }),
                reward_open: None,
                reward_shown: false,
                end_ids: END_DIALOGS.map(|(movie, _, template)| {
                    templates.iter().position(|n| n == template).filter(|_| movies.movies.contains_key(movie)).map(|i| 600 + i as i32)
                }),
                end_open: None,
                end_shown: false,
                end_frames: 0,
                reward_frames: 0,
                poster_id: templates.iter().position(|n| n == POSTER.2).filter(|_| movies.movies.contains_key(POSTER.0)).map(|i| 600 + i as i32),
                poster_open: false,
                poster_shown: false,
                poster_frames: 0,
                profile_generation: (0, 0),
                extras_id: as_id(EXTRAS),
                extras: Default::default(),
                cheats: Default::default(),
                pending_entry: None,
                viewer_id: as_id(TEXT_VIEWER),
                tos: Default::default(),
                credits_id: as_id(CREDITS),
                credits: Default::default(),
                spawn_skater_id: as_id(SPAWN_SKATER),
                music_player_id: as_id(MUSIC_PLAYER),
                music_player: Default::default(),
                spawn_skater: Default::default(),
                spawn_skater_mode: Default::default(),
                swap_due: None,
                swap_loading: false,
                tips_loader_id: as_id(TIPS_LOADER),
                retail_lines: None,
                load_after: 0.0,
                tips_loader: crate::tips_loader_host::TipsLoaderHost {
                    lines: skate_data::collections::Collections::load(&config.asset_root).map(|c| skate_data::tips_loader::read(&c)).unwrap_or_default(),
                    seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() | 1),
                    ..Default::default()
                },
                load: Default::default(),
                load_up: false,
                load_gone: false,
                load_map: None,
                free_skate_id: as_id(FREE_SKATE),
                free_skate: Default::default(),
                image_slots: Default::default(),
                video_cal_id: as_id(VIDEO_CALIBRATION),
                video_cal: Default::default(),
                video_cal_rules: Default::default(),
                popups: Default::default(),
                popup: Default::default(),
                popup_ids,
                popup_dirty: false,
                signup_id: movies.movies.contains_key(SIGNUP_MOVIE).then(|| as_id(SIGNUP_TEMPLATE)),
                signup_shown: None,
                chyron: false,
                settings: Default::default(),
                settings_titles: tables.map(|t| t.settings_titles).unwrap_or_default(),
                apt_keys: apt_keys(&config.asset_root),
                pads: Default::default(),
                repeat: RepeatRule::RETAIL,
                injected: injected_keys(&std::env::var("SKATE3_RETAIL_MENUS_KEYS").unwrap_or_default()),
            });
        }
        Err(e) => error!("RETAIL_MENUS stage failed: {e}"),
    }
}

/// `SKATE3_RETAIL_MENUS_KEYS` tokens: a key index, `_` (one empty frame) or `_xN` (N empty frames).
fn injected_keys(list: &str) -> std::collections::VecDeque<Option<u32>> {
    let mut out = std::collections::VecDeque::new();
    for token in list.split(',').map(str::trim) {
        match token.strip_prefix('_') {
            Some(rest) => {
                let frames = rest.strip_prefix('x').and_then(|n| n.parse().ok()).unwrap_or(1);
                out.extend(std::iter::repeat_n(None, frames));
            }
            None => out.extend(token.parse().ok().map(Some)),
        }
    }
    out
}

fn update(
    stage: Option<ResMut<MenuStage>>,
    movies: Option<Res<RetailMenuMovies>>,
    menus: Option<ResMut<RetailMenus>>,
    keys: Res<ButtonInput<KeyCode>>,
    // Real time: the menus keep running while they suspend gameplay (virtual time paused).
    time: Res<Time<Real>>,
    mut sounds: MessageWriter<FrontendSound>,
    mut backdrop: ResMut<crate::fe_backdrop::FeBackdrop>,
    mut overlay: Option<ResMut<crate::graphics_menu::Menu>>,
    mut fe_hud: ResMut<crate::fe_backdrop::FeHud>,
    mut available: ResMut<RetailPause>,
    mut warned: Local<bool>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    // Mods' Trick Guide and Skate Profile layers (one parameter: the system is at Bevy's 16).
    (guide_mods, profile_mods, credits_mods, config, mut loading_mods, career, career_profile, mut reward_screens, mut end_screen, mut poster_screen): (
        Res<crate::trick_guide_host::TrickGuideMods>,
        Res<crate::offline_profile_host::ProfileMods>,
        Res<crate::extras_host::CreditsMods>,
        Res<crate::config::Config>,
        ResMut<crate::tips_loader_host::LoadingMods>,
        Option<Res<crate::career_stats_host::CareerStatsHost>>,
        Option<Res<crate::career::CareerProfile>>,
        Option<ResMut<crate::career::RewardScreens>>,
        Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
        Option<ResMut<crate::poster_host::PosterScreen>>,
    ),
    // The Trick Guide's demo and the text entry's keyboard (one parameter: the system is at Bevy's 16).
    (mut demo, mut text_entry, mut marquee): (ResMut<crate::trick_guide_scene::TrickGuideDemo>, ResMut<crate::extras_host::TextEntryView>, ResMut<crate::marquee_preview::MarqueeView>),
    // Popups; the instant replay feed and whether the retail replay screen shows (one parameter: Bevy's 16).
    (mut popup_mods, replay_feed, mut replay_ui, mut look_screen, mut music_link): (
        ResMut<crate::popup_host::PopupMods>,
        Option<Res<crate::replay::ReplayFeed>>,
        Option<ResMut<crate::replay::RetailReplayUi>>,
        Option<ResMut<crate::replay::ReplayLookScreen>>,
        ResMut<crate::music_player_host::MusicPlayerLink>,
    ),
    // Challenge Map travel; cheats out, typed text in and the zombie flag the cheat toggles; FE 62's characters and
    // its requests out.
    (mut travel, mut cheats_out, mut unlocks_out, mut typing, mut living, mut characters, mut spawner_out, population, transition, mut cover): (
        ResMut<crate::challenge_map::ChallengeMapTravel>,
        MessageWriter<crate::extras_host::CheatChanged>,
        MessageWriter<crate::extras_host::UnlockGranted>,
        MessageReader<bevy::input::keyboard::KeyboardInput>,
        Option<ResMut<crate::living_world::LivingWorldSettings>>,
        Option<ResMut<crate::custom_models::CustomModels>>,
        MessageWriter<crate::spawn_skater_host::SpawnSkaterRequest>,
        Option<Res<crate::living_world::PopulationState>>,
        Res<crate::map_transition::MapTransition>,
        ResMut<crate::map_transition::LoadingCover>,
    ),
) {
    let ready = stage.as_ref().is_some_and(|s| !s.failed) && movies.is_some() && menus.is_some();
    popup_mods.stage_ready = ready;
    popup_mods.signup_ready = ready && stage.as_ref().is_some_and(|s| s.signup_id.is_some());
    // The ported screens in this build's screen table (TU3 FE ids 56 and 45); none in tables extracted before
    // screens.
    let pause = backdrop.index(crate::fe_backdrop::PAUSE_SCREEN);
    let options = backdrop.index(crate::fe_backdrop::SETTINGS_SCREEN);
    let trick = backdrop.index(crate::fe_backdrop::TRICK_GUIDE_SCREEN);
    let cal = backdrop.index(crate::fe_backdrop::VIDEO_CALIBRATION_SCREEN);
    let map = backdrop.index(crate::fe_backdrop::CHALLENGE_MAP_SCREEN);
    let profile = backdrop.index(crate::fe_backdrop::OFFLINE_PROFILE_SCREEN);
    let extras = backdrop.index(crate::fe_backdrop::EXTRAS_SCREEN);
    let entry = backdrop.index(crate::fe_backdrop::TEXT_ENTRY_SCREEN);
    let viewer = backdrop.index(crate::fe_backdrop::TEXT_VIEWER_SCREEN);
    let credits = backdrop.index(crate::fe_backdrop::CREDITS_SCREEN);
    let spawn = backdrop.index(crate::fe_backdrop::SPAWN_SKATER_SCREEN);
    let free = backdrop.index(crate::fe_backdrop::FREE_SKATE_SCREEN);
    let music = backdrop.index(crate::fe_backdrop::MUSIC_PLAYER_SCREEN);
    available.set_if_neq(RetailPause(ready && pause.is_some()));
    if ready && pause.is_none() && !std::mem::replace(&mut *warned, true) {
        warn!("RETAIL_MENUS: the menu tables have no FE screens yet (run setup); Start opens the PC overlay until then");
    }
    let (Some(mut ms), Some(movies), Some(mut menus)) = (stage, movies, menus) else { return };
    // Game Settings > Units (distances on the Stats panel in feet when imperial).
    let imperial = matches!(
        crate::retail_menus::setting(&menus, "ID_GAMESETTINGS_UNITS_TOGGLE"),
        Some(skate_core::menus::SettingValue::Int(1) | skate_core::menus::SettingValue::Bool(true))
    );
    if ms.failed {
        // A stopped stage gives the game back: no suspended gameplay, no pause backdrop (Start then opens the PC
        // overlay). Without this a stage error with the pause menu open froze the game (session 2026-10-12).
        if let Some(overlay) = overlay.as_mut().filter(|o| o.fe_suspended) {
            warn!("RETAIL_MENUS stage stopped with the pause menu open: gameplay resumed");
            overlay.fe_suspended = false;
            backdrop.set_stack(Vec::new());
            *fe_hud = crate::fe_backdrop::FeHud::default();
        }
        demo.set_if_neq(default());
        ms.restart_due = true;
        return;
    }
    let ms = &mut *ms;
    // The loading screen, when the set has its movie.
    let loader = backdrop.index(crate::fe_backdrop::LOADING_SCREEN).filter(|_| movies.movies.contains_key(TIPS_LOADER_MOVIE));
    // Retail Apt keys fired this frame (pad and keyboard); none while the PC overlay is open on top or a load runs.
    let mut fired = poll_keys(ms, &keys, time.delta_secs());
    if overlay.as_ref().is_some_and(|o| o.open) || ms.load.active() {
        fired.clear();
    }
    let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
    fe.settings = std::mem::take(&mut ms.settings);
    fe.settings_titles = ms.settings_titles.clone();
    fe.settings.open = options.is_some() && ms.stack.top(Layer::Main) == options;
    fe.trick_guide = std::mem::take(&mut ms.trick_guide);
    fe.replay_menu = std::mem::take(&mut ms.replay_menu);
    // Mods changed the guide: rebuild it over the retail one; an open screen starts over at the top.
    if guide_mods.generation != ms.trick_guide_generation && !ms.trick_guide_base.categories.is_empty() {
        ms.trick_guide_generation = guide_mods.generation;
        fe.trick_guide.guide = std::sync::Arc::new(guide_mods.apply(&ms.trick_guide_base));
        let open = fe.trick_guide.open;
        fe.trick_guide.reset();
        if open {
            fe.trick_guide.states.push(skate_core::trick_guide::movie_state::BACK);
        }
    }
    fe.trick_guide.open = trick.is_some() && ms.stack.top(Layer::Main) == trick;
    fe.challenge_map = std::mem::take(&mut ms.challenge_map);
    fe.image_slots = std::mem::take(&mut ms.image_slots);
    fe.challenge_map.open = map.is_some() && ms.stack.top(Layer::Main) == map;
    fe.offline_profile = std::mem::take(&mut ms.offline_profile);
    fe.reward_summary = std::mem::take(&mut ms.reward_summary);
    if let Some(e) = end_screen.as_mut() {
        let available = screens_switch() && ms.end_ids[1].is_some() && ms.end_ids[2].is_some();
        if e.available != available {
            e.available = available;
        }
        fe.end_dialog = e.open.then(|| crate::end_dialog_host::EndDialogHost { dialog: e.dialog.clone(), title: e.title.clone(), results: e.results.clone() });
    }
    if let Some(p) = poster_screen.as_mut() {
        let available = screens_switch() && ms.poster_id.is_some();
        if p.available != available {
            p.available = available;
        }
        let teammates = career_profile.as_ref().map(|c| c.teammates.clone()).unwrap_or_default();
        fe.poster = p.poster.clone().map(|poster| crate::poster_host::PosterHost { poster, teammates });
    }
    if let Some(r) = reward_screens.as_mut() {
        if r.generation != ms.reward_generation {
            ms.reward_generation = r.generation;
            fe.reward_summary.view = r.view.clone();
        }
        let available = screens_switch() && ms.reward_ids.iter().all(Option::is_some);
        if r.available != available {
            r.available = available;
        }
    }
    // The career or mods changed the profile pages: rebuild them; an open screen shows its tab's page again.
    let profile_generation = (profile_mods.generation, career_profile.as_ref().map_or(0, |p| p.generation));
    if profile_generation != ms.profile_generation {
        ms.profile_generation = profile_generation;
        let base = career_profile.as_ref().map(|p| p.data.clone()).unwrap_or_default();
        fe.offline_profile.data = profile_mods.apply(&base);
        if profile.is_some_and(|p| ms.stack.top(Layer::Main) == Some(p)) {
            let p = &mut fe.offline_profile;
            p.states.extend(p.screen.refresh(&p.rules));
        }
    }
    fe.popup = std::mem::take(&mut ms.popup);
    fe.extras = std::mem::take(&mut ms.extras);
    fe.tos = std::mem::take(&mut ms.tos);
    fe.credits = std::mem::take(&mut ms.credits);
    fe.extras.open = extras.is_some() && ms.stack.top(Layer::Main) == extras;
    fe.spawn_skater = std::mem::take(&mut ms.spawn_skater);
    fe.spawn_skater.open = spawn.is_some() && ms.stack.top(Layer::Main) == spawn;
    let preview_open = fe.spawn_skater.open && fe.spawn_skater.screen.mode != skate_core::spawn_skater::Mode::SkateWith;
    fe.free_skate = std::mem::take(&mut ms.free_skate);
    fe.free_skate.open = free.is_some() && ms.stack.top(Layer::Main) == free;
    fe.music_player = std::mem::take(&mut ms.music_player);
    fe.music_player.open = music.is_some() && ms.stack.top(Layer::Main) == music;
    fe.tips_loader = std::mem::take(&mut ms.tips_loader);
    // The on-screen keyboard (FE 46 on top): typed text and Escape (ours), once per frame; the pad keys go through the
    // key loop below.
    let entry_open = entry.is_some_and(|e| ms.stack.top(Layer::Main) == Some(e)) && text_entry.keyboard.is_some();
    let mut entry_outcome = skate_core::text_entry::KeyboardOutcome::None;
    for event in typing.read() {
        if entry_open && event.state == bevy::input::ButtonState::Pressed && !keys.pressed(KeyCode::ControlLeft) && !keys.pressed(KeyCode::ControlRight) {
            if let (Some(t), Some(kb)) = (&event.text, text_entry.keyboard.as_mut()) {
                t.chars().filter(|c| !c.is_control()).for_each(|c| kb.type_char(c));
                info!("TEXT_ENTRY typed {t:?} text={:?}", kb.text);
            }
        }
    }
    if entry_open && keys.just_pressed(KeyCode::Escape) {
        entry_outcome = skate_core::text_entry::KeyboardOutcome::Cancel;
    }
    if let Some(skater) = &skater {
        fe.trick_guide.stance.regular = skater.animation.regular_stance();
        fe.trick_guide.stance.alternate = skater.animation.riding_switch();
    }
    // Work for the registry once the movie work is done: the crossbar item, settings steps.
    let mut selected: Option<String> = None;
    let mut steps: Vec<(String, Direction)> = Vec::new();
    // Mod popup choices, sent to the mods once the movie work is done.
    let mut mod_events: Vec<(String, serde_json::Value)> = Vec::new();
    let result = (|| -> Result<(), String> {
        let (crossbar_id, options_id, trick_id, cal_id) = (ms.crossbar_id, ms.options_id, ms.trick_guide_id, ms.video_cal_id);
        let map_id = ms.challenge_map_id;
        let profile_id = ms.offline_profile_id;
        // The instant replay screen (FE 12 in instant mode): opened with the replay, shown after its IntroComplete
        // (b391: retail never calls the movie's InstantDone; the screen manager's ShowScreen makes it visible), then
        // fed every frame and told what changed (SetState 18 / 19 / 999 / 1002 / 1003); closed with the replay.
        if let Some(replay_id) = ms.replay_id {
            let feed = replay_feed.as_ref().map(|f| f.0.clone()).filter(|f| f.active);
            match (feed, fe.replay_menu.open) {
                (Some(feed), false) => {
                    info!("RETAIL_MENUS open {} (FE 12, instant replay)", REPLAY_SCREENS[ms.replay_screen].template);
                    fe.replay_menu.reset();
                    fe.replay_menu.open = true;
                    fe.replay_menu.feed = feed;
                    ms.replay_shown = false;
                    open_screen(&mut ms.stage, REPLAY_SCREENS[ms.replay_screen].path, REPLAY_SCREENS[ms.replay_screen].template, &mut fe)?;
                }
                (Some(feed), true) => fe.replay_menu.feed = feed,
                (None, true) => {
                    info!("RETAIL_MENUS close {}", REPLAY_SCREENS[ms.replay_screen].template);
                    if let Some(help_id) = ms.replay_help_id.filter(|_| fe.replay_menu.help_open) {
                        close(&mut ms.stage, help_id, &mut fe)?;
                    }
                    fe.replay_menu.open = false;
                    fe.replay_menu.help_open = false;
                    ms.replay_shown = false;
                    close(&mut ms.stage, replay_id, &mut fe)?;
                }
                (None, false) => {}
            }
        }
        // The reward presentation (docs/hails-additions/43-career.md#change-the-reward-presentation-careerpresentationrs):
        // the career names the open screen; each change closes the last movie and opens the next (a new card opens
        // its movie again), shown once its intro completed.
        let wanted = reward_screens.as_ref().and_then(|r| r.open).map(|(s, i)| (s as usize, i));
        if wanted != ms.reward_open {
            if let Some(id) = ms.reward_open.and_then(|(s, _)| ms.reward_ids[s]) {
                close(&mut ms.stage, id, &mut fe)?;
            }
            if let Some((s, i)) = wanted.filter(|(s, _)| ms.reward_ids[*s].is_some()) {
                info!("RETAIL_MENUS open {} {i}", REWARD_SCREENS[s].2);
                open_screen(&mut ms.stage, REWARD_SCREENS[s].1, REWARD_SCREENS[s].2, &mut fe)?;
            }
            ms.reward_open = wanted;
            ms.reward_shown = false;
            ms.reward_frames = 0;
        }
        // The end dialog (FE 4): opened while the run waits in its end options, closed once a pick was made.
        let wanted_end = end_screen.as_ref().filter(|e| e.open).map(|e| e.movie() as usize);
        if wanted_end != ms.end_open {
            if let Some(id) = ms.end_open.and_then(|m| ms.end_ids[m]) {
                close(&mut ms.stage, id, &mut fe)?;
            }
            if let Some(m) = wanted_end.filter(|m| ms.end_ids[*m].is_some()) {
                info!("RETAIL_MENUS open {} (FE 4)", END_DIALOGS[m].2);
                open_screen(&mut ms.stage, END_DIALOGS[m].1, END_DIALOGS[m].2, &mut fe)?;
            }
            ms.end_open = wanted_end;
            ms.end_shown = false;
            ms.end_frames = 0;
        }
        // The sign-up poster: opened while the sign-up waits on it, closed on its A / B.
        let wanted_poster = poster_screen.as_ref().is_some_and(|p| p.poster.is_some());
        if let Some(id) = ms.poster_id.filter(|_| wanted_poster != ms.poster_open) {
            if wanted_poster {
                info!("RETAIL_MENUS open {} (FE 0x1B)", POSTER.2);
                open_screen(&mut ms.stage, POSTER.1, POSTER.2, &mut fe)?;
            } else {
                close(&mut ms.stage, id, &mut fe)?;
            }
            ms.poster_open = wanted_poster;
            ms.poster_shown = false;
            ms.poster_frames = 0;
        }
        let auto =ms.auto_open && time.elapsed_secs() > 4.0;
        ms.auto_open &= !auto;
        if auto {
            fired.insert(0, (APT_START, 0));
        }
        if ms.stack.input_target().is_some() || ms.end_open.is_some() || ms.reward_open.is_some() || ms.poster_open {
            fired.extend(ms.injected.pop_front().flatten().map(|k| (k, 0)));
        }
        for (key, controller) in fired {
            // Session log: every key the menus took, by retail's Apt key name (the user's play tests).
            info!("RETAIL_MENUS key {} pad {controller} screen {:?}", ms.apt_keys.get(key as usize).map_or("?", |k| k.name.as_str()), ms.stack.input_target().and_then(|t| backdrop.id(t)));
            // A / B / X / Y on the shown end dialog pick its slot's option (825F12E8).
            if let (Some(_), Some(slot)) = (ms.end_open, END_DIALOG_KEYS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)) {
                if ms.end_shown {
                    if let Some(action) = end_screen.as_mut().and_then(|e| e.press(slot)) {
                        info!("RETAIL_MENUS end dialog slot {slot}: {action:?}");
                    }
                }
                continue;
            }
            // A / B on the shown poster start or cancel (825ED410); other keys reach its movie (the option picker).
            if let (true, Some(id)) = (ms.poster_open, ms.poster_id) {
                if ms.poster_shown {
                    if let (Some(p), Some(host)) = (poster_screen.as_mut(), fe.poster.as_ref()) {
                        if p.poster.is_some() {
                            p.poster = Some(host.poster.clone());
                        }
                    }
                    match poster_screen.as_mut().and_then(|p| p.press(key as u8)) {
                        Some(outcome) => {
                            info!("RETAIL_MENUS poster key {key}: {outcome:?}");
                            fe.poster = None;
                        }
                        None => {
                            ms.stage.call_path(
                                "_global.ScreenManager.UpdateInput",
                                vec![Value::Text(id.to_string()), Value::Text(key.to_string()), Value::Text(controller.to_string())],
                                &mut fe,
                            )?;
                        }
                    }
                }
                continue;
            }
            // A on an open reward screen goes to the presentation (C++ 82650738 / 826504E0, b899), once it shows.
            if key == APT_NEXT && ms.reward_open.is_some() {
                if ms.reward_shown {
                    if let Some(r) = reward_screens.as_mut() {
                        r.accepted = true;
                    }
                }
                continue;
            }
            // sub_825EC0E8 step 2: while the popup list has entries, every key goes to the popup host only; its C++
            // handler (vt+40) returns 0, so the movie's UpdateInput gets the key once the host is ready (+49).
            if let Some((_, request)) = ms.popups.shown() {
                let handle = ms.popup_ids.iter().find(|(k, _)| *k == request.kind).map(|(_, id)| *id);
                if let (Some(handle), true) = (handle, fe.popup.input_ready) {
                    ms.stage.call_path(
                        "_global.ScreenManager.UpdateInput",
                        vec![Value::Text(handle.to_string()), Value::Text(key.to_string()), Value::Text(controller.to_string())],
                        &mut fe,
                    )?;
                }
                continue;
            }
            // The on-screen keyboard (FE 46 on top; ours): the pad drives the grid, Start finishes. A PC keyboard's
            // Backspace and Enter arrive as B / A here, so the raw keys pick delete / done instead.
            if entry_open {
                use skate_core::text_entry::KeyboardInput as K;
                let input = match key {
                    APT_START => Some(K::Done),
                    APT_PREVIOUS if keys.just_pressed(KeyCode::Backspace) => Some(K::Backspace),
                    APT_PREVIOUS => Some(K::Cancel),
                    APT_NEXT if keys.just_pressed(KeyCode::Enter) => Some(K::Done),
                    APT_NEXT => Some(K::Press),
                    APT_MISC => Some(K::Backspace),
                    APT_OPTION => Some(K::Shift),
                    k if KEYS_UP.contains(&k) => Some(K::Up),
                    k if KEYS_DOWN.contains(&k) => Some(K::Down),
                    k if KEYS_LEFT.contains(&k) => Some(K::Left),
                    k if KEYS_RIGHT.contains(&k) => Some(K::Right),
                    _ => None,
                };
                let layout = text_entry.layout.clone();
                if let (Some(i), Some(kb)) = (input, text_entry.keyboard.as_mut()) {
                    let out = kb.input(i, &layout);
                    // Session log (laptop 2026-10-19: "the cheat entry menu is unusable"): every key the keyboard took.
                    info!("TEXT_ENTRY input={i:?} row={} col={} key={:?} text={:?} shift={} outcome={out:?}", kb.row, kb.col, kb.key(&layout), kb.text, kb.shift);
                    if out != skate_core::text_entry::KeyboardOutcome::None {
                        entry_outcome = out;
                    }
                }
                continue;
            }
            if entry.is_some() && ms.stack.input_target() == entry {
                warn!("TEXT_ENTRY key {key} reached FE 46 but the keyboard is not taking input (top={:?}, keyboard={})", ms.stack.top(Layer::Main), text_entry.keyboard.is_some());
            }
            if key == APT_START {
                // 825EB998 pushes no pause menu over the replay screen; there Start opens the replay help (b500).
                if fe.replay_menu.open {
                    continue;
                }
                // Retail Start (sub_825EB998, mode 0) pushes FE 56 with the plain push. NOT RETAIL YET (unverified):
                // Start while it is open closes it.
                match pause {
                    Some(p) if ms.stack.active(p) => ms.stack.request(Layer::Main, Request::Close { screen: p }),
                    Some(p) => ms.stack.request(Layer::Main, Request::push(p)),
                    // Tables from before the FE screen records: the PC overlay takes Start (`RetailPause`).
                    None => {}
                }
                continue;
            }
            let target = ms.stack.input_target();
            let Some(screen) = target.filter(|t| [pause, options, trick, cal, map, profile, extras, viewer, credits, spawn, free, music].contains(&Some(*t))) else { continue };
            // sub_825EC0E8: the screen's C++ handler first, then the movie's UpdateInput when the handler did not
            // consume the key and the screen's record says so (+12).
            let mut consumed = false;
            let movie_id = if Some(screen) == pause {
                crossbar_id
            } else if Some(screen) == options {
                options_id
            } else if Some(screen) == cal {
                cal_id
            } else if Some(screen) == map {
                map_id
            } else if Some(screen) == profile {
                profile_id
            } else if Some(screen) == extras {
                ms.extras_id
            } else if Some(screen) == viewer {
                ms.viewer_id
            } else if Some(screen) == credits {
                ms.credits_id
            } else if Some(screen) == spawn {
                ms.spawn_skater_id
            } else if Some(screen) == free {
                ms.free_skate_id
            } else if Some(screen) == music {
                ms.music_player_id
            } else {
                trick_id
            };
            if Some(screen) == pause && ms.fade.held(FADE_SCREEN) {
                // The TrickGuide case sets the crossbar's closing flags (+380 / +52) while the fade runs: no input.
                continue;
            }
            if Some(screen) == trick && (ms.fade.held(FADE_SCREEN) || ms.trick_guide_fade_in.is_some()) {
                // Input waits for state 2 (0x82619A20 releases the fade first; vt+40 needs S+192 == 2) and stops while
                // the close fades out (S+52).
                continue;
            }
            if Some(screen) == free {
                // Screen 34 input 8260A4E8 (b564): Up / Down pick the row (sound, SetState 999), Left / Right step it
                // (SetState row; the movie plays the slider sound), B writes back and closes; the movie takes no keys.
                use skate_core::free_skate_options::Key;
                let k = match key {
                    APT_PREVIOUS => Some(Key::Back),
                    k if KEYS_UP.contains(&k) => Some(Key::Up),
                    k if KEYS_DOWN.contains(&k) => Some(Key::Down),
                    k if KEYS_LEFT.contains(&k) => Some(Key::Left),
                    k if KEYS_RIGHT.contains(&k) => Some(Key::Right),
                    _ => None,
                };
                consumed = true;
                if let Some(k) = k {
                    let f = &mut fe.free_skate;
                    let out = f.screen.key(k, &f.rules);
                    if let Some(sound) = &out.sound {
                        sounds.write(FrontendSound::named(sound));
                    }
                    f.states.extend(out.states);
                    if let Some(values) = out.close {
                        // Flow 0x8B: the values go to the session (Free Play only), then the screen closes.
                        info!("RETAIL_MENUS free play options {values:?}");
                        if let Some(l) = living.as_mut() {
                            l.free_play = Some(values);
                        }
                        ms.stack.request(Layer::Main, Request::Close { screen });
                    }
                }
            } else if Some(screen) == spawn {
                // Screen 62 input 0x8264DF18 (b543): B closes (not in the demo mode), X dismisses a called skater or opens
                // the Free Play options; never consumed (NOT RETAIL YET: the return value is not read), so the movie's
                // picker also gets every key. Y (gamercard) is console platform UI.
                let s = &mut fe.spawn_skater;
                let out = match key {
                    APT_PREVIOUS => s.screen.back(&s.rules),
                    APT_MISC => s.screen.x(&s.rules),
                    _ => Default::default(),
                };
                s.take(out);
            } else if Some(screen) == music {
                // Screen 26 input 0x8260B6F0: tabs, edit, song toggles, volume rows, Back (music_player_host::key).
                consumed = fe.music_player.key(key);
            } else if Some(screen) == credits {
                // Screen 25 input 0x82608DC8 (b489): Back pops the screen, no sound, at any time; the movie takes no
                // keys (its record's input flag is off).
                if key == APT_PREVIOUS {
                    ms.stack.request(Layer::Main, Request::Close { screen });
                }
            } else if Some(screen) == viewer {
                // Screen 3 input 0x8263A5A8 (b322), view mode: Back plays core_b_button and closes; the movie scrolls
                // with the left stick (not consumed).
                if key == APT_PREVIOUS {
                    sounds.write(FrontendSound::named("core_b_button"));
                    ms.stack.request(Layer::Main, Request::Close { screen });
                }
            } else if Some(screen) == extras {
                // Screen 18 input 0x82655658 (b320): Back plays core_b_button and leaves the Skate Reel list (SetState(10))
                // or pops the screen; never consumed, so every key also reaches the movie's picker.
                if key == APT_PREVIOUS {
                    let x = &mut fe.extras;
                    let out = x.screen.back(&x.rules);
                    sounds.write(FrontendSound::named(&out.sound));
                    match out.state {
                        Some(state) => x.states.push(state),
                        None => ms.stack.request(Layer::Main, Request::Close { screen }),
                    }
                }
            } else if Some(screen) == profile {
                // Screen 61 input 0x8264C608 (b308): Back closes, the tab keys step the tab, Up / Down scroll in the
                // movie; Y (gamercard) and X (achievements) are console platform UI with no PC equivalent.
                use skate_core::offline_profile::ProfileKey;
                let key = match key {
                    APT_PREVIOUS => Some(ProfileKey::Back),
                    PROFILE_NEXT_TAB => Some(ProfileKey::NextTab),
                    PROFILE_PREVIOUS_TAB => Some(ProfileKey::PreviousTab),
                    k if PROFILE_SCROLL.contains(&k) => Some(ProfileKey::Scroll),
                    _ => None,
                };
                if let Some(k) = key.filter(|_| fe.offline_profile.loaded) {
                    let p = &mut fe.offline_profile;
                    let out = p.screen.key(k, &p.rules);
                    consumed = out.consumed;
                    if let Some(sound) = &out.sound {
                        sounds.write(FrontendSound::named(sound));
                    }
                    p.states.extend(out.states);
                    if out.close {
                        ms.stack.request(Layer::Main, Request::Close { screen });
                    }
                }
            } else if Some(screen) == map {
                // Screen 17 input 82646B28 (b256): B hides the legend, pops a folder or closes at the root; the tab
                // keys switch tabs (clamped). Up / down / A stay with the movie's picker (OnMenuHilight / OnMenuSelect).
                // NOT RETAIL YET: Y (legend) and X (online difficulty) are not mapped to their Apt keys.
                let cm = &mut fe.challenge_map;
                if key == APT_PREVIOUS {
                    consumed = true;
                    sounds.write(FrontendSound::named("core_b_button"));
                    let action = cm.state.back(fe.registry, fe.context);
                    cm.apply(action);
                } else if MAP_NEXT_TAB.contains(&key) || MAP_PREVIOUS_TAB.contains(&key) {
                    consumed = true;
                    let action = cm.state.step_tab(fe.registry, fe.context, MAP_NEXT_TAB.contains(&key));
                    if action != skate_core::challenge_map::Action::None {
                        sounds.write(FrontendSound::named("core_tab"));
                    }
                    cm.apply(action);
                }
            } else if Some(screen) == cal {
                // Screen 51 input 0x8260E890: A advances (0x8260E950), B goes back a page or closes; both play their
                // sound and leave the key to the movie too (the handler returns 0).
                let rules = ms.video_cal_rules.clone();
                let action = match key {
                    APT_NEXT => {
                        sounds.write(FrontendSound::named(&rules.sound_next));
                        Some(ms.video_cal.advance(&rules))
                    }
                    APT_PREVIOUS => {
                        sounds.write(FrontendSound::named(&rules.sound_back));
                        Some(ms.video_cal.back())
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    run_calibration(&mut ms.stage, &mut ms.stack, &mut ms.popups, &mut ms.popup_dirty, &mut ms.video_cal, &ms.video_cal_rules, cal_id, action, screen, &mut fe)?;
                }
            } else if Some(screen) == trick {
                // Screen 14 vt+40 (0x82618F38): Back closes at level 0 or goes up a level (SetState(0)); A on a row
                // without a list is consumed. Back plays core_b_button (h64 0x4D927CD64CC8B818).
                let tg = &mut fe.trick_guide;
                match key {
                    APT_PREVIOUS => {
                        sounds.write(FrontendSound::named("core_b_button"));
                        match tg.state.back() {
                            // Level 0: hold the fade; the screen closes when black (callback 0x82618EF0).
                            skate_core::trick_guide::Back::Close => {
                                let params = ms.fade_params;
                                if let Some(action) = ms.fade.hold(FADE_SCREEN, Some(FadeAction::Close(screen)), false, &params) {
                                    run_fade_action(&mut ms.stack, action);
                                }
                            }
                            skate_core::trick_guide::Back::Up(state) => tg.states.push(state),
                        }
                    }
                    APT_NEXT => consumed = tg.state.consumes_select(&tg.guide),
                    _ => {}
                }
            } else if Some(screen) == pause {
                // Crossbar sub_8261B560: consumes only its close branch.
                if let Some(event) = NavEvent::from_key_index(key) {
                    let out = fe.state.handle(event, &fe.view, &ms.rules, false);
                    if let Some(sound) = &out.sound {
                        sounds.write(FrontendSound::named(sound));
                    }
                    if let Some(state) = out.set_state {
                        // Retail passes both as "%d" strings (sub_8263F320 index 10).
                        set_state(&mut ms.stage, crossbar_id, state, &mut fe)?;
                    }
                    match out.action {
                        Some(CrossbarAction::Close) => {
                            consumed = true;
                            ms.stack.request(Layer::Main, Request::Close { screen });
                        }
                        Some(CrossbarAction::SelectItem { tab, row }) => {
                            selected = fe.view.item(tab, row).map(str::to_owned);
                        }
                        _ => {}
                    }
                }
            } else {
                // Settings sub_8260C258: Back (3) leaves a sub-screen for the main list (sub_8260D078) or closes the
                // screen; Left / Right (14 / 15) step the highlighted row's value, then SetState(50) refreshes.
                // NOT RETAIL YET: the per-row UI sounds are fe records not named yet; nav left / right stand in.
                match key {
                    APT_PREVIOUS => {
                        consumed = true;
                        sounds.write(FrontendSound::named("core_b_button"));
                        if fe.settings.screen > 1 {
                            info!("RETAIL_MENUS settings back to the main list from screen {}", fe.settings.screen);
                            fe.settings.screen = 1;
                            fe.settings.row = 0;
                            set_state(&mut ms.stage, options_id, 1, &mut fe)?;
                        } else {
                            ms.stack.request(Layer::Main, Request::Close { screen });
                        }
                    }
                    APT_LEFT | APT_RIGHT => {
                        consumed = true;
                        if let Some(id) = fe.settings.row_id(fe.settings.row) {
                            let left = key == APT_LEFT;
                            steps.push((id.to_owned(), if left { Direction::Left } else { Direction::Right }));
                            sounds.write(FrontendSound::named(if left { "core_nav_left" } else { "core_nav_right" }));
                        }
                    }
                    _ => {}
                }
            }
            if !consumed && backdrop.info(screen).input_to_movie {
                ms.stage.call_path(
                    "_global.ScreenManager.UpdateInput",
                    vec![Value::Text(movie_id.to_string()), Value::Text(key.to_string()), Value::Text(controller.to_string())],
                    &mut fe,
                )?;
            }
            flush_trick_guide(&mut ms.stage, trick_id, &mut fe)?;
            flush_challenge_map(&mut ms.stage, &mut ms.stack, map, map_id, &mut fe)?;
            flush_offline_profile(&mut ms.stage, profile_id, &mut fe)?;
        }
        // The stack's update (sub_82D0A560) and what it asks of the movies and the game.
        let effects = drop_reopen_and_close(ms.stack.update(&|i| backdrop.info(i)));
        // The VM's objects at every screen change: after a close the count returns to the same value, and the
        // levels list shows any screen still loaded.
        if !effects.is_empty() {
            info!("RETAIL_MENUS APT objects {} before {} screen change(s)", ms.stage.object_summary(), effects.len());
        }
        for effect in effects {
            match effect {
                Effect::Open(i) if Some(i) == pause => {
                    info!("RETAIL_MENUS open pause menu (FE {i})");
                    // Crossbar init (vt slot 3, sub_8261ACA8) releases the screen fade (back from the Trick Guide).
                    let params = ms.fade_params;
                    ms.fade.release(FADE_SCREEN, false, &params);
                    fe.state = CrossbarState::open(&fe.view);
                    open_screen(&mut ms.stage, CROSSBAR_PATH, CROSSBAR, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == options => {
                    // Settings init sub_8260BF58: screen index -1 -> 0 (the pause path opens on the main list).
                    info!("RETAIL_MENUS open Game Settings (FE {i})");
                    fe.settings.screen = 0;
                    fe.settings.row = 0;
                    open_screen(&mut ms.stage, OPTIONS_PATH, OPTIONS, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == trick => {
                    info!("RETAIL_MENUS open Trick Guide (FE {i})");
                    fe.trick_guide.reset();
                    // Its callbacks are ours from the load on (the movie lists its rows while loading).
                    fe.trick_guide.open = true;
                    open_screen(&mut ms.stage, TRICK_GUIDE_PATH, TRICK_GUIDE, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == trick => {
                    info!("RETAIL_MENUS close Trick Guide (FE {i})");
                    fe.trick_guide.open = false;
                    ms.trick_guide_fade_in = None;
                    close(&mut ms.stage, trick_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == map => {
                    // Init sub_826457E0 on the mode root (sub_8261BC30: `progression` in career and free skate).
                    // NOT RETAIL YET: OnlineChallengeMap (`online`) is not reached; it has no handler.
                    info!("RETAIL_MENUS open Challenge Map (FE {i})");
                    fe.challenge_map.open(fe.registry, fe.context, "progression");
                    open_screen(&mut ms.stage, CHALLENGE_MAP_PATH, CHALLENGE_MAP, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == map => {
                    info!("RETAIL_MENUS close Challenge Map (FE {i})");
                    fe.challenge_map.open = false;
                    close(&mut ms.stage, map_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == extras => {
                    info!("RETAIL_MENUS open Extras (FE {i})");
                    // A set from before the credits movie (version 12) keeps the Credits row greyed.
                    let credits_ready = credits.is_some() && movies.has_movie(CREDITS_MOVIE);
                    if let Some(row) = fe.extras.rules.rows.iter_mut().find(|r| r.id == "extras.credits").filter(|_| !credits_ready) {
                        row.greyed = true;
                    }
                    fe.extras.screen = skate_core::extras::Extras::open();
                    fe.extras.states.clear();
                    fe.extras.open = true;
                    open_screen(&mut ms.stage, EXTRAS_PATH, EXTRAS, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == extras => {
                    info!("RETAIL_MENUS close Extras (FE {i})");
                    fe.extras.open = false;
                    close(&mut ms.stage, ms.extras_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == spawn => {
                    // The living world owns the called skaters and the pool Call Skater lists from.
                    let callable = crate::spawn_skater_host::callable(&config.asset_root);
                    let companions = population.as_ref().map(|p| p.world.companions().iter().map(|c| skate_core::spawn_skater::Skater::Character(c.key.clone())).collect()).unwrap_or_default();
                    let names = career_profile.as_ref().map(|c| c.teammates.clone()).unwrap_or_default();
                    let teammates: Vec<(String, String)> = population.as_ref().and_then(|p| p.skaters.as_ref()).map(|s| {
                        names.iter().enumerate().filter_map(|(i, name)| {
                            let n = i as u32 + 1;
                            let c = s.characters.iter().find(|c| c.save_slot == Some(n) && s.filled_slots.contains(&n))?;
                            Some((c.key.clone(), name.clone()))
                        }).collect()
                    }).unwrap_or_default();
                    let groups = crate::spawn_skater_host::groups(characters.as_deref(), &callable, &teammates);
                    let s = &mut fe.spawn_skater;
                    s.screen = skate_core::spawn_skater::SpawnSkater::open(ms.spawn_skater_mode, &groups, companions);
                    info!("RETAIL_MENUS open SpawnSkater (FE {i}) mode {} rows {}", s.screen.mode.code(), s.screen.rows().len());
                    s.states.clear();
                    s.actions.clear();
                    s.open = true;
                    open_screen(&mut ms.stage, SPAWN_SKATER_PATH, SPAWN_SKATER, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == free => {
                    // Init copies the session's values into the screen (b564).
                    let values = living.as_ref().and_then(|l| l.free_play).unwrap_or_default();
                    info!("RETAIL_MENUS open Free Play options (FE {i}) {values:?}");
                    fe.free_skate.screen = skate_core::free_skate_options::FreeSkateOptions::open(values);
                    fe.free_skate.states.clear();
                    fe.free_skate.open = true;
                    open_screen(&mut ms.stage, FREE_SKATE_PATH, FREE_SKATE, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == free => {
                    info!("RETAIL_MENUS close Free Play options (FE {i})");
                    fe.free_skate.open = false;
                    close(&mut ms.stage, ms.free_skate_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == loader => {
                    info!("RETAIL_MENUS open loading screen (FE {i})");
                    fe.tips_loader.open = true;
                    fe.tips_loader.tips.open();
                    let retail = ms.retail_lines.get_or_insert_with(|| fe.tips_loader.lines.clone());
                    fe.tips_loader.lines = loading_mods.lines(retail);
                    fe.tips_loader.tips.timing = loading_mods.timing();
                    // The Stats panel's rows (offline fetch sub_82607840); none = no Stats panel.
                    let mut seed = fe.tips_loader.seed;
                    fe.tips_loader.stats = career.as_ref().map_or_else(Vec::new, |c| {
                        c.panel_rows(&ms.language, &crate::fe_language::NumberFormat::default(), imperial, &mut || crate::tips_loader_host::lcg(&mut seed))
                    });
                    fe.tips_loader.stats.extend(loading_mods.stat_rows());
                    fe.tips_loader.seed = seed;
                    ms.load_map = transition.loading().map(|e| e.label.clone());
                    loading_mods.opened(ms.load_map.as_deref());
                    ms.load_up = false;
                    ms.load_gone = false;
                    open_screen(&mut ms.stage, TIPS_LOADER_PATH, TIPS_LOADER, &mut fe)?;
                    load_tag(&mut ms.stage, true, &mut fe);
                }
                Effect::Close(i) if Some(i) == loader => {
                    info!("RETAIL_MENUS close loading screen (FE {i})");
                    fe.tips_loader.open = false;
                    ms.load_up = false;
                    ms.load_gone = true;
                    close(&mut ms.stage, ms.tips_loader_id, &mut fe)?;
                    load_tag(&mut ms.stage, false, &mut fe);
                    loading_mods.closed(ms.load_map.take().as_deref());
                }
                Effect::Close(i) if Some(i) == spawn => {
                    info!("RETAIL_MENUS close SpawnSkater (FE {i})");
                    fe.spawn_skater.open = false;
                    close(&mut ms.stage, ms.spawn_skater_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == viewer => {
                    info!("RETAIL_MENUS open text viewer (FE {i})");
                    open_screen(&mut ms.stage, TEXT_VIEWER_PATH, TEXT_VIEWER, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == viewer => {
                    info!("RETAIL_MENUS close text viewer (FE {i})");
                    close(&mut ms.stage, ms.viewer_id, &mut fe)?;
                    fe.tos.viewer = None;
                }
                // FE 25 (b489): the screen reads the lines file, then posts Rolling once it is parsed (below).
                Effect::Open(i) if Some(i) == music => {
                    let tracks = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
                        .ok()
                        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
                        .map(|t| t.ipod_tracks)
                        .unwrap_or_default();
                    let songs = crate::music_player_host::song_rows(&tracks[..tracks.len().min(music_link.songs as usize)], &ms.language);
                    info!("RETAIL_MENUS open Music Player (FE {i}): {} songs, {} in the playlist, {:?}", songs.len(), music_link.playlist.len(), music_link.mode);
                    fe.music_player.open_with(songs, music_link.playlist.clone(), music_link.mode, music_link.volume);
                    fe.music_player.playing = music_link.playing;
                    open_screen(&mut ms.stage, MUSIC_PLAYER_PATH, MUSIC_PLAYER, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == music => {
                    info!("RETAIL_MENUS close Music Player (FE {i})");
                    fe.music_player.open = false;
                    close(&mut ms.stage, ms.music_player_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == credits => {
                    let path = crate::retail_menu_movies::set_root(&config).join(CREDITS_FILE);
                    let retail = std::fs::read(&path).map(|b| skate_core::credits::parse(&skate_core::credits::decode(&b))).unwrap_or_else(|e| {
                        warn!("RETAIL_MENUS credits: {}: {e}", path.display());
                        Vec::new()
                    });
                    let mods = credits_mods.sections();
                    info!("RETAIL_MENUS open credits (FE {i}): {} retail lines, {} mod sections", retail.len(), mods.len());
                    fe.credits = crate::extras_host::CreditsHost { credits: Some(skate_core::credits::Credits::new(retail, &mods)), exit: false };
                    open_screen(&mut ms.stage, CREDITS_PATH, CREDITS, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == credits => {
                    info!("RETAIL_MENUS close credits (FE {i})");
                    close(&mut ms.stage, ms.credits_id, &mut fe)?;
                    fe.credits = Default::default();
                }
                // FE 46 (b305): no movie; ours shows the on-screen keyboard while it is on top.
                Effect::Open(i) if Some(i) == entry => {
                    if let Some(request) = ms.pending_entry.take() {
                        info!("RETAIL_MENUS open text entry (FE {i}) for {}", request.callback);
                        text_entry.title = crate::extras_host::text_of(fe.language, &request.title);
                        text_entry.keyboard = Some(skate_core::text_entry::Keyboard::open(request));
                    }
                    ms.stack.ready(i);
                }
                Effect::Close(i) if Some(i) == entry => {
                    info!("RETAIL_MENUS close text entry (FE {i})");
                    text_entry.keyboard = None;
                }
                Effect::Open(i) if Some(i) == profile => {
                    info!("RETAIL_MENUS open Skate Profile (FE {i})");
                    fe.offline_profile.open();
                    open_screen(&mut ms.stage, OFFLINE_PROFILE_PATH, OFFLINE_PROFILE, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == profile => {
                    info!("RETAIL_MENUS close Skate Profile (FE {i})");
                    fe.offline_profile.loaded = false;
                    fe.offline_profile.states.clear();
                    close(&mut ms.stage, profile_id, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == cal => {
                    info!("RETAIL_MENUS open Video Calibration (FE {i})");
                    ms.video_cal = skate_core::video_calibration::VideoCalibration::open();
                    open_screen(&mut ms.stage, VIDEO_CALIBRATION_PATH, VIDEO_CALIBRATION, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == cal => {
                    info!("RETAIL_MENUS close Video Calibration (FE {i})");
                    close(&mut ms.stage, cal_id, &mut fe)?;
                }
                // Screen vt+28 -> facade ShowScreen (0x825DA410): the AS id as "%d" and "true" / "false".
                Effect::Show(i, visible) if [pause, options, trick, cal, extras, profile].contains(&Some(i)) => {
                    let id = [(pause, crossbar_id), (options, options_id), (trick, trick_id), (cal, cal_id), (extras, ms.extras_id), (profile, profile_id)]
                        .into_iter()
                        .find(|(s, _)| *s == Some(i))
                        .map_or(0, |(_, id)| id);
                    ms.stage.call_path(
                        "_global.ScreenManager.ShowScreen",
                        vec![Value::Text(id.to_string()), Value::Text(if visible { "true" } else { "false" }.into())],
                        &mut fe,
                    )?;
                }
                Effect::Close(i) if Some(i) == pause => {
                    info!("RETAIL_MENUS close pause menu (FE {i})");
                    close(&mut ms.stage, crossbar_id, &mut fe)?
                }
                Effect::Close(i) if Some(i) == options => {
                    info!("RETAIL_MENUS close Game Settings (FE {i})");
                    // Begin close sub_8260C048: SetScreenState(2) outside the front end (FE state 2), then the outro.
                    set_state(&mut ms.stage, options_id, 2, &mut fe)?;
                    close(&mut ms.stage, options_id, &mut fe)?;
                    fe.settings.open = false;
                }
                Effect::Open(i) | Effect::Close(i) => warn!("RETAIL_MENUS: FE screen {i} has no template yet"),
                Effect::SuspendGameplay(on) => {
                    info!("RETAIL_MENUS gameplay {}", if on { "suspended" } else { "resumed" });
                    if let Some(overlay) = overlay.as_mut() {
                        overlay.fe_suspended = on;
                    }
                }
                // vt+44(hud, online): 0 both on, 1 both off, 2 online only (3 = keep is never emitted).
                Effect::Hud(mode) => {
                    use skate_core::fe_stack::HudMode;
                    *fe_hud = match mode {
                        HudMode::On | HudMode::Keep => crate::fe_backdrop::FeHud { hud: true, online: true },
                        HudMode::Off => crate::fe_backdrop::FeHud { hud: false, online: false },
                        HudMode::OnlineOnly => crate::fe_backdrop::FeHud { hud: false, online: true },
                    };
                }
                // vt+48 -> sub_825E4408: on a change, the chyron movie's _global.Chyron_SetVisible("true" / "false").
                // NOT RETAIL YET: retail plays Chyron_PlayOutro instead of hiding when the chyron is mid-show.
                Effect::Chyron(on) if on != ms.chyron => {
                    ms.chyron = on;
                    let defined = matches!(
                        ms.stage.vm.get(ms.stage.vm.global, "Chyron_SetVisible"),
                        crate::apt_vm::Value::Object(_)
                    );
                    if defined {
                        ms.stage.call_path(
                            "_global.Chyron_SetVisible",
                            vec![Value::Text(if on { "true" } else { "false" }.into())],
                            &mut fe,
                        )?;
                    }
                }
                Effect::Chyron(_) => {}
                // The blur follows the stack's top through FeBackdrop (mod overrides); show / param / focus have
                // no screen that uses them yet.
                Effect::Blur(_) | Effect::Show(..) | Effect::Param(..) | Effect::Focus(_) => {}
            }
        }
        // NOT RETAIL (guard): retail always returns to the pause menu, whose init releases the fade; with no screen
        // left (the pause menu was closed meanwhile) the fade would stay black, so release it.
        if ms.stack.top(Layer::Main).is_none() && ms.fade.held(FADE_SCREEN) {
            let params = ms.fade_params;
            ms.fade.release(FADE_SCREEN, false, &params);
        }
        // The fader's ticks (FE cadence): the Trick Guide's state-4 count, then actions due at black.
        let params = ms.fade_params;
        let before = ms.fade.alpha;
        let period = 1.0 / params.tick_hz.max(1.0);
        let ticks = ((time.delta_secs() + ms.fade_carry) / period).floor() as u32;
        ms.load_after = if ms.load.active() && !transition.busy() { ms.load_after + time.delta_secs() } else { 0.0 };
        ms.fade_carry = (time.delta_secs() + ms.fade_carry - ticks as f32 * period).min(period * 30.0);
        for _ in 0..ticks.min(30) {
            if let Some(n) = ms.trick_guide_fade_in {
                ms.trick_guide_fade_in = n.checked_sub(1).filter(|n| *n > 0);
                if ms.trick_guide_fade_in.is_none() {
                    ms.fade.release(FADE_SCREEN, false, &params);
                }
            }
            for action in ms.fade.tick() {
                if let Some(skater) = run_fade_action(&mut ms.stack, action) {
                    ms.swap_due = Some(skater);
                }
            }
            // The loading screen around every map load (game state 0x82707508; doc 31 "Loading screen"); the PC overlay
            // closes when a load starts (`graphics_menu`), so its loads get the screen too.
            if let Some(l) = loader {
                use skate_core::tips_loader::LoadStep;
                // A mod's `sdk.loading.hold` keeps a running load's screen up a while longer (tips_loader_host).
                let loading = (transition.busy() && (ms.load.active() || !overlay.as_ref().is_some_and(|o| o.open)))
                    || (ms.load.active() && loading_mods.holding(ms.load_after));
                match ms.load.update(loading, ms.fade.alpha >= 1.0, ms.load_up, ms.load_gone) {
                    Some(LoadStep::HoldBlack) => {
                        ms.fade.hold(FADE_LOAD, None, false, &params);
                    }
                    Some(LoadStep::Push) => ms.stack.request(Layer::Main, Request::push(l)),
                    Some(LoadStep::ReleaseInstant) => ms.fade.release(FADE_LOAD, true, &params),
                    Some(LoadStep::Close) => ms.stack.request(Layer::Main, Request::Close { screen: l }),
                    Some(LoadStep::Release) => {
                        ms.load_gone = false;
                        ms.fade.release(FADE_LOAD, false, &params);
                    }
                    None => {}
                }
            }
        }
        let now = crate::map_transition::LoadingCover { available: loader.is_some(), active: ms.load.active(), covered: ms.load.covered() };
        if *cover != now {
            *cover = now;
        }
        // 0x89 at black: unload and load the player (callback 82706AE8); when the load is done (or failed: the
        // library keeps a character), exit (827064D8) closes FE 62 and releases the fader.
        if let Some(skater) = ms.swap_due.take() {
            use skate_core::spawn_skater::{Action, Skater};
            spawner_out.write(crate::spawn_skater_host::SpawnSkaterRequest(Action::SetPlayer(skater.clone())));
            if let Some(c) = characters.as_mut() {
                match &skater {
                    Skater::Own => c.request_stock(),
                    Skater::Character(id) => {
                        if !c.request_character(id) {
                            warn!("RETAIL_MENUS spawn skater: no character {id}");
                        }
                    }
                }
            }
            ms.swap_loading = true;
        }
        if ms.swap_loading && !characters.as_ref().is_some_and(|c| c.swapping()) {
            ms.swap_loading = false;
            info!("RETAIL_MENUS spawn skater: swap done, closing FE 62");
            for s in [spawn, pause].into_iter().flatten() {
                ms.stack.request(Layer::Main, Request::Close { screen: s });
            }
            let params = ms.fade_params;
            ms.fade.release(FADE_SWAP, false, &params);
        }
        if before != ms.fade.alpha && (ms.fade.alpha == 0.0 || ms.fade.alpha == 1.0) {
            info!("RETAIL_MENUS fade {}", if ms.fade.alpha == 1.0 { "black" } else { "clear" });
        }
        ms.stage.update(time.delta_secs() * 1000.0, &movies.movies, &mut fe)?;
        if let Some(show) = ms.save_tag.step(crate::save_tag::saving(), time.delta_secs()) {
            save_tag(&mut ms.stage, show, &mut fe);
            // Hiding the save tag re-shows the load tag while a load is still up (825DF4F8).
            if !show && fe.tips_loader.open {
                load_tag(&mut ms.stage, true, &mut fe);
            }
        }
        // Settings loaded (vt+108 sub_8260C010): SetScreenState(1) builds the rows; intro complete = ready.
        if let Some(o) = options.filter(|o| ms.stack.top(Layer::Main) == Some(*o)) {
            let loaded = format!("Game.OnScreenLoaded({options_id})");
            if take_call(&mut fe, &loaded) {
                fe.settings.open = true;
                set_state(&mut ms.stage, options_id, 1, &mut fe)?;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({options_id})")) {
                ms.stack.ready(o);
            }
        }
        // Mods' popups (sdk.menus.popup): a new one replaces the mod's popup with the same key; a stopped mod's
        // leave without an event.
        for owner in std::mem::take(&mut popup_mods.cleared) {
            let prefix = owner.map_or_else(|| "mod:".to_owned(), |o| format!("mod:{o}:"));
            if !ms.popups.clear_owned(&prefix).is_empty() {
                ms.popup_dirty = true;
            }
        }
        for request in std::mem::take(&mut popup_mods.engine_requests) {
            ms.popups.remove_callback(&request.callback);
            let callback = request.callback.clone();
            let id = ms.popups.push(request);
            info!("RETAIL_MENUS popup {id} ({callback})");
            ms.popup_dirty = true;
        }
        for (owner, key, request) in std::mem::take(&mut popup_mods.requests) {
            let callback = crate::popup_host::mod_callback(&owner, &key);
            ms.popup_dirty |= ms.popups.remove_callback(&callback);
            if let Some(request) = request {
                let id = ms.popups.push(request);
                info!("RETAIL_MENUS popup {id} from mod {owner} ({key})");
                ms.popup_dirty = true;
            }
        }
        // Popup choices (host vt+64 0x825EF698): the callback by id, then the list shows its next entry.
        for choice in std::mem::take(&mut fe.popup.choices) {
            let Some(event) = ms.popups.choose(choice) else { continue };
            info!("RETAIL_MENUS popup {} choice {} ({})", event.id, event.choice, event.callback);
            ms.popup_dirty = true;
            if let Some((owner, key)) = event.callback.strip_prefix("mod:").and_then(|c| c.split_once(':')) {
                mod_events.push((owner.to_owned(), serde_json::json!({"name": "menu_popup", "key": key, "choice": event.choice})));
            } else if event.callback.starts_with("engine:") {
                popup_mods.engine_choices.push((event.callback.clone(), event.choice));
            } else if event.callback == VIDEO_CALIBRATION_POPUP {
                if let Some(c) = cal.filter(|c| ms.stack.active(*c)) {
                    let action = ms.video_cal.popup_result(event.choice as u32, &ms.video_cal_rules);
                    run_calibration(&mut ms.stage, &mut ms.stack, &mut ms.popups, &mut ms.popup_dirty, &mut ms.video_cal, &ms.video_cal_rules, cal_id, action, c, &mut fe)?;
                }
            }
        }
        if std::mem::take(&mut ms.popup_dirty) {
            show_popup(&mut ms.stage, &ms.popups, &ms.popup_ids, &mut fe, &mut sounds)?;
        }
        // The sign-up prompt (teleporters.rs state 2, challenges.rs state 1) with the title when it appears or
        // changes, 0 when it goes.
        if let Some(signup) = ms.signup_id {
            let wanted = popup_mods.signup_title.clone().map(|t| (popup_mods.signup_state, t));
            if wanted != ms.signup_shown {
                // The prompt component's show (82665CD0, research b854) plays this each time it appears.
                if ms.signup_shown.is_none() && wanted.is_some() {
                    sounds.write(FrontendSound::named(SIGNUP_SHOW_SOUND));
                }
                fe.signup_title = wanted.as_ref().map(|(_, t)| t.clone());
                let state = wanted.as_ref().map_or(0, |(s, _)| *s);
                set_state(&mut ms.stage, signup, state, &mut fe)?;
                info!("RETAIL_MENUS signup state {state} title={:?}", fe.signup_title);
                ms.signup_shown = wanted;
            }
        }
        // Shown after its intro, or on load, or after SHOW_FALLBACK_FRAMES updates: the hudComponents dialogs report
        // neither (laptop run 2026-11-07: logged open, never drawn).
        if let Some(id) = ms.end_open.and_then(|m| ms.end_ids[m]) {
            ms.end_frames += 1;
            let loaded = take_call(&mut fe, &format!("Game.OnScreenLoaded({id})"));
            let intro = take_call(&mut fe, &format!("Game.OnScreenIntroComplete({id})"));
            if !ms.end_shown && (loaded || intro || ms.end_frames > SHOW_FALLBACK_FRAMES) {
                info!("RETAIL_MENUS show end dialog {id} (loaded {loaded}, intro {intro}, frames {})", ms.end_frames);
                ms.end_shown = true;
                ms.stage.call_path("_global.ScreenManager.ShowScreen", vec![Value::Text(id.to_string()), Value::Text("true".into())], &mut fe)?;
            }
        }
        if let Some(id) = ms.poster_id.filter(|_| ms.poster_open) {
            ms.poster_frames += 1;
            let loaded = take_call(&mut fe, &format!("Game.OnScreenLoaded({id})"));
            let intro = take_call(&mut fe, &format!("Game.OnScreenIntroComplete({id})"));
            if !ms.poster_shown && (loaded || intro || ms.poster_frames > SHOW_FALLBACK_FRAMES) {
                info!("RETAIL_MENUS show poster {id} (loaded {loaded}, intro {intro}, frames {})", ms.poster_frames);
                ms.poster_shown = true;
                ms.stage.call_path("_global.ScreenManager.ShowScreen", vec![Value::Text(id.to_string()), Value::Text("true".into())], &mut fe)?;
                // b935: a single challenge posts the poster state (a lot row would post 10, the lot list).
                set_state(&mut ms.stage, id, skate_core::challenges::poster::STATE_SIGN_UP, &mut fe)?;
            }
        }
        if let Some(id) = ms.reward_open.and_then(|(s, _)| ms.reward_ids[s]) {
            ms.reward_frames += 1;
            let loaded = take_call(&mut fe, &format!("Game.OnScreenLoaded({id})"));
            let intro = take_call(&mut fe, &format!("Game.OnScreenIntroComplete({id})"));
            if !ms.reward_shown && (loaded || intro || ms.reward_frames > SHOW_FALLBACK_FRAMES) {
                info!("RETAIL_MENUS show reward screen {id} (loaded {loaded}, intro {intro}, frames {})", ms.reward_frames);
                ms.reward_shown = true;
                ms.stage.call_path("_global.ScreenManager.ShowScreen", vec![Value::Text(id.to_string()), Value::Text("true".into())], &mut fe)?;
            }
        }
        // The instant replay screen after this frame's movie update: shown once its intro completed, then told what
        // changed in the replay.
        if let Some(replay_id) = ms.replay_id.filter(|_| fe.replay_menu.open) {
            take_call(&mut fe, &format!("Game.OnScreenLoaded({replay_id})"));
            if !ms.replay_shown && take_call(&mut fe, &format!("Game.OnScreenIntroComplete({replay_id})")) {
                ms.replay_shown = true;
                ms.stage.call_path(
                    "_global.ScreenManager.ShowScreen",
                    vec![Value::Text(replay_id.to_string()), Value::Text("true".into())],
                    &mut fe,
                )?;
            }
            if ms.replay_shown {
                fe.replay_menu.post_changes();
                for state in std::mem::take(&mut fe.replay_menu.states) {
                    set_state(&mut ms.stage, replay_id, state, &mut fe)?;
                }
            }
            // The help (FE 13) over the replay screen: opened and closed with the feed; each page turn the replay
            // asked for is a SetState(0) (`NextHelpPage`) once the help is shown.
            if let Some(help_id) = ms.replay_help_id.filter(|_| ms.replay_shown) {
                let host = &mut fe.replay_menu;
                match (host.feed.help, host.help_open) {
                    (true, false) => {
                        info!("RETAIL_MENUS open {} (FE 13)", REPLAY_SCREENS[ms.replay_screen].help);
                        host.help_open = true;
                        host.help_shown = false;
                        host.help_turns_posted = host.feed.help_turns;
                        open_screen(&mut ms.stage, REPLAY_SCREENS[ms.replay_screen].help_path, REPLAY_SCREENS[ms.replay_screen].help, &mut fe)?;
                    }
                    (false, true) => {
                        info!("RETAIL_MENUS close {}", REPLAY_SCREENS[ms.replay_screen].help);
                        host.help_open = false;
                        close(&mut ms.stage, help_id, &mut fe)?;
                    }
                    _ => {}
                }
                if fe.replay_menu.help_open {
                    take_call(&mut fe, &format!("Game.OnScreenLoaded({help_id})"));
                    if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({help_id})")) {
                        fe.replay_menu.help_shown = true;
                    }
                    while fe.replay_menu.help_shown && fe.replay_menu.help_turns_posted < fe.replay_menu.feed.help_turns {
                        fe.replay_menu.help_turns_posted += 1;
                        set_state(&mut ms.stage, help_id, crate::replay_menu_host::HELP_NEXT_PAGE, &mut fe)?;
                    }
                }
            }
        }
        // Video Calibration loaded (vt+108 0x8260E888): the first page; intro complete = ready.
        if let Some(c) = cal.filter(|c| ms.stack.top(Layer::Main) == Some(*c)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({cal_id})")) {
                let rules = ms.video_cal_rules.clone();
                let action = ms.video_cal.advance(&rules);
                run_calibration(&mut ms.stage, &mut ms.stack, &mut ms.popups, &mut ms.popup_dirty, &mut ms.video_cal, &ms.video_cal_rules, cal_id, action, c, &mut fe)?;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({cal_id})")) {
                ms.stack.ready(c);
            }
        }
        // Trick Guide: the movie builds its list itself on load; intro complete = ready. The picker's callbacks
        // during the update queue screen states.
        if let Some(t) = trick.filter(|t| ms.stack.top(Layer::Main) == Some(*t)) {
            take_call(&mut fe, &format!("Game.OnScreenLoaded({trick_id})"));
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({trick_id})")) {
                ms.stack.ready(t);
                // State 4: count updates, then release the fade. NOT RETAIL YET: what enters state 4 is not found;
                // the count starts at the intro's end.
                ms.trick_guide_fade_in = Some(TRICK_GUIDE_FADE_IN_TICKS);
            }
        }
        flush_trick_guide(&mut ms.stage, trick_id, &mut fe)?;
        // Challenge Map: intro complete = ready; its open states and the picker's callbacks go out after the update.
        if let Some(m) = map.filter(|m| ms.stack.top(Layer::Main) == Some(*m)) {
            take_call(&mut fe, &format!("Game.OnScreenLoaded({map_id})"));
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({map_id})")) {
                ms.stack.ready(m);
            }
        }
        flush_challenge_map(&mut ms.stage, &mut ms.stack, map, map_id, &mut fe)?;
        // Skate Profile: init's page state goes out once the movie loaded; intro complete = ready.
        if let Some(p) = profile.filter(|p| ms.stack.top(Layer::Main) == Some(*p)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({profile_id})")) {
                fe.offline_profile.loaded = true;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({profile_id})")) {
                ms.stack.ready(p);
            }
        }
        flush_offline_profile(&mut ms.stage, profile_id, &mut fe)?;
        // Extras: loaded -> SetState(10) builds the list (NOT RETAIL YET: who posts retail's first 10 is not found, b320);
        // intro complete = ready. Then the rows the player picked.
        if let Some(x) = extras.filter(|x| ms.stack.top(Layer::Main) == Some(*x)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.extras_id)) {
                fe.extras.states.push(fe.extras.rules.list_state);
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.extras_id)) {
                ms.stack.ready(x);
            }
        }
        // Text viewer: loaded hook 0x8263A2A0 posts SetState(1) (the movie splits nothing itself: InitWords); intro
        // complete = ready.
        if let Some(v) = viewer.filter(|v| ms.stack.top(Layer::Main) == Some(*v)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.viewer_id)) {
                set_state(&mut ms.stage, ms.viewer_id, 1, &mut fe)?;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.viewer_id)) {
                ms.stack.ready(v);
            }
        }
        // Credits (82608CE8 per update): once the movie is loaded and the lines are parsed, SetState(1) Rolling, once;
        // ExitCredits (the last line scrolled off) pops the screen on the next update.
        if let Some(c) = credits.filter(|c| ms.stack.top(Layer::Main) == Some(*c)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.credits_id)) && fe.credits.credits.is_some() {
                set_state(&mut ms.stage, ms.credits_id, skate_core::credits::STATE_ROLLING, &mut fe)?;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.credits_id)) {
                ms.stack.ready(c);
            }
            if std::mem::take(&mut fe.credits.exit) {
                info!("RETAIL_MENUS credits ended");
                ms.stack.request(Layer::Main, Request::Close { screen: c });
            }
        }
        for state in std::mem::take(&mut fe.extras.states) {
            set_state(&mut ms.stage, ms.extras_id, state, &mut fe)?;
        }
        // SpawnSkater: loaded -> SetState(State_UpdateList) builds the list (NOT RETAIL YET: who posts retail's first one
        // is not read); intro complete = ready; then the picks and keys (b543 / b545).
        if let Some(s) = spawn.filter(|s| ms.stack.top(Layer::Main) == Some(*s)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.spawn_skater_id)) {
                fe.spawn_skater.states.push(fe.spawn_skater.rules.state_list);
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.spawn_skater_id)) {
                ms.stack.ready(s);
            }
        }
        // The loading screen: intro complete = up (the black goes at once); then a panel every 9.0 s (sub_82605CC0).
        if let Some(l) = loader.filter(|l| ms.stack.top(Layer::Main) == Some(*l)) {
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.tips_loader_id)) {
                ms.stack.ready(l);
                ms.load_up = true;
            }
            if ms.load_up {
                let t = &mut fe.tips_loader;
                let available = t.available();
                let mut seed = t.seed;
                let state = t.tips.update(time.delta_secs(), available, &mut || crate::tips_loader_host::lcg(&mut seed));
                t.seed = seed;
                if let Some(state) = state {
                    set_state(&mut ms.stage, ms.tips_loader_id, state.code() as i32, &mut fe)?;
                }
            }
        }
        // Free Play options: loaded -> every row refreshes and the selection shows (NOT RETAIL YET: the poster of the
        // first states is not read); intro complete = ready; then the key states.
        if let Some(f) = free.filter(|f| ms.stack.top(Layer::Main) == Some(*f)) {
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.free_skate_id)) {
                let first = [0, 1, 2, fe.free_skate.rules.state_selection];
                fe.free_skate.states.extend(first);
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.free_skate_id)) {
                ms.stack.ready(f);
            }
        }
        for state in std::mem::take(&mut fe.free_skate.states) {
            set_state(&mut ms.stage, ms.free_skate_id, state, &mut fe)?;
        }
        if let Some(m) = music.filter(|m| ms.stack.top(Layer::Main) == Some(*m)) {
            // Loaded -> the tab's state (0 Interactive, 1 Playlist, then 63 RefreshPlayList). NOT RETAIL YET: who
            // posts retail's first state is not found (init 0x8260B400 only opens the movie).
            if take_call(&mut fe, &format!("Game.OnScreenLoaded({})", ms.music_player_id)) {
                let tab = fe.music_player.tab;
                fe.music_player.states.push(tab);
                if tab == 1 {
                    fe.music_player.states.push(0x3F);
                }
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({})", ms.music_player_id)) {
                ms.stack.ready(m);
            }
            fe.music_player.playing = music_link.playing;
        }
        for sound in std::mem::take(&mut fe.music_player.sounds) {
            sounds.write(FrontendSound { key: sound });
        }
        for state in std::mem::take(&mut fe.music_player.states) {
            set_state(&mut ms.stage, ms.music_player_id, state, &mut fe)?;
        }
        for action in std::mem::take(&mut fe.music_player.actions) {
            use crate::music_player_host::MusicPlayerAction;
            info!("RETAIL_MENUS music player {action:?}");
            match action {
                MusicPlayerAction::Close { save } => {
                    if save {
                        music_link.playlist = fe.music_player.playlist.clone();
                        music_link.mode = fe.music_player.mode;
                        music_link.save = true;
                    }
                    if let Some(m) = music {
                        ms.stack.request(Layer::Main, Request::Close { screen: m });
                    }
                }
                MusicPlayerAction::Mode(mode) => {
                    music_link.mode = mode;
                    music_link.actions.push(action);
                }
                other => music_link.actions.push(other),
            }
        }
        for sound in std::mem::take(&mut fe.spawn_skater.sounds) {
            sounds.write(FrontendSound::named(&sound));
        }
        for state in std::mem::take(&mut fe.spawn_skater.states) {
            set_state(&mut ms.stage, ms.spawn_skater_id, state, &mut fe)?;
        }
        for action in std::mem::take(&mut fe.spawn_skater.actions) {
            use skate_core::spawn_skater::Action;
            info!("RETAIL_MENUS spawn skater {action:?}");
            match action {
                Action::Close => {
                    if let Some(s) = spawn {
                        ms.stack.request(Layer::Main, Request::Close { screen: s });
                    }
                }
                Action::LimitReached => {
                    let [description, title, button] = fe.spawn_skater.rules.limit_popup.clone();
                    ms.popups.push(skate_core::fe_popup::PopupRequest {
                        title,
                        description,
                        options: vec![button],
                        callback: SPAWN_SKATER_POPUP.into(),
                        ..Default::default()
                    });
                    ms.popup_dirty = true;
                }
                // FlowMessage 0x89 (82705E88, state 82706938, b562): hold the fader (bit 7); at black the callback swaps
                // the skater (below). NOT RETAIL YET (inferred): the pause menu closes with FE 62 (Free Play resumes).
                Action::SetPlayer(skater) => {
                    let params = ms.fade_params;
                    if let Some(FadeAction::SetPlayer(now)) = ms.fade.hold(FADE_SWAP, Some(FadeAction::SetPlayer(skater)), false, &params) {
                        ms.swap_due = Some(now);
                    }
                }
                // Flow 0x8A: the Free Play options FE 34 over FE 62.
                Action::Options => match free {
                    Some(f) => ms.stack.request(Layer::Main, Request::push(f)),
                    None => warn!("RETAIL_MENUS: no Free Play options screen (FE 34) in the tables"),
                },
                // Called / dismissed companions go to the living world; the preview is not ported yet.
                other => {
                    spawner_out.write(crate::spawn_skater_host::SpawnSkaterRequest(other));
                }
            }
        }
        for action in std::mem::take(&mut fe.extras.selected) {
            use skate_core::extras::ExtrasAction;
            info!("RETAIL_MENUS extras select {action:?}");
            match action {
                // Row 0 (0x826558E8): text entry for a cheat code, titled with the row's label; push FE 46.
                ExtrasAction::CheatEntry => {
                    if let Some(e) = entry {
                        let label = fe.extras.rules.rows.first().map_or("ID_EXTRA_CHEAT_CODE".to_owned(), |r| r.label.clone());
                        ms.pending_entry = Some(skate_core::text_entry::TextEntryRequest::cheat_code(&label));
                        ms.stack.request(Layer::Main, Request::push(e));
                    }
                }
                // Rows 1-3 (ours: unlock directly): the redeem flag and the grants go out as events for the unlock store
                // (none yet); the popup (ours) says it worked.
                ExtrasAction::Grant { redeem, grants } => {
                    let source = fe.extras.rules.rows.iter().find(|r| matches!(&r.action, ExtrasAction::Grant { redeem: r2, .. } if *r2 == redeem)).map_or(String::new(), |r| r.id.clone());
                    for unlock in std::iter::once(redeem).chain(grants) {
                        info!("RETAIL_MENUS unlock {} {} from {source}", unlock.kind, unlock.name);
                        unlocks_out.write(crate::extras_host::UnlockGranted { unlock, source: source.clone() });
                    }
                    ms.popups.push(skate_core::fe_popup::PopupRequest {
                        title: "ID_VALID_CODE_TITLE".into(),
                        description: "ID_VALID_CODE_DESC".into(),
                        options: vec!["ID_COMMON_OK".into()],
                        callback: CHEAT_POPUP.into(),
                        ..Default::default()
                    });
                    ms.popup_dirty = true;
                }
                // Row 5: the text viewer FE 3 with our licenses (retail: the web browser's license file).
                ExtrasAction::TextViewer { title, body } => {
                    if let Some(v) = viewer {
                        let text = match body.as_str() {
                            "licenses" => crate::extras_host::LICENSES.to_owned(),
                            other => other.strip_prefix('#').unwrap_or(other).to_owned(),
                        };
                        fe.tos.viewer = Some(skate_core::text_viewer::TextViewer::open(&title, &text, &Default::default()));
                        ms.stack.request(Layer::Main, Request::push(v));
                    }
                }
                // Row 4: the credits FE 25 (b489).
                ExtrasAction::PushScreen(name) if name == "credits" && credits.is_some() => {
                    ms.stack.request(Layer::Main, Request::push(credits.unwrap()));
                }
                // Greyed by default until their screens are ported (doc 31).
                other => warn!("RETAIL_MENUS extras: {other:?} has no handler yet"),
            }
        }
        // The keyboard's result (FE 46 OK path 0x82639890 / cancel 0x82639B00).
        match std::mem::replace(&mut entry_outcome, skate_core::text_entry::KeyboardOutcome::None) {
            skate_core::text_entry::KeyboardOutcome::Done(text) => {
                let allowed = fe.language.get("ALL_VALID_ALPHANUMERIC_CHARACTERS").cloned().unwrap_or_default();
                let request = text_entry.keyboard.as_ref().map(|k| k.request.clone());
                use skate_core::text_entry::EntryResult;
                match request.as_ref().map(|r| (r, skate_core::text_entry::finish(r, &text, &allowed))) {
                    Some((r, EntryResult::Accepted(text))) => {
                        if let Some(e) = entry {
                            ms.stack.request(Layer::Main, Request::Close { screen: e });
                        }
                        if r.callback == skate_core::text_entry::CHEAT_CALLBACK {
                            // The matcher (b304) on the current flags: entering a code toggles what is in effect.
                            let mut state = skate_core::cheats::CheatState::default();
                            if living.as_ref().is_some_and(|l| l.zombie) {
                                state.on.insert("zombie".into());
                            }
                            let (outcome, dialog) = skate_core::cheats::enter(&text, &ms.cheats, &fe.context.mode, &mut state);
                            info!("RETAIL_MENUS cheat code entered: {outcome:?}");
                            if let skate_core::cheats::CheatOutcome::Toggled { id, on } = &outcome {
                                cheats_out.write(crate::extras_host::CheatChanged { id: id.clone(), on: *on });
                            }
                            ms.popups.push(skate_core::fe_popup::PopupRequest {
                                title: dialog.title,
                                description: dialog.text,
                                options: vec![dialog.button],
                                callback: CHEAT_POPUP.into(),
                                ..Default::default()
                            });
                            ms.popup_dirty = true;
                        }
                    }
                    // Characters outside the set: retail's warning popup (record 0x107), the keyboard stays.
                    Some((_, EntryResult::Invalid)) => {
                        use skate_core::text_entry::{INVALID_BUTTON, INVALID_TEXT, INVALID_TITLE};
                        ms.popups.push(skate_core::fe_popup::PopupRequest {
                            title: INVALID_TITLE.into(),
                            description: INVALID_TEXT.into(),
                            options: vec![INVALID_BUTTON.into()],
                            callback: CHEAT_POPUP.into(),
                            ..Default::default()
                        });
                        ms.popup_dirty = true;
                    }
                    Some((_, EntryResult::Reopen)) | None => {}
                }
            }
            skate_core::text_entry::KeyboardOutcome::Cancel => {
                if let Some(e) = entry {
                    ms.stack.request(Layer::Main, Request::Close { screen: e });
                }
            }
            skate_core::text_entry::KeyboardOutcome::None => {}
        }
        let hidden = ms.popups.shown().is_some();
        if text_entry.hidden != hidden {
            text_entry.hidden = hidden;
        }
        // Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) selects the mode menu (8261B9E8): when the
        // mode changed it resets level 1, row 0, tab 0 and walks to the first enabled row, then it always posts
        // SetState(InitialiseMenu) through the facade (+56 vt+20). The screen is ready (state 3) from here.
        if let Some(p) = pause.filter(|p| ms.stack.top(Layer::Main) == Some(*p)) {
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({crossbar_id})")) {
                ms.stack.ready(p);
                fe.state = CrossbarState::open(&fe.view);
                set_state(&mut ms.stage, crossbar_id, ms.rules.state_initialise_menu, &mut fe)?;
            }
        }
        Ok(())
    })();
    // `SKATE3_RETAIL_MENUS_TRACE=1`: log the host calls the movies made that nothing answered (diagnostics).
    if std::env::var_os("SKATE3_RETAIL_MENUS_TRACE").is_some() && !fe.calls.is_empty() {
        info!("RETAIL_MENUS calls: {}", fe.calls.join(" | "));
    }
    // Audio.PlaySound(FE_SOUND_X) from the movies: the value is the fe record name (RegisterAudioEnum).
    for call in fe.calls.iter().filter_map(|c| c.strip_prefix("Audio.PlaySound(")) {
        let name = call.trim_end_matches(')').split(',').next().unwrap_or("");
        if !name.is_empty() && name != "undefined" {
            sounds.write(FrontendSound::named(name));
        }
    }
    ms.crossbar = fe.state;
    ms.settings = fe.settings;
    ms.trick_guide = std::mem::take(&mut fe.trick_guide);
    ms.replay_menu = std::mem::take(&mut fe.replay_menu);
    if let Some(look) = look_screen.as_mut() {
        let channels = ms.replay_shown && REPLAY_SCREENS.get(ms.replay_screen).is_some_and(|r| r.channels);
        look.set_if_neq(crate::replay::ReplayLookScreen(channels));
    }
    if let Some(ui) = replay_ui.as_mut() {
        let loading = !ms.replay_shown && ms.replay_id.is_some() && ms.replay_menu.open;
        ui.set_if_neq(crate::replay::RetailReplayUi(ms.replay_shown, loading));
    }
    fe.challenge_map.markers = if fe.challenge_map.open { fe.challenge_map.state.markers(fe.registry, fe.context) } else { Vec::new() };
    ms.challenge_map = std::mem::take(&mut fe.challenge_map);
    ms.offline_profile = std::mem::take(&mut fe.offline_profile);
    ms.reward_summary = std::mem::take(&mut fe.reward_summary);
    // The movie's SetCICMenuOptionIndex calls this frame stay with the open poster.
    if let (Some(p), Some(host)) = (poster_screen.as_mut(), fe.poster.take()) {
        if p.poster.is_some() {
            p.poster = Some(host.poster);
        }
    }
    ms.extras = std::mem::take(&mut fe.extras);
    ms.tos = std::mem::take(&mut fe.tos);
    ms.credits = std::mem::take(&mut fe.credits);
    ms.spawn_skater = std::mem::take(&mut fe.spawn_skater);
    ms.free_skate = std::mem::take(&mut fe.free_skate);
    ms.music_player = std::mem::take(&mut fe.music_player);
    ms.tips_loader = std::mem::take(&mut fe.tips_loader);
    ms.image_slots = std::mem::take(&mut fe.image_slots);
    ms.popup = std::mem::take(&mut fe.popup);
    // A Challenge Map leaf the player picked goes through the registry: the engine's teleport handler queues the
    // teleport and closes the map and the pause menu (retail fades out and closes the front end, sub_826494C8);
    // a mod's handler gets its menu_select event; a greyed one never reaches here.
    if let Some(entry) = ms.challenge_map.selected.take() {
        let outcome = menus.act(MenuAction::Select { entry: entry.clone() });
        info!("RETAIL_MENUS challenge map select {}: {outcome:?}", entry.hook_id());
        if let Outcome::Forward(Owner::Engine(name), _) = &outcome {
            if let Some(key) = name.strip_prefix(crate::retail_menus::TELEPORT) {
                travel.requested.push(key.to_owned());
                for screen in [map, pause].into_iter().flatten() {
                    ms.stack.request(Layer::Main, Request::Close { screen });
                }
            }
        }
    }
    // The 3D demo behind FE 14 (trick_guide_scene.rs): the screen on top, its highlighted clip and camera track
    // (S+204, S+200).
    demo.set_if_neq(crate::trick_guide_scene::TrickGuideDemo {
        open: ms.trick_guide.open,
        animation: ms.trick_guide.state.animation.clone(),
        camera_track: ms.trick_guide.state.camera_track,
    });
    // FE 62's preview (marquee_preview.rs): the screen on top in a mode that previews (not Call Skater, b575).
    marquee.set_if_neq(crate::marquee_preview::MarqueeView { open: preview_open });
    for (owner, event) in mod_events {
        menus.send_event(&owner, event);
    }
    let settings_selected = std::mem::take(&mut ms.settings.selected);
    if result.is_ok() {
        let mut refresh: Option<i32> = None;
        if let Some(item) = selected {
            // Retail runs the item (its FE screen); ours goes through the registry: an engine handler pushes a
            // ported FE screen or opens an overlay section, a mod's gets its menu_select event.
            let outcome = menus.act(MenuAction::Select { entry: EntryRef::item(&item) });
            info!("RETAIL_MENUS select {item}: {outcome:?}");
            if let Outcome::Forward(Owner::Engine(name), _) = &outcome {
                if let Some(id) = name.strip_prefix(crate::retail_menus::SCREEN) {
                    match backdrop.index(id) {
                        // The Trick Guide opens when the screen is black (crossbar case 0x8261BF78 holds the fade,
                        // callback 0x8261CDD8 pushes FE 14). NOT RETAIL YET: Game Settings opens without the fade
                        // (its case is not checked).
                        Some(screen) if Some(screen) == trick => {
                            let params = ms.fade_params;
                            if let Some(action) = ms.fade.hold(FADE_SCREEN, Some(FadeAction::Push(screen)), false, &params) {
                                run_fade_action(&mut ms.stack, action);
                            }
                        }
                        Some(screen) => {
                            // The row sets FE 62 +0x40 before the push (b307: 200 Free Play, 100 Call Skater).
                            if Some(screen) == spawn {
                                if let Some((_, mode)) = ms.spawn_skater.rules.items.iter().find(|(i, _)| *i == item) {
                                    ms.spawn_skater_mode = *mode;
                                }
                            }
                            ms.stack.request(Layer::Main, Request::push(screen))
                        }
                        None => warn!("RETAIL_MENUS: no FE screen {id}"),
                    }
                } else if let (Some(section), Some(overlay)) =
                    (name.strip_prefix(crate::retail_menus::OVERLAY), overlay.as_mut())
                {
                    if let Some(p) = pause {
                        ms.stack.request(Layer::Main, Request::Close { screen: p });
                    }
                    overlay.open_section(section);
                }
            }
        }
        // Settings OnMenuSelect (sub_8260D290): a link row opens its screen (+68) and rebuilds (SetState(1)); other
        // rows go to their handler (engine or mod).
        for row in settings_selected {
            let Some(id) = ms.settings.row_id(row).map(str::to_owned) else { continue };
            sounds.write(FrontendSound::named("core_a_button"));
            match menus.act(MenuAction::Select { entry: EntryRef::setting(&id) }) {
                Outcome::OpenScreen(screen) => {
                    ms.settings.screen = screen;
                    ms.settings.row = 0;
                    refresh = Some(1);
                }
                // A row whose engine handler is a ported FE screen (row 25 Video Calibration: sub_82D0AEF0 pushes FE 51
                // with param 0, keep-open 1, keep-visible 0, so Game Settings stays open, hidden).
                Outcome::Forward(Owner::Engine(name), _) if name.starts_with(crate::retail_menus::SCREEN) => {
                    let id = &name[crate::retail_menus::SCREEN.len()..];
                    match backdrop.index(id) {
                        Some(screen) => ms.stack.request(
                            Layer::Main,
                            Request::Push { screen, param: 0, keep_open: true, keep_visible: false },
                        ),
                        None => warn!("RETAIL_MENUS: no FE screen {id}"),
                    }
                }
                outcome => info!("RETAIL_MENUS settings select {id}: {outcome:?}"),
            }
        }
        for (id, direction) in steps {
            let outcome = menus.act(MenuAction::Step { entry: EntryRef::setting(&id), direction });
            info!("RETAIL_MENUS settings step {id} {direction:?}: {outcome:?}");
            refresh = refresh.or(Some(50));
        }
        if let Some(state) = refresh {
            let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
            fe.settings = std::mem::take(&mut ms.settings);
            fe.settings_titles = ms.settings_titles.clone();
            if let Err(e) = set_state(&mut ms.stage, ms.options_id, state, &mut fe) {
                error!("RETAIL_MENUS settings refresh: {e}");
            }
            ms.settings = fe.settings;
        }
    }
    if ms.stack.top(Layer::Main).is_none() {
        *fe_hud = crate::fe_backdrop::FeHud::default();
    }
    let stack: Vec<String> = ms.stack.screens(Layer::Main).filter_map(|i| backdrop.id(i).map(str::to_owned)).collect();
    backdrop.set_stack(stack);
    if let Err(e) = result {
        error!("RETAIL_MENUS stage stopped: {e}");
        ms.failed = true;
    }
}

/// Retail ScreenManagerBase::OpenScreen (sub_82D09140) arguments.
/// A Video Calibration step: post the page, close the screen, or push the "finished" popup (0x8260E950: Sk8Popup,
/// priority 0, not cancelable, Yes / No; its choice comes back through `VIDEO_CALIBRATION_POPUP`).
#[allow(clippy::too_many_arguments)]
fn run_calibration(
    stage: &mut Stage,
    stack: &mut FeStack,
    popups: &mut skate_core::fe_popup::PopupList,
    popup_dirty: &mut bool,
    cal: &mut skate_core::video_calibration::VideoCalibration,
    rules: &skate_core::video_calibration::CalibrationRules,
    id: i32,
    action: skate_core::video_calibration::CalibrationAction,
    screen: usize,
    fe: &mut FrontEnd,
) -> Result<(), String> {
    use skate_core::video_calibration::CalibrationAction as A;
    let _ = cal;
    match action {
        A::SetState(page) => set_state(stage, id, page as i32, fe),
        A::Close => {
            stack.request(Layer::Main, Request::Close { screen });
            Ok(())
        }
        A::Popup => {
            let id = popups.push(skate_core::fe_popup::PopupRequest {
                title: rules.popup_title.clone(),
                description: rules.popup_description.clone(),
                options: rules.popup_buttons.clone(),
                callback: VIDEO_CALIBRATION_POPUP.into(),
                ..Default::default()
            });
            info!("RETAIL_MENUS Video Calibration finished: popup {id}");
            *popup_dirty = true;
            Ok(())
        }
    }
}

/// FE vt+24 (`sub_825EC530`) and host 29 show (`0x825EF440`): hide every popup movie, then for the list's last
/// entry SetScreenState(1) and ShowScreen on its kind's movie, play core_popup and take input. An empty list hides.
fn show_popup(
    stage: &mut Stage,
    popups: &skate_core::fe_popup::PopupList,
    ids: &[(PopupKind, i32)],
    fe: &mut FrontEnd,
    sounds: &mut MessageWriter<FrontendSound>,
) -> Result<(), String> {
    let show = |stage: &mut Stage, fe: &mut FrontEnd, id: i32, on: bool| {
        stage.call_path(
            "_global.ScreenManager.ShowScreen",
            vec![Value::Text(id.to_string()), Value::Text(if on { "true" } else { "false" }.into())],
            fe,
        )
    };
    for (_, id) in ids {
        show(stage, fe, *id, false)?;
    }
    fe.popup.shown = None;
    fe.popup.input_ready = false;
    let Some((popup, request)) = popups.shown() else { return Ok(()) };
    let Some(handle) = ids.iter().find(|(k, _)| *k == request.kind).map(|(_, id)| *id) else {
        warn!("RETAIL_MENUS: popup {popup} has no movie ({}); run setup", request.kind.template());
        return Ok(());
    };
    // small_popup reads no options and answers only B when cancelable (doc 31 "Popup kinds").
    if request.kind == PopupKind::Small && !request.cancelable && !request.options.is_empty() {
        warn!("RETAIL_MENUS: popup {popup} ({}) is small with options but not cancelable: no press can close it", request.callback);
    }
    fe.popup.shown = Some(request.clone());
    stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(handle.to_string()), Value::Text("1".into())], fe)?;
    show(stage, fe, handle, true)?;
    sounds.write(FrontendSound::named("core_popup"));
    fe.popup.input_ready = true;
    info!("RETAIL_MENUS popup {popup} shown ({}, screen {handle}, {})", request.kind.template(), request.callback);
    Ok(())
}

/// A popup template's path from the menu tables' template list (`popup/sk8popup.swf`).
fn popup_path(assets: &std::path::Path, template: &str) -> Option<String> {
    let tables = std::fs::read_to_string(assets.join(crate::retail_menus::TABLES)).ok()?;
    let tables = skate_data::menu_tables::MenuTables::from_json(&tables).ok()?;
    tables.templates.iter().find(|t| t.name == template).map(|t| t.path.clone())
}

fn open_screen(stage: &mut Stage, path: &str, template: &str, fe: &mut FrontEnd) -> Result<(), String> {
    open_screen_on(stage, path, "screen", template, fe)
}

/// Replay editor timeline strip (82509B80 -> 82508E08 bands, 82509298 keys; b403): docs/hails-additions/31-retail-menus.md#replay-timeline-strip
fn timeline_draws(placeholder: &crate::apt_scene::Draw, feed: &crate::replay_menu_host::ReplayScreenFeed) -> Vec<crate::apt_scene::Draw> {
    let look = TimelineLook::default();
    let [o, px, py] = [0, 1, 2].map(|i| placeholder.vertices[i].position);
    let at = |x: f32, y: f32, u: f32, v: f32| crate::apt_scene::Vertex {
        position: [o[0] + x * (px[0] - o[0]) + y * (py[0] - o[0]), o[1] + x * (px[1] - o[1]) + y * (py[1] - o[1])],
        uv: [u, v],
    };
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32, texture: String, multiply: [f32; 4]| crate::apt_scene::Draw {
        texture,
        fill: placeholder.fill,
        vertices: vec![at(x0, y0, 0., 0.), at(x1, y0, 1., 0.), at(x1, y1, 1., 1.), at(x0, y0, 0., 0.), at(x1, y1, 1., 1.), at(x0, y1, 0., 1.)],
        multiply,
        add: [0.0; 4],
    };
    let mut out = Vec::new();
    if feed.instant {
        return out;
    }
    let under = feed.keys.iter().position(|k| (k - feed.position as f32).abs() <= look.snap);
    let a = placeholder.multiply[3];
    // One band per camera segment (the recorded camera before the first key); the segment under the playhead is
    // "selected" unless a key is under it.
    let segments = if feed.segments.is_empty() { vec![(0.0, 3)] } else { feed.segments.clone() };
    for (i, &(start, kind)) in segments.iter().enumerate() {
        let end = segments.get(i + 1).map_or(1.0, |s| s.0);
        let current = (start..end).contains(&(feed.position as f32)) || (i + 1 == segments.len() && feed.position as f32 >= end);
        let kind = usize::from(kind.min(3));
        let (rgb, alpha) = if current && under.is_none() { (look.selected[kind], look.alpha_selected) } else { (look.normal[kind], look.alpha) };
        out.push(quad(start * look.width, look.band_top, end * look.width, look.band_bottom, String::new(), [rgb[0], rgb[1], rgb[2], alpha * a]));
    }
    let camera_edit = (7..=10).contains(&feed.mode);
    for (i, k) in feed.keys.iter().enumerate() {
        let highlighted = under == Some(i);
        if highlighted && camera_edit {
            continue;
        }
        let x = k * look.width - 0.5;
        let name = if highlighted { "key_glow" } else { "key_default" };
        out.push(quad(x - look.key_size[0] / 2.0, 0.0, x + look.key_size[0] / 2.0, look.key_size[1],
            format!("{}texture/{name}", crate::retail_menu_movies::NAMED_IMAGE_PREFIX), [1.0, 1.0, 1.0, a]));
    }
    out
}

/// The timeline strip's retail look (b403, 82508E08 / 82509298), data a mod may override.
struct TimelineLook {
    width: f32,
    band_top: f32,
    band_bottom: f32,
    /// Band colours per camera type (0 Follow, 1 Tripod, 2 Free, 3 Game), normal / selected, and their alphas.
    normal: [[f32; 3]; 4],
    selected: [[f32; 3]; 4],
    alpha: f32,
    alpha_selected: f32,
    key_size: [f32; 2],
    /// A key within this fraction of the playhead is under it (`825084C8`).
    snap: f32,
}

impl Default for TimelineLook {
    fn default() -> Self {
        Self {
            width: 817.0,
            band_top: 3.0,
            band_bottom: 30.0,
            normal: [[0.41, 0.41, 0.19], [0.12, 0.28, 0.36], [0.27, 0.40, 0.15], [0.30, 0.30, 0.35]],
            selected: [[0.74, 0.74, 0.19], [0.29, 0.54, 0.65], [0.52, 0.72, 0.36], [0.53, 0.53, 0.53]],
            alpha: 0.65,
            alpha_selected: 0.85,
            key_size: [13.0, 51.0],
            snap: 0.00889,
        }
    }
}

/// OpenScreen bridge (0x82D09140): path, AS layer ("screen" unless the caller names one), the template's visible flag
/// ("0" for every menu template here), template name.
/// The action overlay movie (research b426: template 59 SignUpHUD).
const SIGNUP_MOVIE: &str = "source/screens/hud2/actionoverlay2";
const SIGNUP_TEMPLATE: &str = "SignUpHUD";

fn open_screen_on(stage: &mut Stage, path: &str, layer: &str, template: &str, fe: &mut FrontEnd) -> Result<(), String> {
    stage.call_path(
        "_global.ScreenManager.OpenScreen",
        vec![
            Value::Text(format!("source/screens/{path}")),
            Value::Text(layer.into()),
            Value::Text("0".into()),
            Value::Text(template.into()),
        ],
        fe,
    )?;
    Ok(())
}

/// Runs a screen action; a skater swap is returned for the caller that owns the character library.
fn run_fade_action(stack: &mut FeStack, action: FadeAction) -> Option<skate_core::spawn_skater::Skater> {
    match action {
        FadeAction::Push(screen) => stack.request(Layer::Main, Request::push(screen)),
        FadeAction::Close(screen) => stack.request(Layer::Main, Request::Close { screen }),
        FadeAction::SetPlayer(skater) => return Some(skater),
    }
    None
}

/// The Trick Guide's queued screen states (its picker callbacks post them from inside a movie call), in order; the
/// rebuild they start can queue more (bounded).
/// The Challenge Map's queued screen states (`SetState(n)`), and its close at the root (B).
fn flush_challenge_map(stage: &mut Stage, stack: &mut FeStack, map: Option<usize>, id: i32, fe: &mut FrontEnd) -> Result<(), String> {
    for state in std::mem::take(&mut fe.challenge_map.states) {
        set_state(stage, id, state, fe)?;
    }
    if std::mem::take(&mut fe.challenge_map.close) {
        if let Some(screen) = map {
            stack.request(Layer::Main, Request::Close { screen });
        }
    }
    Ok(())
}

/// The Skate Profile's queued screen states, once its movie loaded.
fn flush_offline_profile(stage: &mut Stage, id: i32, fe: &mut FrontEnd) -> Result<(), String> {
    if !fe.offline_profile.loaded {
        return Ok(());
    }
    for state in std::mem::take(&mut fe.offline_profile.states) {
        set_state(stage, id, state, fe)?;
    }
    Ok(())
}

fn flush_trick_guide(stage: &mut Stage, id: i32, fe: &mut FrontEnd) -> Result<(), String> {
    for _ in 0..8 {
        let states = std::mem::take(&mut fe.trick_guide.states);
        if states.is_empty() {
            return Ok(());
        }
        for state in states {
            set_state(stage, id, state, fe)?;
        }
    }
    Ok(())
}

/// The C++ SetScreenState through the facade: both as "%d" strings (sub_8263F320 index 10).
fn set_state(stage: &mut Stage, id: i32, state: i32, fe: &mut FrontEnd) -> Result<(), String> {
    stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.to_string()), Value::Text(state.to_string())], fe)?;
    Ok(())
}

/// Whether the movie made this call this frame (removes it).
fn take_call(fe: &mut FrontEnd, call: &str) -> bool {
    let before = fe.calls.len();
    fe.calls.retain(|c| c != call);
    fe.calls.len() != before
}

fn close(stage: &mut Stage, id: i32, fe: &mut FrontEnd) -> Result<(), String> {
    // Retail CloseScreen (sub_82D09298): the AS id as "%d"; the screen plays its outro.
    stage.call_path("_global.ScreenManager.CloseScreen", vec![Value::Text(id.to_string())], fe)?;
    Ok(())
}

fn render(
    mut commands: Commands,
    stage: Option<ResMut<MenuStage>>,
    movies: Option<Res<RetailMenuMovies>>,
    config: Res<crate::config::Config>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
    controllers: Option<Res<crate::input::ControllerInput>>,
    (text_entry, mut keyboard_fonts): (Option<Res<crate::extras_host::TextEntryView>>, Local<Option<crate::text_entry_ui::KeyboardFonts>>),
) {
    let (Some(mut ms), Some(movies)) = (stage, movies) else { return };
    let ms = &mut *ms;
    let mut draws = if ms.failed {
        Vec::new()
    } else {
        match ms.stage.draw(|key, movie| movies.scene(key, movie)) {
            Ok(draws) => draws,
            Err(e) => {
                error!("RETAIL_MENUS draw: {e}");
                ms.failed = true;
                Vec::new()
            }
        }
    };
    // The on-screen keyboard over FE 46, on the menu layer in the menus' fonts (text_entry_ui).
    if let Some(view) = text_entry.as_deref().filter(|v| v.keyboard.is_some()) {
        let fonts = keyboard_fonts.get_or_insert_with(|| {
            let mut assets = movies.shared_text_assets().unwrap_or_default();
            crate::text_entry_ui::KeyboardFonts::from(&mut assets)
        });
        draws.extend(crate::text_entry_ui::draws(view, fonts, &ms.language));
    }
    // Button prompts (CustomButton components, `prompt/<retail button>`): the first connected pad's printed labels
    // (SDL), Xbox naming without a pad; each label's glyph is generated once (prompt_glyphs).
    let pad = controllers.as_ref().and_then(|c| c.kinds.iter().flatten().next().cloned());
    for draw in &mut draws {
        let Some(button) = draw.texture.strip_prefix("prompt/") else { continue };
        let Some(prompt) = crate::prompt_glyphs::resolve_for(button, pad.as_deref()) else { continue };
        let key = prompt.key();
        let pixels = move || crate::prompt_glyphs::render(&prompt);
        if !ms.textures.contains_key(&key) {
            let handle = images.add(Image::new(
                Extent3d { width: crate::prompt_glyphs::SIZE, height: crate::prompt_glyphs::SIZE, depth_or_array_layers: 1 },
                TextureDimension::D2,
                pixels(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
            ms.textures.insert(key.clone(), handle);
        }
        draw.texture = key;
    }
    // An unresolved prompt draws nothing (an empty key would draw solid white).
    draws.retain(|d| !d.texture.starts_with("prompt/"));
    // Map markers: 32 x 32 px quads at the clip translation only (8268D960, 825E2618; b282, b263).
    let mut expanded = Vec::with_capacity(draws.len());
    for draw in draws.drain(..) {
        if draw.texture == crate::apt_scene::REPLAY_TIMELINE {
            expanded.extend(timeline_draws(&draw, &ms.replay_menu.feed));
            continue;
        }
        if draw.texture != crate::apt_scene::MAP_MARKERS {
            expanded.push(draw);
            continue;
        }
        let o = draw.vertices[0].position;
        if ms.challenge_map.origin != o {
            info!(
                "CHALLENGE_MAP markers at origin [{:.1}, {:.1}]: {} markers, first {:?}, picture {:?} (none = the default map)",
                o[0], o[1], ms.challenge_map.markers.len(), ms.challenge_map.markers.first().map(|(i, p, _)| (i.as_str(), [o[0] + p[0], o[1] + p[1]])),
                ms.challenge_map.state.picture
            );
        }
        ms.challenge_map.origin = o;
        let at = |x: f32, y: f32, u: f32, v: f32| crate::apt_scene::Vertex { position: [o[0] + x, o[1] + y], uv: [u, v] };
        for (icon, [x, y], _) in &ms.challenge_map.markers {
            let (a, b, c, d) = (at(x - 16., y - 16., 0., 0.), at(x + 16., y - 16., 1., 0.), at(x + 16., y + 16., 1., 1.), at(x - 16., y + 16., 0., 1.));
            expanded.push(crate::apt_scene::Draw {
                texture: format!("{}texture/{icon}", crate::retail_menu_movies::NAMED_IMAGE_PREFIX),
                fill: draw.fill,
                vertices: vec![a.clone(), b, c.clone(), a, c, d],
                multiply: draw.multiply,
                add: draw.add,
            });
        }
    }
    draws = expanded;
    // Image slots (ImageRenderComponent, `slot/<n>`): the slot's loaded image; an empty slot or an image nobody has
    // draws nothing (retail: the texture lookup finds nothing, b258 section 2), never the white fallback.
    for draw in &mut draws {
        if draw.texture.starts_with(crate::image_renderer_host::SLOT_TEXTURE_PREFIX) {
            draw.texture = ms.image_slots.texture(&draw.texture).unwrap_or_default().to_owned();
            if draw.texture.is_empty() {
                draw.texture = crate::image_renderer_host::SLOT_TEXTURE_PREFIX.to_owned();
            }
        }
    }
    draws.retain(|d| {
        let named = [crate::image_renderer_host::SLOT_TEXTURE_PREFIX, crate::image_renderer_host::ASSET_TEXTURE_PREFIX, crate::retail_menu_movies::NAMED_IMAGE_PREFIX]
            .iter()
            .any(|p| d.texture.starts_with(p));
        !named || movies.image(&d.texture).is_some()
    });
    // The loading screen hides the world behind it (black under the movie) from black until it fades back in.
    if ms.load.covered() {
        let v = |x: f32, y: f32| crate::apt_scene::Vertex { position: [x, y], uv: [0.0, 0.0] };
        let (x0, y0, x1, y1) = (-1280.0, -720.0, 2560.0, 1440.0);
        draws.insert(0, crate::apt_scene::Draw {
            texture: String::new(),
            vertices: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y0), v(x1, y1), v(x0, y1)],
            multiply: [0.0, 0.0, 0.0, 1.0],
            add: [0.0; 4],
            fill: crate::apt_scene::Fill::Solid,
        });
    }
    // The FE fader over everything (alpha 1 = black), past the stage edges for any aspect ratio.
    if ms.fade.alpha > 0.0 {
        let v = |x: f32, y: f32| crate::apt_scene::Vertex { position: [x, y], uv: [0.0, 0.0] };
        let (x0, y0, x1, y1) = (-1280.0, -720.0, 2560.0, 1440.0);
        draws.push(crate::apt_scene::Draw {
            texture: String::new(),
            vertices: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y0), v(x1, y1), v(x0, y1)],
            multiply: [0.0, 0.0, 0.0, ms.fade.alpha],
            add: [0.0; 4],
            fill: crate::apt_scene::Fill::Solid,
        });
    }
    let root = crate::retail_menu_movies::set_root(&config);
    for draw in &draws {
        if draw.texture.is_empty() || ms.textures.contains_key(&draw.texture) {
            continue;
        }
        let image = match movies.image(&draw.texture) {
            Some(MenuImage::Retail { path, width, height }) => std::fs::read(root.join(path))
                .ok()
                .filter(|b| b.len() == width as usize * height as usize * 4)
                .map(|b| (width, height, b)),
            Some(MenuImage::Mod(m)) => Some((m.width, m.height, m.rgba.clone())),
            None => None,
        };
        let Some((width, height, bytes)) = image else {
            warn!("RETAIL_MENUS texture missing: {}", draw.texture);
            ms.textures.insert(draw.texture.clone(), ms.white.clone());
            continue;
        };
        let handle = images.add(Image::new(
            Extent3d { width, height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            bytes,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ));
        ms.textures.insert(draw.texture.clone(), handle);
    }
    let (textures, white) = (&ms.textures, ms.white.clone());
    render_draws(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut ms.slots,
        &draws,
        |t| if t.is_empty() { Some(white.clone()) } else { textures.get(t).cloned() },
        MENU_Z,
        // Colour transforms in gamma space like the console front end (retail shot: near-black panel body).
        true,
    );
}

#[cfg(test)]
mod timeline_tests {
    use super::*;

    /// The editor timeline (b403): a band from y 3 to 30 in the clip's space coloured by the camera, a 13 x 51
    /// `key_default` per key, `key_glow` (and an unselected band) when the playhead is on a key; nothing in instant mode.
    #[test]
    fn timeline_draws_the_camera_band_and_the_keys() {
        let v = |x: f32, y: f32| crate::apt_scene::Vertex { position: [x, y], uv: [0.0, 0.0] };
        // The strip at (100, 50), unscaled.
        let placeholder = crate::apt_scene::Draw {
            texture: crate::apt_scene::REPLAY_TIMELINE.into(),
            fill: crate::apt_scene::Fill::TextureClamped,
            vertices: vec![v(100.0, 50.0), v(101.0, 50.0), v(100.0, 51.0)],
            multiply: [1.0; 4],
            add: [0.0; 4],
        };
        let mut feed = crate::replay_menu_host::ReplayScreenFeed { mode: 6, keys: vec![0.5], segments: vec![(0.0, 0)], position: 0.2, ..Default::default() };
        let draws = timeline_draws(&placeholder, &feed);
        assert_eq!(draws.len(), 2);
        // The band: Follow (type 0) selected colour, from x 100 to 917, y 53 to 80.
        assert_eq!(draws[0].multiply, [0.74, 0.74, 0.19, 0.85]);
        assert_eq!((draws[0].vertices[0].position, draws[0].vertices[2].position), ([100.0, 53.0], [917.0, 80.0]));
        // The key at x = 0.5 x 817 - 0.5, 13 wide, 51 high.
        assert!(draws[1].texture.ends_with("texture/key_default"));
        assert_eq!(draws[1].vertices[0].position, [100.0 + 408.0 - 6.5, 50.0]);
        // On the key: the glow, the band not selected (Follow normal colour).
        feed.position = 0.5;
        let draws = timeline_draws(&placeholder, &feed);
        assert!(draws[1].texture.ends_with("texture/key_glow"));
        assert_eq!(draws[0].multiply, [0.41, 0.41, 0.19, 0.65]);
        feed.instant = true;
        assert!(timeline_draws(&placeholder, &feed).is_empty());
    }
}
