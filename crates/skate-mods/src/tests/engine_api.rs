use serde_json::json;
use crate::{vm::Vm, Command, Manifest};
use std::{collections::BTreeMap, time::{SystemTime,UNIX_EPOCH}};

#[test]
fn public_engine_wrappers_emit_valid_commands_and_read_scoped_observations() {
    let root=std::env::temp_dir().join(format!("skate-engine-api-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.lua"),r#"
      return {on_load=function()
        assert(sdk.player.skater(42).name == 'Rider')
        assert(sdk.player.read().landing_seq == 8)
        assert(sdk.player.read().landed_trick == 'Kickflip')
        assert(sdk.session.info().authority == '42')
        assert(sdk.volumes.read('zone').inside[1] == '42')
        assert(sdk.volumes.read('private') == nil)
        assert(sdk.player.physics().ragdoll == false)
        assert(#sdk.player.contacts() == 0 and #sdk.player.joints() == 0)
        sdk.camera.watch(42)
        sdk.camera.watch(nil)
        sdk.player.teleport({position={1,2,3},velocity={2,0,0}})
        sdk.session.claim()
        sdk.session.transfer(42)
        sdk.session.teleport(42,{position={1,2,3}})
        sdk.volumes.box('zone',{position={0,0,0},size={4,4,4},opacity=0})
        sdk.volumes.remove('zone')
        sdk.camera.capture('view',{position={1,2,3},look_at={0,0,0}})
        sdk.graphics.mesh_buffer('screen',{capture='view'})
        sdk.camera.clear_capture('view')
        sdk.graphics.mesh('ghost',{path='cube.glb',opacity=0.25})
        sdk.ui.menu('challenges',{title='Challenges',items={{id='races',label='Races',children={{id='sprint',label='Sprint'}}}}})
        sdk.player.set_joint(3,{free_swing=true,free_twist=true,drive_enabled=false})
        sdk.player.reset_joint(3)
        sdk.player.reset_joints()
        sdk.ui.remove_menu('challenges')
      end}
    "#).unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.engine", "api":2,"name":"Engine API","version":"1.0.0","author":"test","description":"Generic wrappers","entry":"main.lua"})).unwrap();
    let snapshot=json!({"player":{"landing_seq":8,"landed_trick":"Kickflip"},"skaters":{"42":{"name":"Rider"}},"session":{"authority":"42"},"volumes":{"tests.engine":{"zone":{"inside":["42"]}},"another.mod":{"private":{"inside":["42"]}}}});
    let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snapshot).unwrap();
    let commands=vm.call("on_load",json!({}),&snapshot).unwrap();
    assert_eq!(commands.len(),17);
    assert!(commands.iter().all(Command::validate));
    assert!(matches!(&commands[0],Command::CameraWatch{peer:Some(p)} if p == "42"));
    assert!(matches!(&commands[9],Command::GraphicsMeshBuffer{options,..} if options.capture.as_deref()==Some("view")));
    assert!(matches!(&commands[11],Command::GraphicsMesh{opacity,..} if *opacity==0.25));
    drop(vm);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn invalid_engine_commands_are_rejected() {
    for value in [
        json!({"kind":"player_teleport","options":{"position":[200001,0,0]}}),
        json!({"kind":"volume_box","key":"zone","options":{"position":[0,0,0],"size":[0,1,1]}}),
        json!({"kind":"camera_capture","key":"view","options":{"position":[0,0,0],"look_at":[0,0,1],"width":65}}),
        json!({"kind":"graphics_mesh_buffer","key":"screen","options":{"capture":"view","texture":"image.png"}}),
        json!({"kind":"session_transfer","peer":"not-a-peer"}),
    ] {
        let command:Command=serde_json::from_value(value).unwrap();
        assert!(!command.validate());
    }
}
