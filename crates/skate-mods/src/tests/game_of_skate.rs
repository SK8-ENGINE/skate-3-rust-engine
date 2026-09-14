//! The mod is tested through the public Lua VM; no game rules live in the host.
use serde_json::{json, Value};
use crate::{vm::Vm, Command, validate_package};
use std::{collections::BTreeMap, path::PathBuf};

struct Game { vm: Vm, snapshot: Value, state: Value, overlays: BTreeMap<String, String> }
impl Game {
    fn new(ids: &[&str]) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples/game-of-skate");
        let manifest = validate_package(&root).unwrap();
        let mut snapshot = json!({"player":{},"skaters":{},"network":{"active":ids.len()>1,"local_id":ids[0],"host_id":ids[0],"is_host":true,"players":ids,"states":{}}});
        for id in ids { snapshot["skaters"][*id] = json!({"name":format!("Skater {id}"),"landing_seq":7,"landed_trick":"Old kickflip","trick_seq":100,"trick":"Old kickflip","bail_seq":2,"bailing":false}); }
        let vm = Vm::new(&root, &manifest, &BTreeMap::from([("copy_seconds".into(),json!(15)),("allow_ollie".into(),json!(false))]), &snapshot).unwrap();
        let mut g = Self {vm,snapshot,state:Value::Null,overlays:BTreeMap::new()};
        g.vm.call("on_load",json!({}),&g.snapshot).unwrap();
        let start=g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"start"}),&g.snapshot).unwrap();
        for c in start {if let Command::NetworkState{value,..}=c {g.state=value;}}
        g.tick(0.);
        g
    }
    fn tick(&mut self, dt: f64) {
        self.vm.call("on_update",json!({"dt":dt}),&self.snapshot).unwrap();
        let commands = self.vm.call("on_fixed_update",json!({"dt":dt}),&self.snapshot).unwrap();
        for c in commands {
            assert!(c.validate());
            match c {
                Command::NetworkState {key,value} if key == "skate" => { assert!(serde_json::to_vec(&value).unwrap().len()<=512); self.state=value; }
                Command::Overlay {key,text} => {self.overlays.insert(key,text);}
                _ => {}
            }
        }
    }
    fn land(&mut self, id: &str, name: &str) {
        let s = &mut self.snapshot["skaters"][id];
        s["landing_seq"] = json!(s["landing_seq"].as_u64().unwrap()+1);
        s["landed_trick"] = json!(name);
        s["bailing"] = json!(false);
        self.tick(0.02);
    }
    fn bail(&mut self,id:&str) {
        let s=&mut self.snapshot["skaters"][id];
        s["bail_seq"]=json!(s["bail_seq"].as_u64().unwrap()+1);
        s["bailing"]=json!(true);
        self.tick(0.02);
    }
}

#[test]
fn announcements_and_old_landings_do_not_count_and_solo_needs_second_landing() {
    let mut g = Game::new(&["0"]);
    g.snapshot["skaters"]["0"]["trick_seq"]=json!(101);
    g.snapshot["skaters"]["0"]["trick"]=json!("Kickflip");
    g.tick(1.);
    assert_eq!(g.state["p"],"s");
    g.land("0","Kickflip");
    assert_eq!(g.state["p"],"c");
    assert_eq!(g.state["d"],"0");
    g.tick(1.);
    assert_eq!(g.state["p"],"c");
    g.land("0","Kickflip");
    assert_eq!(g.state["p"],"r");
    assert_eq!(g.state["d"],"1");
    assert!(g.overlays["skate_status"].starts_with("Matched"));
    g.tick(3.);
    assert_eq!(g.state["p"],"s");
    assert_eq!(g.state["n"],2);
}

#[test]
fn wrong_trick_bail_and_timeout_each_award_only_one_letter() {
    let mut g = Game::new(&["0"]);
    g.land("0","Kickflip");
    g.land("0","Heelflip");
    assert_eq!(g.state["l"],"1");
    g.tick(3.);
    g.land("0","Kickflip");
    g.bail("0");
    assert_eq!(g.state["l"],"2");
    g.tick(1.);
    assert_eq!(g.state["l"],"2");
    g.tick(2.);
    g.land("0","Kickflip");
    g.tick(16.);
    assert_eq!(g.state["l"],"3");
}

#[test]
fn remote_pre_turn_results_are_consumed_and_remote_bails_are_not_missed() {
    let mut g=Game::new(&["1","2","3"]);
    g.land("2","Kickflip");
    g.land("1","Kickflip");
    g.tick(1.);
    assert_eq!(g.state["d"],"000");
    g.land("2","Kickflip");
    assert_eq!(g.state["d"],"010");
    // A remote can already be standing again when the latest packet arrives.
    g.snapshot["skaters"]["3"]["bail_seq"]=json!(3);
    g.tick(0.02);
    assert_eq!(g.state["p"],"r");
    assert_eq!(g.state["l"],"001");
    g.tick(3.);
    assert_eq!(g.state["a"],"2");
    assert_eq!(g.state["p"],"s");
}

#[test]
fn ten_player_state_fits_lua_network_budget() {
    let ids: Vec<_>=(0..10).map(|i| format!("1844674407370955160{i}")).collect();
    let refs: Vec<_>=ids.iter().map(String::as_str).collect();
    let mut g=Game::new(&refs);
    g.land(refs[0],&"x".repeat(64));
    for id in &refs[1..] { g.land(id,&"x".repeat(64)); }
    assert_eq!(g.state["p"],"r");
}

#[test]
fn departing_setter_is_replaced_and_new_peer_old_landing_is_ignored() {
    let mut g=Game::new(&["1","2","3"]);
    g.snapshot["network"]["players"]=json!(["1","3"]);
    g.land("1","Kickflip");
    g.snapshot["network"]["players"]=json!(["1","2","3"]);
    g.tick(1.);
    assert_eq!(g.state["d"],"000");
    g.snapshot["network"]["players"]=json!(["2","3"]);
    g.snapshot["network"]["local_id"]=json!("2");
    g.tick(1.);
    assert_eq!(g.state["p"],"s");
    assert_eq!(g.state["a"],"2");
}

#[test]
fn menu_start_stop_is_explicit_and_clients_cannot_start_the_host_game() {
    let mut g=Game::new(&["1","2"]);
    g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"stop"}),&g.snapshot).unwrap();
    g.tick(2.);
    assert_eq!(g.state["p"],"i");
    g.land("1","Kickflip");
    assert_eq!(g.state["p"],"i");
    g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"start"}),&g.snapshot).unwrap();
    g.tick(1.1);
    assert_eq!(g.state["p"],"s");
    g.snapshot["network"]["is_host"]=json!(false);
    let commands=g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"start"}),&g.snapshot).unwrap();
    assert!(!commands.iter().any(|c|matches!(c,Command::NetworkState{..})));
}

#[test]
fn paused_ui_refreshes_client_host_and_solo_actions_without_physics_ticks() {
    let mut g=Game::new(&["1","2"]);
    g.snapshot["paused"]=json!(true);
    g.snapshot["network"]["is_host"]=json!(false);
    let menu = |commands: Vec<Command>| commands.into_iter().find_map(|c| match c {
        Command::UiMenu{options,..} => Some(options), _=>None
    }).expect("menu refreshed");
    let client=menu(g.vm.call("on_ui_update",json!({"dt":10.,"paused":true}),&g.snapshot).unwrap());
    assert_eq!(client.section.as_deref(),Some("Gamemodes"));
    assert_eq!(client.title,"SKATE");
    assert!(!client.items[0].enabled && !client.items[1].enabled);
    assert!(client.items[0].description.contains("Only the multiplayer host"));
    g.snapshot["network"]["is_host"]=json!(true);
    let host=menu(g.vm.call("on_ui_update",json!({"dt":10.,"paused":true}),&g.snapshot).unwrap());
    assert!(host.items[0].enabled && !host.items[1].enabled);
    let started=menu(g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"start"}),&g.snapshot).unwrap());
    assert!(!started.items[0].enabled && started.items[1].enabled);
    let stopped=menu(g.vm.call("on_event",json!({"name":"menu_action","menu":"session","item":"stop"}),&g.snapshot).unwrap());
    assert!(stopped.items[0].enabled && !stopped.items[1].enabled);
    g.snapshot["network"]["is_host"]=json!(false);
    g.vm.call("on_ui_update",json!({"dt":1.,"paused":true}),&g.snapshot).unwrap();
    g.snapshot["network"]["active"]=json!(false);
    let solo=menu(g.vm.call("on_ui_update",json!({"dt":1.,"paused":true}),&g.snapshot).unwrap());
    assert!(solo.items[0].enabled && !solo.items[1].enabled);
}
