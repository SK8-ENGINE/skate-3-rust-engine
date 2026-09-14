//! Exercise independent Lua VMs exchanging public network snapshots.
use crate::{vm::Vm, Command, validate_package};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};
const MOD: &str = "examples.game-of-skate";
struct Peer {
    id: String, vm: Vm, overlays: BTreeMap<String,String>,
    suspended: bool, camera: Option<String>, teleports: Vec<crate::TeleportOptions>,
}
struct Match {
    peers: Vec<Peer>, wire: Value, skaters: Value, host: String,
}
impl Match {
    fn new(ids: &[&str]) -> Self {
        let mut g=Self {peers:vec![],wire:json!({}),skaters:json!({}),host:ids[0].into()};
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples/game-of-skate");
        let manifest=validate_package(&root).unwrap();
        for (i,id) in ids.iter().enumerate() {
            g.skaters[*id]=json!({"position":[i as f32*10.,2.,3.],"heading":0.7,"name":format!("Skater {id}"),"landing_seq":7,"landed_trick":"Old kickflip","bail_seq":2,"bailing":false});
        }
        for id in ids {
            let snapshot=json!({"player":g.skaters[*id],"skaters":g.skaters,"network":{"active":ids.len()>1,"local_id":id,"host_id":ids[0],"is_host":*id==ids[0],"players":ids,"states":{}}});
            let vm=Vm::new(&root,&manifest,&BTreeMap::from([("copy_seconds".into(),json!(15)),("allow_ollie".into(),json!(false))]),&snapshot).unwrap();
            g.peers.push(Peer{id:(*id).into(),vm,overlays:BTreeMap::new(),suspended:false,camera:None,teleports:vec![]});
        }
        for i in 0..g.peers.len() {g.call(i,"on_load",json!({}));}
        g
    }
    fn snapshot(&self,i:usize)->Value {
        let id=&self.peers[i].id;
        json!({"player":self.skaters[id],"skaters":self.skaters,"network":{"active":self.peers.len()>1,"local_id":id,"host_id":self.host,"is_host":*id==self.host,"players":self.peers.iter().map(|p|&p.id).collect::<Vec<_>>(),"states":{MOD:self.wire}}})
    }
    fn call(&mut self,i:usize,callback:&str,arg:Value) {
        let snapshot=self.snapshot(i);
        let commands=self.peers[i].vm.call(callback,arg,&snapshot).unwrap();
        for c in commands {
            assert!(c.validate(),"{c:?}");
            let peer=&mut self.peers[i];
            match c {
                Command::NetworkState{key,value}=> {
                    let bytes=serde_json::to_vec(&value).unwrap();assert!(bytes.len()<=512,"{} bytes for {key}",bytes.len());
                    self.wire[&peer.id][&key]=serde_json::from_slice(&bytes).unwrap();
                }
                Command::PlayerSuspend{suspended}=>peer.suspended=suspended,
                Command::CameraWatch{peer:target}=>peer.camera=target,
                Command::PlayerTeleport{options}=>peer.teleports.push(options),
                Command::Overlay{key,text}=>{peer.overlays.insert(key,text);}
                _=>{}
            }
        }
    }
    fn event(&mut self,i:usize,item:&str){self.call(i,"on_event",json!({"name":"menu_action","menu":"session","item":item}));}
    fn step(&mut self) {
        for i in 0..self.peers.len(){self.call(i,"on_update",json!({"dt":0.05}));self.call(i,"on_ui_update",json!({}));self.call(i,"on_fixed_update",json!({"dt":0.05}));}
    }
    fn settle(&mut self){for _ in 0..24 {self.step();}}
    fn start(&mut self){self.event(0,"start");self.settle();}
    fn state(&self)->&Value {&self.wire[&self.host]["skate"]}
    fn land(&mut self,id:&str,trick:&str) {
        let p=&mut self.skaters[id];p["landing_seq"]=json!(p["landing_seq"].as_u64().unwrap()+1);p["landed_trick"]=json!(trick);p["bailing"]=json!(false);self.step();
    }
}
#[test]
fn independent_clients_receive_host_first_turn_and_mirror_active_camera() {
    let mut g=Match::new(&["99","1","2"]);g.start();
    assert_eq!(g.wire["99"]["origin"]["ids"],json!(["99","1","2"]));
    assert_eq!(g.state()["p"],"s");assert_eq!(g.state()["c"],1);
    assert!(!g.peers[0].suspended);assert!(g.peers[1].suspended && g.peers[2].suspended);
    assert_eq!(g.peers[1].camera.as_deref(),Some("99"));
    assert!(g.peers[1].overlays["skate_status"].contains("Skater 99: land a trick"));
    g.land("99","Kickflip");g.settle();
    assert_eq!(g.state()["p"],"c");assert_eq!(g.state()["c"],2);
    assert!(g.peers[0].suspended && g.peers[2].suspended);assert!(!g.peers[1].suspended);
    assert_eq!(g.peers[0].camera.as_deref(),Some("1"));assert_eq!(g.peers[1].camera,None);
    assert!(g.peers[1].overlays["skate_status"].contains("You: copy Kickflip"));
    assert_eq!(g.peers[1].teleports.last().unwrap().position,[0.,2.,3.]);
    assert_eq!(g.peers[1].teleports.last().unwrap().heading,Some(0.7));
    g.land("1","Kickflip");g.settle();assert_eq!(g.state()["c"],3);
    assert_eq!(g.state()["d"],"010");assert_eq!(g.peers[0].camera.as_deref(),Some("2"));
    assert_eq!(g.peers[2].teleports.last().unwrap().position,[0.,2.,3.]);
    g.land("2","Heelflip");assert_eq!(g.state()["p"],"r");assert_eq!(g.state()["l"],"001");
}
#[test]
fn pre_turn_results_announcements_and_old_landings_cannot_complete_a_turn() {
    let mut g=Match::new(&["99","1"]);g.start();
    g.skaters["99"]["trick"]=json!("Kickflip");g.step();assert_eq!(g.state()["p"],"s");
    g.land("1","Kickflip");g.land("99","Kickflip");g.settle();assert_eq!(g.state()["p"],"c");assert_eq!(g.state()["d"],"00");
    g.land("1","Kickflip");assert_eq!(g.state()["p"],"r");assert_eq!(g.state()["d"],"01");
}
#[test]
fn solo_resets_between_setting_and_copying_and_needs_a_second_landing() {
    let mut g=Match::new(&["0"]);g.start();g.land("0","Kickflip");g.settle();
    assert_eq!(g.state()["p"],"c");assert_eq!(g.state()["d"],"0");assert_eq!(g.peers[0].teleports.len(),2);
    g.land("0","Kickflip");assert_eq!(g.state()["p"],"r");assert_eq!(g.state()["d"],"1");
}
#[test]
fn bail_and_timeout_award_one_letter_to_the_active_copier_only() {
    let mut g=Match::new(&["99","1","2"]);g.start();g.land("99","Kickflip");g.settle();
    g.skaters["1"]["bail_seq"]=json!(3);g.step();g.settle();
    assert_eq!(g.state()["l"],"010");assert_eq!(g.state()["c"],3);
    for _ in 0..310 {g.step();}
    assert_eq!(g.state()["l"],"011");assert_eq!(g.state()["p"],"r");
}
#[test]
fn stop_unload_and_disconnect_restore_player_and_camera() {
    let mut g=Match::new(&["99","1"]);g.start();g.event(1,"stop");g.step();assert_eq!(g.state()["p"],"s");
    g.event(0,"stop");g.step();
    for p in &g.peers{assert!(!p.suspended);assert!(p.camera.is_none());}
    g.start();g.call(1,"on_unload",json!({}));assert!(!g.peers[1].suspended);assert!(g.peers[1].camera.is_none());
}
#[test]
fn absent_acknowledgement_blocks_play_and_reports_the_missing_setup() {
    let mut g=Match::new(&["99","1"]);g.event(0,"start");
    for _ in 0..320 {g.call(0,"on_update",json!({"dt":0.05}));g.call(0,"on_fixed_update",json!({"dt":0.05}));}
    assert_eq!(g.state()["p"],"i");assert!(g.state()["error"].as_str().unwrap().contains("acknowledge"));assert!(!g.peers[0].suspended);
}
#[test]
fn ten_large_peer_ids_and_long_tricks_fit_each_network_record() {
    let ids:Vec<_>=(0..10).map(|i|format!("1844674407370955160{i}")).collect();
    let refs:Vec<_>=ids.iter().map(String::as_str).collect();let mut g=Match::new(&refs);g.start();
    g.land(refs[0],&"x".repeat(64));
    for id in &refs[1..]{g.settle();g.land(id,&"x".repeat(64));}
    assert_eq!(g.state()["p"],"r");
}

#[test]
fn late_joiners_spectate_and_disconnected_copiers_do_not_block_the_round() {
    let mut g=Match::new(&["99","1","2"]);
    let late=g.peers.pop().unwrap();g.start();g.peers.push(late);g.settle();
    assert_eq!(g.wire["99"]["origin"]["ids"],json!(["99","1"]));
    assert!(g.peers[2].suspended);assert_eq!(g.peers[2].camera.as_deref(),Some("99"));
    g.land("99","Kickflip");g.settle();g.peers.remove(1);g.step();
    assert_eq!(g.state()["p"],"r");
    g.event(0,"stop");g.step();assert!(g.peers.iter().all(|p|!p.suspended));
}
