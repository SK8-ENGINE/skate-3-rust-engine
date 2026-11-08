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
        sdk.minimap.icon('gate',{position={10,0,-5}})
        sdk.minimap.remove('gate')
        sdk.minimap.show(false)
      end}
    "#).unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.engine", "api":2,"name":"Engine API","version":"1.0.0","author":"test","description":"Generic wrappers","entry":"main.lua"})).unwrap();
    let snapshot=json!({"player":{"landing_seq":8,"landed_trick":"Kickflip"},"skaters":{"42":{"name":"Rider"}},"session":{"authority":"42"},"volumes":{"tests.engine":{"zone":{"inside":["42"]}},"another.mod":{"private":{"inside":["42"]}}}});
    let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snapshot).unwrap();
    let commands=vm.call("on_load",json!({}),&snapshot).unwrap();
    assert_eq!(commands.len(),20);
    assert!(commands.iter().all(Command::validate));
    assert!(matches!(&commands[0],Command::CameraWatch{peer:Some(p)} if p == "42"));
    assert!(matches!(&commands[9],Command::GraphicsMeshBuffer{options,..} if options.capture.as_deref()==Some("view")));
    assert!(matches!(&commands[11],Command::GraphicsMesh{opacity,..} if *opacity==0.25));
    assert!(matches!(&commands[17],Command::MinimapIcon{key,options} if key=="gate" && options.icon=="race_gate" && options.position==[10.,0.,-5.]));
    assert!(matches!(&commands[18],Command::MinimapRemove{key} if key=="gate"));
    assert!(matches!(&commands[19],Command::MinimapShow{shown:false}));
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
        json!({"kind":"minimap_icon","key":"gate","options":{"position":[0,0,0],"scale":0}}),
    ] {
        let command:Command=serde_json::from_value(value).unwrap();
        assert!(!command.validate());
    }
}

#[test]
fn challenge_wrappers_read_the_snapshot_and_emit_commands() {
    let root=std::env::temp_dir().join(format!("skate-challenge-api-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.lua"),r#"
      return {on_load=function()
        assert(#sdk.challenges.list() == 1)
        assert(sdk.challenges.get('ots_a').high_score == 1200)
        assert(sdk.challenges.get('missing') == nil)
        assert(sdk.challenges.run().points == 300)
        sdk.challenges.start('ots_a')
        sdk.challenges.abort()
        sdk.challenges.set_enabled('ots_a', false)
        sdk.challenges.configure('ots_a', {owned_points=500, out_of_bounds_time=8})
        sdk.challenges.configure('ots_a')
      end}
    "#).unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.challenges", "api":2,"name":"Challenges","version":"1.0.0","author":"test","description":"Challenge wrappers","entry":"main.lua"})).unwrap();
    let snapshot=json!({"challenges":{"list":[{"id":"ots_a","high_score":1200}],"run":{"id":"ots_a","points":300}}});
    let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snapshot).unwrap();
    let commands=vm.call("on_load",json!({}),&snapshot).unwrap();
    assert_eq!(commands.len(),5);
    assert!(commands.iter().all(Command::validate));
    assert!(matches!(&commands[0],Command::ChallengeStart{id} if id=="ots_a"));
    assert!(matches!(&commands[1],Command::ChallengeAbort{}));
    assert!(matches!(&commands[2],Command::ChallengeEnable{enabled:false,..}));
    assert!(matches!(&commands[3],Command::ChallengeConfigure{options:Some(o),..} if o.owned_points==Some(500) && o.out_of_bounds_time==Some(8)));
    assert!(matches!(&commands[4],Command::ChallengeConfigure{options:None,..}));
    drop(vm);
    std::fs::remove_dir_all(&root).unwrap();
    let bad:Command=serde_json::from_value(json!({"kind":"challenge_configure","id":"ots_a","options":{"owned_display_time":-1}})).unwrap();
    assert!(!bad.validate());
}

#[test]
fn the_career_test_mod_skips_the_start_then_adds_sales() {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/career-test"));
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(root.join("mod.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert!(!manifest.enabled_by_default, "the dev test mod is opt-in");
    let snapshot = json!({"career": {"on": true, "board_sales": 0, "milestone": {"index": -2, "count": 6}}});
    let mut vm = Vm::new(root, &manifest, &BTreeMap::new(), &snapshot).unwrap();
    let first = vm.call("on_update", json!({"dt": 1.0}), &snapshot).unwrap();
    assert!(first.iter().any(|c| matches!(c, Command::CareerSkipStart {})), "{first:?}");
    let second = vm.call("on_update", json!({"dt": 1.0}), &snapshot).unwrap();
    assert!(second.iter().any(|c| matches!(c, Command::CareerAddSales { amount: 15000, .. })), "{second:?}");
    assert!(second.iter().any(|c| matches!(c, Command::CareerArmAction { id } if id == "career_test_bonus")), "{second:?}");
    assert!(first.iter().chain(&second).all(Command::validate));
}

#[test]
fn career_wrappers_read_the_snapshot_and_emit_commands() {
    let root=std::env::temp_dir().join(format!("skate-career-api-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.lua"),r#"
      return {on_load=function()
        assert(sdk.career.on())
        assert(sdk.career.get().board_sales == 15000)
        sdk.career.add_sales(5000, 'trick bonus')
        sdk.career.add_money(100)
        sdk.career.arm_action('milestone_02_complete')
        sdk.career.set_flag('seen_intro')
        sdk.career.grant_achievement('employee_2')
        sdk.career.grant_unlock(9, 'dist_megapark')
        sdk.career.grant_reward('pcu_training', true)
        sdk.career.skip_start()
        sdk.career.activate_challenge('ots_dwtn_01')
        sdk.career.activate_group('ots')
        sdk.career.set_team_name('Shred Co')
        sdk.career.set_teammate_name(2, 'Gerry')
        sdk.career.set_teammate_look(3, {gender='female'})
        sdk.career.set_teammate_preset(4, 'preset07')
      end}
    "#).unwrap();
    let manifest:Manifest=serde_json::from_value(json!({"id":"tests.career", "api":2,"name":"Career","version":"1.0.0","author":"test","description":"Career wrappers","entry":"main.lua"})).unwrap();
    let snapshot=json!({"career":{"on":true,"board_sales":15000}});
    let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snapshot).unwrap();
    let commands=vm.call("on_load",json!({}),&snapshot).unwrap();
    assert_eq!(commands.len(),14);
    assert!(matches!(&commands[13],Command::CareerTeammatePreset{n:4,id} if id=="preset07"));
    assert!(matches!(&commands[12],Command::CareerTeammateLook{n:3,profile:Some(p)} if p["gender"]=="female"));
    assert!(matches!(&commands[10],Command::CareerTeamName{name} if name=="Shred Co"));
    assert!(matches!(&commands[11],Command::CareerTeammateName{n:2,name} if name=="Gerry"));
    assert!(matches!(&commands[8],Command::CareerActivateChallenge{id} if id=="ots_dwtn_01"));
    assert!(matches!(&commands[9],Command::CareerActivateGroup{id} if id=="ots"));
    assert!(commands.iter().all(Command::validate));
    assert!(matches!(&commands[5],Command::CareerGrantUnlock{category:9,name} if name=="dist_megapark"));
    assert!(matches!(&commands[6],Command::CareerGrantReward{big:true,id} if id=="pcu_training"));
    assert!(matches!(&commands[7],Command::CareerSkipStart{}));
    assert!(matches!(&commands[0],Command::CareerAddSales{amount:5000,reason:Some(r)} if r=="trick bonus"));
    assert!(matches!(&commands[1],Command::CareerAddMoney{amount:100}));
    assert!(matches!(&commands[2],Command::CareerArmAction{id} if id=="milestone_02_complete"));
    assert!(matches!(&commands[3],Command::CareerSetFlag{value:true,..}));
    assert!(matches!(&commands[4],Command::CareerGrantAchievement{name} if name=="employee_2"));
    drop(vm);
    std::fs::remove_dir_all(&root).unwrap();
    let too_much:Command=serde_json::from_value(json!({"kind":"career_add_sales","amount":20_000_000u64})).unwrap();
    assert!(!too_much.validate());
    let bad_id:Command=serde_json::from_value(json!({"kind":"career_set_flag","name":"a:b","value":true})).unwrap();
    assert!(!bad_id.validate());
    let bad_teammate:Command=serde_json::from_value(json!({"kind":"career_teammate_name","n":0,"name":"x"})).unwrap();
    assert!(!bad_teammate.validate());
}
