use crate::{vm::Vm, Command, validate_package};
use serde_json::{json,Value};
use std::{collections::BTreeMap,path::PathBuf};
struct Harness { vm:Vm, snapshot:Value, wire:Value, text:String, commands:Vec<Command> }
impl Harness {
 fn new(package:&str, settings:Value, id:&str, host:&str)->Self {
  let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples").join(package);
  let manifest=validate_package(&root).unwrap();
  let mut options:BTreeMap<String,Value>=manifest.settings.iter().map(|(k,s)|(k.clone(),s.default.clone())).collect();
  for (k,v) in settings.as_object().unwrap(){options.insert(k.clone(),v.clone());}
  let p=json!({"position":[0.,2.,0.],"heading":0.,"landing_seq":7,"bail_seq":0,"clean":true,"sketchy":false,"on_board":true,"bailing":false});
  let snapshot=json!({"player":p,"skaters":{"99":p,"1":p},"physics":{"bodies":{}},"network":{"active":true,"local_id":id,"host_id":host,"is_host":id==host,"players":["99","1"],"states":{}}});
  let vm=Vm::new(&root,&manifest,&options,&snapshot).unwrap();
  let mut h=Self{vm,snapshot,wire:json!({}),text:String::new(),commands:vec![]};h.call("on_load",json!({}));h
 }
 fn call(&mut self,name:&str,arg:Value) {
  let commands=self.vm.call(name,arg,&self.snapshot).unwrap();
  for c in &commands {assert!(c.validate(),"{c:?}");match c {
   Command::NetworkState{key,value}=>{assert!(serde_json::to_vec(value).unwrap().len()<=512);self.wire[key]=value.clone();}
   Command::Overlay{text,..}=>self.text=text.clone(), _=>{}
  }}self.commands.extend(commands);
 }
 fn event(&mut self,item:&str){self.call("on_event",json!({"name":"menu_action","menu":"challenge","item":item}));}
 fn step(&mut self,dt:f64){self.call("on_update",json!({"dt":dt}));self.call("on_fixed_update",json!({"dt":dt}));}
 fn land(&mut self,trick:&str){let p=&mut self.snapshot["skaters"]["1"];p["landed_trick"]=json!(trick);p["landing_seq"]=json!(p["landing_seq"].as_u64().unwrap()+1);}
}
#[test]
fn simon_confirmed_landing_requires_rideaway_and_bail_cancels_it() {
 let mut h=Harness::new("simon-says",json!({"countdown_secs":1,"ride_away":1,"allow_flips":false,"allow_shuvits":false,"allow_grabs":false,"allow_grinds":false,"allow_manuals_plants":false,"allow_spins":false}),"99","99");
 h.event("start");h.step(1.1);assert_eq!(h.wire["match"]["p"],"copy");
 h.snapshot["network"]["states"]=json!({"simon.says":{"1":{"round":{"e":h.wire["match"]["e"],"n":1,"l":7,"b":0}}}});
 let trick=h.text.split("Call: ").nth(1).unwrap().split(" | ").next().unwrap().to_owned();
 h.snapshot["skaters"]["1"]["trick"]=json!(trick);h.step(0.1);assert_eq!(h.wire["match"]["p"],"copy");
 h.land(&trick);h.step(0.1);assert_eq!(h.wire["match"]["p"],"copy");
 h.snapshot["skaters"]["1"]["bail_seq"]=json!(1);h.step(1.1);assert_eq!(h.wire["match"]["p"],"copy");
 h.land(&trick);h.step(0.1);h.step(1.1);
 assert_eq!(h.wire["match"]["p"],"break");assert_eq!(h.wire["match"]["w"],2);assert_eq!(h.wire["match"]["l"],"23");
}
#[test]
fn simon_clients_follow_host_elimination_and_stop_restores_control() {
 let mut h=Harness::new("simon-says",json!({}),"1","99");h.event("start");assert!(h.wire["match"].is_null());
 h.snapshot["network"]["states"]=json!({"simon.says":{"99":{"match":{"e":"one","p":"copy","n":1,"c":1,"ids":["99","1"],"l":"30","r":10,"w":0}}}});
 h.step(0.1);assert!(h.commands.iter().any(|c|matches!(c,Command::PlayerSuspend{suspended:true})));
 assert!(h.commands.iter().any(|c|matches!(c,Command::CameraWatch{peer:Some(p)} if p=="99")));
 h.commands.clear();h.snapshot["network"]["states"]["simon.says"]["99"]["match"]["p"]=json!("stopped");h.step(0.1);
 assert!(h.commands.iter().any(|c|matches!(c,Command::PlayerSuspend{suspended:false})));
}
#[test]
fn wipeout_uses_persistent_kinematics_and_sensor_hits_and_restores_origin() {
 let mut h=Harness::new("wipeout-challenge",json!({}),"99","99");h.event("start");
 assert!(h.commands.iter().any(|c|matches!(c,Command::PlayerTeleport{options} if options.position==[-5.,43.6,-5.])));
 h.step(0.1);
 assert!(h.commands.iter().any(|c|matches!(c,Command::PhysicsSpawn{key,body} if key=="wz01" && body.body_type==skate_dynamics::BodyType::Kinematic)));
 h.snapshot["player"]["position"]=json!([0.,43.,0.]);
 h.snapshot["physics"]["bodies"]["wz01"]=json!({"position":[4.5,43.1,0.],"player_overlapping":false});
 h.commands.clear();h.step(0.1);
 assert!(h.commands.iter().any(|c|matches!(c,Command::PhysicsSetLinvel{key,..} if key=="wz01")));
 assert!(!h.commands.iter().any(|c|matches!(c,Command::PhysicsRemove{key} if key=="wz01")));
 h.step(2.1);h.snapshot["physics"]["bodies"]["wo.w1"]=json!({"player_overlapping":true});h.step(0.01);
 assert!(h.text.contains("ELIMINATED"));
 h.commands.clear();h.event("restart");assert!(h.commands.iter().any(|c|matches!(c,Command::PlayerTeleport{options} if options.position==[-5.,43.6,-5.])));
 h.commands.clear();h.event("stop");assert!(h.commands.iter().any(|c|matches!(c,Command::PlayerTeleport{options} if options.position==[0.,2.,0.])));
}
#[test]
fn wipeout_counts_contact_entries_not_frames() {
 let mut h=Harness::new("wipeout-challenge",json!({}),"99","99");h.event("start");h.snapshot["player"]["position"]=json!([0.,43.,0.]);h.step(2.1);
 h.snapshot["physics"]["bodies"]["wz01"]=json!({"position":[4.5,43.1,0.],"player_overlapping":true});h.step(0.01);
 assert!(h.text.contains("Next hazard"));h.step(1.2);assert!(!h.text.contains("ELIMINATED"));
 h.snapshot["physics"]["bodies"]["wz01"]["player_overlapping"]=json!(false);h.step(0.01);
 h.snapshot["physics"]["bodies"]["wz01"]["player_overlapping"]=json!(true);h.step(0.01);assert!(h.text.contains("ELIMINATED"));
}

#[test]
fn simon_uses_settled_body_rotation_not_the_360_in_a_board_flip_name() {
 let mut h=Harness::new("simon-says",json!({"countdown_secs":1,"ride_away":1,"allow_flips":false,"allow_shuvits":false,"allow_ollies":false,"allow_grabs":false,"allow_grinds":false,"allow_manuals_plants":false}),"99","99");
 h.event("start");h.step(1.1);
 h.snapshot["network"]["states"]=json!({"simon.says":{"1":{"round":{"e":h.wire["match"]["e"],"n":1,"l":7,"b":0}}}});
 let spin:u32=h.text.split("Call: ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
 h.snapshot["skaters"]["1"]["landed_spin_degrees"]=json!(0);h.land("360 Flip");h.step(0.1);h.step(1.1);assert_eq!(h.wire["match"]["p"],"copy");
 h.snapshot["skaters"]["1"]["landed_spin_degrees"]=json!(spin);h.land("Tailwalk Frontflip");h.step(0.1);h.step(1.1);assert_eq!(h.wire["match"]["w"],2);
}
