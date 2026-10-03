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
