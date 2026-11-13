//! Retail teleporters (b420 / b421 / b423): disc and DLC teleport challenges from `private/teleporters.json`.
//! Retail flow, prompt and mod reach: docs/hails-additions/38-dlc-support.md#teleporters-code
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_data::trigger_volumes::TriggerVolumeRecord;
use std::path::Path;

/// The popup callback id of the teleport confirm.
const CONFIRM: &str = "engine:teleporter";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Exit {
    pub map: String,
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
    /// The exit's world row (`dist_megapark_full`): a skate.Park world loads its layout (research b532).
    #[serde(default)]
    pub world: Option<String>,
}

/// One teleporter as setup wrote it (or a mod added it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Teleporter {
    /// The challenge key (`tele_world_to_stadium`), stable across installs; mods: `mod:<owner>:<key>`.
    pub id: String,
    pub map: String,
    /// The trigger volume's id (`%016x` instance id) in the map's volumes.
    pub volume_id: String,
    /// The volume when it is not in the map's world stream (the challenge hull's).
    #[serde(default)]
    pub volume: Option<TriggerVolumeRecord>,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub exits: Vec<Exit>,
    /// A skate.Park layout's teleporter (setup park_layout.py, research b438): armed only while the map was loaded
    /// with that layout (park_layout.rs).
    #[serde(default)]
    pub layout: Option<String>,
    /// The floating icon: its `ribbon_indicator` key and anchor (visual_indicators.rs, research b533).
    #[serde(default)]
    pub indicator: Option<crate::visual_indicators::Placement>,
}

#[derive(Deserialize)]
struct File {
    format: String,
    version: u32,
    teleporters: Vec<Teleporter>,
}

pub(crate) fn load(assets: &Path) -> Result<Vec<Teleporter>, String> {
    let path = assets.join("private/teleporters.json");
    let file: File = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if file.format != "skate3rust.teleporters" || file.version != 1 {
        return Err(format!("{}: unsupported {} v{}", path.display(), file.format, file.version));
    }
    let mut all = file.teleporters;
    all.extend(load_park(assets));
    Ok(all.into_iter().filter(valid).collect())
}

/// The skate.Park layouts' teleporters (setup park_layout.py `layout_teleporters`, research b438), each with its
/// layout; none when setup wrote none.
fn load_park(assets: &Path) -> Vec<Teleporter> {
    #[derive(Deserialize)]
    struct Park {
        version: u32,
        teleporters: Vec<Teleporter>,
    }
    let path = crate::park_layout::directory(assets).join("teleporters.json");
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    match serde_json::from_slice::<Park>(&bytes) {
        Ok(park) if park.version == 1 => park.teleporters.into_iter().filter(|t| t.layout.is_some()).collect(),
        Ok(park) => {
            warn!("SKATE_TELEPORTERS {}: version {} not supported", path.display(), park.version);
            Vec::new()
        }
        Err(error) => {
            warn!("SKATE_TELEPORTERS {}: {error}", path.display());
            Vec::new()
        }
    }
}

fn valid(t: &Teleporter) -> bool {
    !t.exits.is_empty() && t.exits.iter().all(|e| e.matrix.iter().flatten().all(|v| v.is_finite()))
}

/// A teleporter was confirmed (serialisable: the authority decides, peers apply; multiplayer-ready, no transport).
#[derive(Message, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TeleporterUsed {
    pub body: String,
    pub teleporter: String,
    pub exit: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Waiting,
    /// The confirm popup is up.
    Confirm,
}

/// The loaded map's teleporters (retail's and mods') and the local skater's prompt state.
#[derive(Resource, Default)]
pub(crate) struct Teleporters {
    generation: Option<u64>,
    pub retail: Vec<Teleporter>,
    /// Mods' teleporters by (owner, key): the ones on the loaded map are armed beside retail's.
    pub mods: std::collections::BTreeMap<(String, String), Teleporter>,
    /// The teleporter whose volume the local skater is inside.
    pub inside: Option<String>,
    /// The loaded map's skate.Park layout (CurrentMap::layout), for the layout teleporters.
    pub layout: Option<String>,
    pub phase: Phase,
    last_batch: u64,
    /// Used teleporters not yet delivered to Lua mods (`teleporter_used`, drained by the mod runtime).
    pub mod_queue: Vec<TeleporterUsed>,
    /// SKATE_VERIFY_TELEPORTER=<id> (with a verification capture): start in that teleporter's volume and press for
    /// the player once (no popup), so the run's capture shows the arrival (SKATE_VERIFY_TELEPORTER_PROMPT=1: no press).
    verify: Option<(String, bool)>,
}

impl Teleporters {
    /// Every armed teleporter: retail's, then mods' (a mod's teleporter with a retail id replaces it).
    pub(crate) fn armed(&self, map: Option<&Path>) -> Vec<&Teleporter> {
        let on_map = |t: &&Teleporter| map.is_some_and(|p| crate::teleport_menu::same_map(p, &t.map))
            && (t.layout.is_none() || t.layout == self.layout);
        let mods: Vec<&Teleporter> = self.mods.values().filter(on_map).collect();
        let mut armed: Vec<&Teleporter> = self.retail.iter().filter(on_map).filter(|t| !mods.iter().any(|m| m.id == t.id)).collect();
        armed.extend(mods);
        armed
    }
    pub(crate) fn by_volume(&self, map: Option<&Path>, volume: &str) -> Option<&Teleporter> {
        self.armed(map).into_iter().find(|t| t.volume_id == volume)
    }
    pub(crate) fn get(&self, map: Option<&Path>, id: &str) -> Option<&Teleporter> {
        self.armed(map).into_iter().find(|t| t.id == id)
    }
    /// Drop what a mod added.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.mods.retain(|(o, _), _| o != owner);
    }
}

/// The prompt line (ours; the font's `skate.` logo glyph spelled out).
pub(crate) fn prompt_text(t: &Teleporter) -> String {
    let title = t.title.as_deref().map(|s| s.chars().map(|c| if (c as u32) > 0x7e && !c.is_alphabetic() { "skate.".to_owned() } else { c.to_string() }).collect::<String>());
    format!("D-pad Up: {}", title.unwrap_or_else(|| t.id.clone()))
}

/// The confirm request (b421 / `828556D8`): one exit asks Yes / No, several list the exits.
pub(crate) fn confirm_popup(t: &Teleporter) -> skate_core::fe_popup::PopupRequest {
    let options = if t.exits.len() == 1 {
        vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()]
    } else {
        t.exits.iter().map(|e| e.locator.clone()).collect()
    };
    skate_core::fe_popup::PopupRequest {
        title: t.title.clone().or_else(|| t.title_id.clone()).unwrap_or_else(|| t.id.clone()),
        description: if t.exits.len() == 1 { "ID_PAUSEMENU_CONFIRM" } else { "ID_TELEPORT_STADIUM_RAMPS_DESC" }.into(),
        options,
        callback: CONFIRM.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// The exit a popup choice picks: Yes (index 1 of No / Yes) takes exit 0; a list option its exit; else none.
pub(crate) fn chosen_exit(t: &Teleporter, choice: i32) -> Option<usize> {
    if t.exits.len() == 1 {
        (choice == 1).then_some(0)
    } else {
        usize::try_from(choice).ok().filter(|&i| i < t.exits.len())
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Teleporters>()
        .add_message::<TeleporterUsed>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet))
        .add_systems(Update, (prompt, apply).chain());
}

/// A new map: read the teleporters, add the hull volumes to the map's volumes, forget the prompt.
fn arm(
    mut teleporters: ResMut<Teleporters>,
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
) {
    if teleporters.generation == Some(map.generation) {
        return;
    }
    if teleporters.generation.is_none() && config.verification_capture.is_some() {
        teleporters.verify = std::env::var("SKATE_VERIFY_TELEPORTER").ok().map(|id| (id, false));
    }
    teleporters.generation = Some(map.generation);
    teleporters.inside = None;
    teleporters.layout = map.layout.clone();
    teleporters.phase = Phase::Waiting;
    if teleporters.retail.is_empty() {
        teleporters.retail = load(&config.asset_root).unwrap_or_else(|e| {
            info!("SKATE_TELEPORTERS none ({e})");
            Vec::new()
        });
    }
    let mut armed = 0;
    for t in teleporters.armed(map.path.as_deref()) {
        if volumes.get(&t.volume_id).is_none() {
            match &t.volume {
                Some(record) => volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(record)),
                None => {
                    warn!("SKATE_TELEPORTERS {} has no volume {} on this map", t.id, t.volume_id);
                    continue;
                }
            }
        }
        armed += 1;
    }
    info!("SKATE_TELEPORTERS map={} armed={armed}", map.name);
    if let Some((id, false)) = teleporters.verify.clone() {
        let volume = teleporters.get(map.path.as_deref(), &id).and_then(|t| volumes.get(&t.volume_id)).map(|v| v.shape.center);
        if let Some(c) = volume {
            info!("SKATE_VERIFY_TELEPORTER {id} start in its volume at {c:?}");
            let _ = skater.travel_to([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [c[0], c[1], c[2], 1.]]);
        }
    }
}

/// The local skater's prompt: inside / outside from the trigger events, D-pad Up asks, the popup's choice confirms.
#[allow(clippy::too_many_arguments)]
fn prompt(
    mut commands: Commands,
    mut teleporters: ResMut<Teleporters>,
    map: Res<crate::map_transition::CurrentMap>,
    input: Res<crate::input::ControllerInput>,
    mut entered: MessageReader<crate::trigger_volumes::TriggerEntered>,
    mut exited: MessageReader<crate::trigger_volumes::TriggerExited>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
    mut used: MessageWriter<TeleporterUsed>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    lines: Query<Entity, With<PromptLine>>,
) {
    let path = map.path.clone();
    for e in exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER) {
        if teleporters.by_volume(path.as_deref(), &e.volume).map(|t| t.id.clone()) == teleporters.inside {
            teleporters.inside = None;
        }
    }
    for e in entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER) {
        if let Some(t) = teleporters.by_volume(path.as_deref(), &e.volume) {
            teleporters.inside = Some(t.id.clone());
        }
    }
    let choices: Vec<i32> = popups.engine_choices.iter().filter(|(c, _)| c == CONFIRM).map(|&(_, i)| i).collect();
    popups.engine_choices.retain(|(c, _)| c != CONFIRM);
    let current = teleporters.inside.clone().and_then(|id| teleporters.get(path.as_deref(), &id).cloned());
    if teleporters.phase == Phase::Confirm {
        if let Some(choice) = choices.last() {
            teleporters.phase = Phase::Waiting;
            if let Some((t, exit)) = current.as_ref().and_then(|t| chosen_exit(t, *choice).map(|x| (t, x))) {
                used.write(TeleporterUsed { body: crate::trigger_volumes::PLAYER.into(), teleporter: t.id.clone(), exit });
            }
        }
    }
    let gameplay = crate::graphics_menu::gameplay_active(menu) && !replay.active;
    let show = gameplay && teleporters.phase == Phase::Waiting && current.is_some();
    // Retail input.cfg: D-pad Up pressed (LB + D-pad Up is the session marker's return).
    let (modifier, _, _) = input.session_marker_actions();
    let pressed = show && !modifier && input.dpad_up_pressed() && teleporters.last_batch != input.consumed_batches;
    teleporters.last_batch = input.consumed_batches;
    if let (Some((id, done)), Some(t)) = (teleporters.verify.as_mut(), current.as_ref()) {
        // SKATE_VERIFY_TELEPORTER_PROMPT=1: stay in the volume without pressing (the capture shows the prompt).
        if !*done && *id == t.id && show && std::env::var("SKATE_VERIFY_TELEPORTER_PROMPT").as_deref() != Ok("1") {
            *done = true;
            info!("SKATE_VERIFY_TELEPORTER {id} pressed");
            used.write(TeleporterUsed { body: crate::trigger_volumes::PLAYER.into(), teleporter: t.id.clone(), exit: 0 });
        }
    }
    if pressed {
        let t = current.as_ref().expect("shown with a teleporter");
        if popups.stage_ready {
            popups.engine_requests.push(confirm_popup(t));
            teleporters.phase = Phase::Confirm;
        } else {
            used.write(TeleporterUsed { body: crate::trigger_volumes::PLAYER.into(), teleporter: t.id.clone(), exit: 0 });
        }
    }
    // The prompt: retail's action overlay (state 2 with the title) when the stage has it, else our text line.
    let shown = show && !pressed;
    popups.signup_title = shown.then(|| current.as_ref().map(|t| t.title.clone().or_else(|| t.title_id.clone())
        .unwrap_or_else(|| t.id.clone()))).flatten();
    let line = lines.iter().next();
    match (shown && !popups.signup_ready, line) {
        (true, None) => {
            let text = prompt_text(current.as_ref().expect("shown with a teleporter"));
            commands.spawn((PromptLine, GlobalZIndex(5), Node { position_type: PositionType::Absolute, bottom: percent(18),
                width: percent(100), justify_content: JustifyContent::Center, ..default() }))
                .with_child((Text::new(text), TextFont { font_size: 24., ..default() }, TextColor(Color::WHITE)));
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

#[derive(Component)]
struct PromptLine;

/// Move the skater for a confirmed teleporter: placed at once on this map, through the map transition otherwise.
fn apply(
    mut used: MessageReader<TeleporterUsed>,
    mut teleporters: ResMut<Teleporters>,
    current: Res<crate::map_transition::CurrentMap>,
    config: Res<crate::config::Config>,
    net: Res<crate::multiplayer::Multiplayer>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut travel: ResMut<crate::challenge_map::ChallengeMapTravel>,
    mut transition: ResMut<crate::map_transition::MapTransition>,
) {
    let mut queue = Vec::new();
    for event in used.read() {
        let Some(t) = teleporters.get(current.path.as_deref(), &event.teleporter) else { continue };
        let Some(exit) = t.exits.get(event.exit) else { continue };
        // Retail arrives on the board, at rest (b455: 82D55280 passes IsOnBoard = 1; 82C0D680 zeroes velocities).
        let on_board = true;
        let request = crate::challenge_map::TeleportRequest { destination: t.id.clone(), map: exit.map.clone(),
            transform: exit.matrix, on_board, session_marker: false };
        // A skate.Park world on the exit's map loads with its layout, as the Challenge Map does (challenge_map::plan).
        let worlds = crate::park_layout::worlds(&config.asset_root);
        let layout = crate::park_layout::layout_for(&worlds, exit.world.as_deref(), &exit.map);
        info!("SKATE_TELEPORTER {} exit={} map={} world={:?} layout={:?} locator={}", t.id, event.exit, exit.map, exit.world, layout, exit.locator);
        queue.push(event.clone());
        if current.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &exit.map)) && layout == current.layout.as_deref() {
            if let Err(e) = skater.travel_ex(exit.matrix, None, on_board) {
                warn!("SKATE_TELEPORTER {} failed: {e}", t.id);
            }
            continue;
        }
        let maps = crate::map_library::discover(&config.asset_root);
        let Some(entry) = maps.iter().find(|m| m.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &exit.map))) else {
            warn!("SKATE_TELEPORTER {} refused: map {} is not installed", t.id, exit.map);
            continue;
        };
        if net.active() {
            warn!("SKATE_TELEPORTER {} refused: no map switch in multiplayer", t.id);
            continue;
        }
        let entry = crate::map_library::Entry { world: layout.and(exit.world.clone()), ..entry.clone() };
        travel.start((request, Some(entry)), current.path.clone(), &mut transition);
    }
    teleporters.mod_queue.extend(queue);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teleporter(exits: usize) -> Teleporter {
        let mut m = [[0.0; 4]; 4];
        m[3] = [1.0, 2.0, 3.0, 1.0];
        Teleporter { id: "tele_world_to_stadium".into(), map: "University".into(), volume_id: "2c701706002605aa".into(),
            volume: None, title_id: Some("ID_TELE_WORLD_TO_STADIUM_PARK".into()), title: Some("Enter Stadium \u{a9}Park".into()),
            exits: (0..exits).map(|i| Exit { map: "MegaPark".into(), locator: format!("dest_{i}"), matrix: m, world: None }).collect(), layout: None, indicator: None }
    }

    #[test]
    fn one_exit_asks_no_yes_and_yes_takes_exit_zero() {
        let t = teleporter(1);
        let popup = confirm_popup(&t);
        assert_eq!(popup.options, ["ID_COMMON_NO", "ID_COMMON_YES"]);
        assert_eq!(popup.description, "ID_PAUSEMENU_CONFIRM");
        assert_eq!(popup.callback, CONFIRM);
        assert_eq!(chosen_exit(&t, 1), Some(0));
        assert_eq!(chosen_exit(&t, 0), None);
        assert_eq!(chosen_exit(&t, -1), None, "B / cancel goes back to waiting");
    }

    #[test]
    fn several_exits_list_them_and_the_option_picks_its_exit() {
        let t = teleporter(3);
        assert_eq!(confirm_popup(&t).options.len(), 3);
        assert_eq!(chosen_exit(&t, 2), Some(2));
        assert_eq!(chosen_exit(&t, 3), None);
    }

    #[test]
    fn a_park_teleporter_is_armed_only_with_its_layout() {
        let mut all = Teleporters::default();
        all.retail = vec![Teleporter { id: "skpk:ZenPlaza_Full:0".into(), layout: Some("ZenPlaza_Full".into()), ..teleporter(1) }];
        let path = std::path::PathBuf::from("maps/University.skate");
        assert!(all.armed(Some(&path)).is_empty(), "the Empty world places no teleporter");
        all.layout = Some("ZenPlaza_Full".into());
        assert_eq!(all.armed(Some(&path)).len(), 1);
    }

    #[test]
    fn the_prompt_spells_out_the_logo_glyph() {
        assert_eq!(prompt_text(&teleporter(1)), "D-pad Up: Enter Stadium skate.Park");
    }

    #[test]
    fn a_mod_teleporter_replaces_the_retail_one_with_its_id_and_only_this_map_is_armed() {
        let mut all = Teleporters::default();
        all.retail = vec![teleporter(1), Teleporter { map: "DownTown".into(), id: "other".into(), ..teleporter(1) }];
        let path = std::path::PathBuf::from("maps/University.skate");
        assert_eq!(all.armed(Some(&path)).len(), 1);
        all.mods.insert(("m".into(), "k".into()), Teleporter { volume_id: "beef".into(), ..teleporter(1) });
        let armed = all.armed(Some(&path));
        assert_eq!((armed.len(), armed[0].volume_id.as_str()), (1, "beef"));
        all.clear_owner("m");
        assert_eq!(all.armed(Some(&path))[0].volume_id, "2c701706002605aa");
    }

    #[test]
    fn setup_files_load_and_invalid_exits_are_dropped() {
        let dir = std::env::temp_dir().join(format!("skate-teleporters-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("private")).unwrap();
        let mut bad = teleporter(1);
        bad.id = "bad".into();
        bad.exits[0].matrix[0][0] = f32::NAN;
        let text = serde_json::json!({"format": "skate3rust.teleporters", "version": 1, "teleporters": [teleporter(1), bad]});
        std::fs::write(dir.join("private/teleporters.json"), text.to_string().replace("NaN", "null")).unwrap();
        // serde_json writes NaN as null, which is no f32: the whole file is refused rather than half read.
        assert!(load(&dir).is_err());
        let text = serde_json::json!({"format": "skate3rust.teleporters", "version": 1, "teleporters": [teleporter(1)]});
        std::fs::write(dir.join("private/teleporters.json"), text.to_string()).unwrap();
        assert_eq!(load(&dir).unwrap(), vec![teleporter(1)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
