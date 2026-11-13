use crate::{Command, Manifest, validate_package, vm::Vm};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
struct Bones {
    vm: Vm,
    snapshot: Value,
    commands: Vec<Command>,
    tick: u64,
}
impl Bones {
    fn new(settings: Value) -> Self {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples/broken-bones");
        let manifest = validate_package(&root).unwrap();
        let mut options: BTreeMap<_, _> = manifest
            .settings
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect();
        options.extend(
            settings
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        let snapshot = json!({"player":{"mode":"ground","bailing":false},"player_physics":{"tick":1,"joints":[{"index":3,"name":"JOINT_LEFT_ARM_FOREARM","parent":5,"child":4,"override_owner":null}],"contacts":[],"parts":[]}});
        let vm = Vm::new(&root, &manifest, &options, &snapshot).unwrap();
        let mut h = Self {
            vm,
            snapshot,
            commands: vec![],
            tick: 1,
        };
        h.call("on_load", json!({}));
        h.commands.clear();
        h
    }
    fn call(&mut self, name: &str, arg: Value) {
        let commands = self.vm.call(name, arg, &self.snapshot).unwrap();
        assert!(commands.iter().all(Command::validate));
        self.commands.extend(commands);
    }
    fn step(&mut self) {
        self.tick += 1;
        self.snapshot["player_physics"]["tick"] = json!(self.tick);
        self.call("on_update", json!({"dt":0.05}));
        self.call("on_fixed_update", json!({"dt":0.05}));
    }
    fn contact(&mut self, speed: f32, impulse: f32) {
        self.snapshot["player_physics"]["contacts"] = json!([{"id":"4/world","phase":"begin","a":{"kind":"skater","index":4},"b":{"kind":"world"},"closing_speed":speed,"impulse":[0.,impulse,0.],"force":[0.,999999.,0.]}]);
    }
    fn requests(&self) -> Vec<&Command> {
        self.commands
            .iter()
            .filter(|c| matches!(c, Command::Request { .. }))
            .collect()
    }
    fn acknowledge(&mut self, ok: bool) {
        let (key, token) = self
            .commands
            .iter()
            .rev()
            .find_map(|c| {
                if let Command::Request { key, token, .. } = c {
                    Some((key.clone(), *token))
                } else {
                    None
                }
            })
            .unwrap();
        self.snapshot["command_results"] = json!({"examples.broken-bones":{key:{"token":token,"ok":ok,"error":if ok{Value::Null}else{json!("owned by another mod")}}}});
    }
}
#[test]
fn bones_require_both_closing_speed_and_impulse_not_support_force() {
    let mut h = Bones::new(json!({}));
    h.contact(0., 1000.);
    h.step();
    assert!(h.requests().is_empty());
    h.contact(9., 10.);
    h.step();
    assert!(h.requests().is_empty());
    h.contact(9., 100.);
    h.step();
    assert_eq!(h.requests().len(), 1);
    match h.requests()[0] {
        Command::Request { command, .. } => match &**command {
            Command::PlayerJoint { joint, options } => {
                assert_eq!(*joint, 3);
                assert_eq!(options.drive_enabled, Some(false));
                assert_eq!(options.possession_enabled, Some(false));
                assert!(options.descendants);
                assert_ne!(options.enabled, Some(false));
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
    h.acknowledge(true);
    h.commands.clear();
    h.step();
    assert!(h.requests().is_empty());
    assert!(h.commands.iter().any(|c|matches!(c,Command::NetworkState{key,value} if key=="injuries" && value["j"]==json!([3]))));
    h.commands.clear();
    h.call(
        "on_event",
        json!({"name":"menu_action","menu":"bones","item":"heal"}),
    );
    assert!(
        h.commands
            .iter()
            .any(|c| matches!(c, Command::PlayerResetJoint { joint: 3 }))
    );
}
#[test]
fn bones_wait_for_bail_and_respect_other_mod_joint_ownership() {
    let mut h = Bones::new(json!({"only_bails":true}));
    h.contact(9., 100.);
    h.step();
    assert!(h.requests().is_empty());
    h.snapshot["player_physics"]["contacts"] = json!([]);
    h.snapshot["player"]["bailing"] = json!(true);
    h.step();
    assert_eq!(h.requests().len(), 1);
    let mut h = Bones::new(json!({}));
    h.snapshot["player_physics"]["joints"][0]["override_owner"] = json!("another.mod");
    h.contact(9., 100.);
    h.step();
    assert!(h.requests().is_empty());
}
#[test]
fn bones_failed_override_is_reported_without_claiming_an_injury() {
    let mut h = Bones::new(json!({}));
    h.contact(9., 100.);
    h.step();
    h.acknowledge(false);
    h.commands.clear();
    h.snapshot["player_physics"]["contacts"] = json!([]);
    h.step();
    assert!(
        !h.commands
            .iter()
            .any(|c| matches!(c,Command::NetworkState{value,..} if value["j"]==json!([3])))
    );
    assert!(
        h.commands.iter().any(
            |c| matches!(c,Command::Overlay{text,..} if text.contains("owned by another mod"))
        )
    );
}
#[test]
fn generalized_commands_reject_invalid_refs_and_nested_requests() {
    for v in [
        json!({"kind":"native_impulse","body":{"kind":"remote","index":0},"impulse":[1,0,0],"angular":false}),
        json!({"kind":"input_override","action":1,"value":0}),
        json!({"kind":"graph_gate","graph":"motion","target":"raw_memory","index":1,"enabled":false}),
        json!({"kind":"request","key":"outer","command":{"kind":"request","key":"inner","command":{"kind":"log","text":"x"}}}),
    ] {
        let c: Command = serde_json::from_value(v).unwrap();
        assert!(!c.validate());
    }
}
#[test]
fn generalized_wrappers_route_typed_bodies_and_ignore_stale_receipts() {
    let root = std::env::temp_dir().join(format!("skate-general-api-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.lua"),
        r#"
 return {
 on_load=function()
  sdk.bodies.impulse({kind='skater',index=4},{1,2,3},{0,1,0})
  sdk.bodies.impulse({kind='mod',key='car'},{1,0,0})
  sdk.input.override_action(64,0.5)
  sdk.input.override_action(64,nil)
  sdk.graphs.set_enabled('motion','state',3,false)
  sdk.engine.inspect('catalog','scoring')
  sdk.commands.request('move',{kind='log',text='one'})
 end,
 on_event=function() sdk.commands.request('move',{kind='log',text='two'}) end,
 on_fixed_update=function() assert(sdk.commands.result('move')==nil) end
 }"#,
    )
    .unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.general","api":2,"name":"test","version":"1","author":"test","description":"test","entry":"main.lua"})).unwrap();
    let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &json!({})).unwrap();
    let commands = vm.call("on_load", json!({}), &json!({})).unwrap();
    assert!(commands.iter().all(Command::validate));
    assert!(matches!(commands[0], Command::NativeImpulse { .. }));
    assert!(matches!(commands[1], Command::PhysicsImpulse { .. }));
    let old = commands
        .iter()
        .find_map(|c| {
            if let Command::Request { key, token, .. } = c {
                (key == "move").then_some(*token)
            } else {
                None
            }
        })
        .unwrap();
    vm.call("on_event", json!({}), &json!({})).unwrap();
    vm.call(
        "on_fixed_update",
        json!({}),
        &json!({"command_results":{"tests.general":{"move":{"token":old,"ok":true}}}}),
    )
    .unwrap();
    std::fs::remove_file(root.join("main.lua")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

struct FlatGround;
impl crate::DynamicsHost for FlatGround {
    fn raycast(&mut self, o: [f32; 3], _: [f32; 3], _: &crate::RaycastOptions) -> Option<Value> {
        Some(json!({"point":[o[0],0.,o[2]],"normal":[0.,1.,0.],"distance":o[1]}))
    }
    fn velocity_at(&self, _: &str, _: [f32; 3]) -> Option<[f32; 3]> {
        Some([0.; 3])
    }
    fn effective_inv_mass(&self, _: &str, _: [f32; 3], _: [f32; 3]) -> Option<f32> {
        Some(0.001)
    }
    fn spring_ray(
        &mut self,
        _: &str,
        _: skate_dynamics::SpringRayDesc,
    ) -> Option<skate_dynamics::SpringRayHit> {
        None
    }
    fn local_ang_accel_impulse(&self, _: &str, _: [f32; 3], _: f32) -> Option<[f32; 3]> {
        Some([0.; 3])
    }
}
#[test]
fn skyline_waits_for_spawn_receipt_and_reports_failure_without_disabling_mod() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods/Skyline_Drive_Mod");
    let manifest = validate_package(&root).unwrap();
    let options = manifest
        .settings
        .iter()
        .map(|(k, s)| (k.clone(), s.default.clone()))
        .collect();
    let mut s = json!({"player":{"position":[0.,1.,0.],"heading":0.},"physics":{"bodies":{}},"keys":{"F10":true}});
    let mut vm = Vm::new(&root, &manifest, &options, &s).unwrap();
    let mut call = |name: &str, s: &Value| {
        crate::with_host(&mut FlatGround, || vm.call(name, json!({"dt":0.05}), s)).unwrap()
    };
    assert!(call("on_load", &s).iter().all(Command::validate));
    // F10 removes old resources; spawning happens the following fixed tick.
    call("on_fixed_update", &s);
    s["keys"]["F10"] = json!(false);
    let cmds = call("on_fixed_update", &s);
    let token = cmds
        .iter()
        .find_map(|c| {
            if let Command::Request {
                key,
                token,
                command,
            } = c
            {
                assert!(matches!(&**command, Command::PhysicsSpawn { .. }));
                (key == "spawn_chassis").then_some(*token)
            } else {
                None
            }
        })
        .expect("spawn request");
    for _ in 0..12 {
        let cmds = call("on_fixed_update", &s);
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c,Command::GraphicsMesh{key,..} if key=="skyline_visual"))
        );
    }
    s["command_results"] = json!({manifest.id.clone():{"spawn_chassis":{"token":token,"ok":false,"error":"collision model unavailable"}}});
    let cmds = call("on_fixed_update", &s);
    assert!(
        cmds.iter().any(
            |c| matches!(c,Command::Log{text} if text.contains("collision model unavailable"))
        )
    );
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c,Command::GraphicsMesh{key,..} if key=="skyline_visual"))
    );
    // Retry must ignore the old failure receipt, and bind graphics only after success.
    s["keys"]["F10"] = json!(true);
    call("on_fixed_update", &s);
    s["keys"]["F10"] = json!(false);
    let cmds = call("on_fixed_update", &s);
    let new_token = cmds
        .iter()
        .find_map(|c| {
            if let Command::Request { token, .. } = c {
                Some(*token)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(token, new_token);
    call("on_fixed_update", &s);
    s["command_results"][&manifest.id]["spawn_chassis"] = json!({"token":new_token,"ok":true});
    s["physics"]["bodies"]["chassis"] = json!({"position":[0.,1.,4.5],"rotation":[0.,0.,0.,1.],"linvel":[0.,0.,0.],"angvel":[0.,0.,0.]});
    let cmds = call("on_fixed_update", &s);
    assert!(cmds.iter().all(Command::validate));
    assert!(
        cmds.iter()
            .any(|c| matches!(c,Command::GraphicsMesh{key,..} if key=="skyline_visual"))
    );
    call("on_unload", &s);
}

/// `"enabled_by_default": false` keeps a mod off without a saved preference (dev / test mods),
/// the manifest default keeps the old behaviour (on), and a saved preference wins.
#[test]
fn opt_in_mods_start_disabled_without_a_preference() {
    let base = std::env::temp_dir().join(format!("skate-opt-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (root, prefs) = (base.join("mods"), base.join("prefs"));
    for (dir, id, extra) in [("a", "tests.on", ""), ("b", "tests.optin", r#","enabled_by_default":false"#)] {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("main.lua"), "return {}").unwrap();
        std::fs::write(
            d.join("mod.json"),
            format!(r#"{{"id":"{id}","api":2,"name":"t","version":"1.0.0","author":"t","description":"t","entry":"main.lua"{extra}}}"#),
        )
        .unwrap();
    }
    let mut m = crate::Manager::new(root.clone(), prefs.clone());
    m.scan(true);
    assert!(m.packages["tests.on"].enabled, "{:?}", m.diagnostics);
    assert!(!m.packages["tests.optin"].enabled);
    // A saved preference wins.
    std::fs::create_dir_all(&prefs).unwrap();
    std::fs::write(prefs.join("tests.optin.json"), r#"{"enabled":true}"#).unwrap();
    let mut m = crate::Manager::new(root, prefs);
    m.scan(true);
    assert!(m.packages["tests.optin"].enabled);
    let _ = std::fs::remove_dir_all(&base);
}

/// Doc 16 L7: editing a running mod's `audio.json` or a file it names reloads only the audio
/// content (the game hot-swaps it): the script keeps running and the changed files are handed
/// over once; editing anything else (the script) reloads the mod as before.
#[test]
fn audio_only_edits_keep_the_script_running() {
    use std::time::{Duration, Instant};
    let base = std::env::temp_dir().join(format!("skate-mods-audio-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (root, prefs) = (base.join("mods"), base.join("prefs"));
    let d = root.join("tests.audio");
    std::fs::create_dir_all(d.join("audio")).unwrap();
    std::fs::write(d.join("mod.json"), r#"{"id":"tests.audio","api":2,"name":"t","version":"1.0.0","author":"t","description":"t","entry":"main.lua"}"#).unwrap();
    std::fs::write(d.join("main.lua"), "return { on_load = function() sdk.log('loaded') end }").unwrap();
    std::fs::write(d.join("audio/a.wav"), b"one").unwrap();
    std::fs::write(d.join("audio/other.txt"), b"x").unwrap();
    std::fs::write(d.join("audio.json"), r#"{"version":1,"replace":{"ambience":{"04_dt_main":"audio/a.wav"}}}"#).unwrap();
    let mut m = crate::Manager::new(root, prefs);
    m.scan(true);
    let loads = |m: &crate::Manager| m.commands.iter().filter(|(_, c)| matches!(c, crate::Command::Log { .. })).count();
    assert!(m.packages["tests.audio"].running(), "{:?}", m.diagnostics);
    assert_eq!(loads(&m), 1);
    // The scan debounce: a change is taken once it held for 750 ms (scans 500 ms apart).
    let settle = |m: &mut crate::Manager| {
        for _ in 0..2 {
            m.last_scan = Instant::now() - Duration::from_secs(2);
            m.scan(false);
            std::thread::sleep(Duration::from_millis(800));
        }
        m.last_scan = Instant::now() - Duration::from_secs(2);
        m.scan(false);
    };
    std::fs::write(d.join("audio.json"), r#"{"version":1,"replace":{"ambience":{"04_dt_main":"audio/a.wav","04_dt_alt":"audio/b.wav"}}}"#).unwrap();
    settle(&mut m);
    let p = m.packages.get_mut("tests.audio").unwrap();
    assert!(p.running());
    assert_eq!(p.take_audio_changes(), Some(vec!["audio.json".to_owned()]));
    assert_eq!(p.take_audio_changes(), None, "taken once");
    assert_eq!(loads(&m), 1, "the script was not restarted");
    // A file the new audio.json names (new) and one the old named: audio only.
    std::fs::write(d.join("audio/b.wav"), b"two").unwrap();
    std::fs::write(d.join("audio/a.wav"), b"uno").unwrap();
    settle(&mut m);
    let mut changes = m.packages.get_mut("tests.audio").unwrap().take_audio_changes().unwrap();
    changes.sort();
    assert_eq!(changes, ["audio/a.wav", "audio/b.wav"]);
    assert_eq!(loads(&m), 1);
    // Anything else: the mod reloads as before.
    std::fs::write(d.join("audio/other.txt"), b"y").unwrap();
    settle(&mut m);
    assert_eq!(loads(&m), 2, "a full reload");
    assert_eq!(m.packages.get_mut("tests.audio").unwrap().take_audio_changes(), None);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn input_reads_controller_identity_and_accepts_action_keys() {
    let root = std::env::temp_dir().join(format!("skate-input-api-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.lua"),
        r#"
 return {
 on_load=function()
  assert(sdk.capabilities.controllers==1 and sdk.capabilities.action_ids==1)
  assert(sdk.input.controller()==nil and sdk.input.controller(2)==nil)
  assert(sdk.input.action_ids.a==80 and sdk.input.action_ids.left_stick_x==64 and sdk.input.action_ids.b==81)
  sdk.input.override_action('a',1)
  sdk.input.override_action('right_stick_y',-0.5)
  sdk.input.override_action(78,nil)
 end,
 on_update=function()
  local active=sdk.input.controller()
  assert(active.family=='xbox_elite' and active.vendor_id==0x045e and active.product_id==0x0b22)
  assert(active.paddles==0 and active.hardware_paddles==4 and active.backend=='sdl')
  assert(active.face_labels[1]=='A' and active.summary~=nil)
  assert(sdk.input.controller(0)==nil)
  local all=sdk.input.controllers()
  assert(all.active==1 and all.slots[2].family=='xbox_elite' and all.slots[1]==nil)
  assert(sdk.engine.read('input').controllers.active==1)
  assert(sdk.input.action('a')==0.25 and sdk.input.action(80)==0.25)
 end,
 on_event=function(e)
  if e.bad=='key' then sdk.input.override_action('jump',1) end
  if e.bad=='slot' then sdk.input.controller(4) end
 end
 }"#,
    )
    .unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.input","api":2,"name":"test","version":"1","author":"test","description":"test","entry":"main.lua"})).unwrap();
    let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &json!({})).unwrap();
    let commands = vm.call("on_load", json!({}), &json!({})).unwrap();
    let overrides: Vec<_> = commands.iter().filter_map(|c| match c {
        Command::InputOverride { action, value } => Some((*action, *value)),
        _ => None,
    }).collect();
    assert_eq!(overrides, [(80, Some(1.0)), (68, Some(-0.5)), (78, None)]);
    let mut actions = [0.0f32; 18];
    actions[16] = 0.25;
    let elite = json!({"family":"xbox_elite","name":"Xbox One Elite 2 Controller","vendor_id":0x045e,"product_id":0x0b22,
        "backend":"sdl","driver":"XInput#0","paddles":0,"hardware_paddles":4,"prompt_style":"xbox",
        "face_labels":["A","B","X","Y"],"summary":"Xbox One Elite 2 Controller [Xbox Elite]"});
    vm.call("on_update", json!({"dt":0.016}),
        &json!({"actions":actions,"controllers":{"active":1,"slots":[null,elite,null,null]}})).unwrap();
    // An unknown action key and a slot outside 0..3 are Lua errors, not commands.
    for bad in ["key", "slot"] {
        let error = vm.call("on_event", json!({"bad":bad}), &json!({})).unwrap_err();
        assert!(error.contains(if bad == "key" { "unknown action key jump" } else { "controller slot must be 0..3" }), "{error}");
    }
    std::fs::remove_file(root.join("main.lua")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

/// Dev and test mods in the repo's `mods/` ship disabled (lib.rs: a saved preference or SKATE3_MODS_ENABLE turns them
/// on): a dev mod left on runs for every player and costs the mod snapshot every fixed step (research b553).
#[test]
fn shipped_dev_mods_are_off_by_default() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        if !entry.path().join("mod.json").is_file() {
            continue;
        }
        let (manifest, _) = crate::validate_package_content(&entry.path()).unwrap_or_else(|e| panic!("{:?}: {e}", entry.path()));
        if manifest.id.starts_with("dev-") || manifest.name.contains("(dev)") || manifest.id.contains("test") {
            assert!(!manifest.enabled_by_default, "{}: dev mods ship off by default", manifest.id);
            checked += 1;
        }
    }
    assert!(checked > 0);
}
