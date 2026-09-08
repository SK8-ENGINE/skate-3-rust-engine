use serde_json::json;
use skate_mods::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(code: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "skate-sdk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(p.join("mods/example")).unwrap();
        let f = Self(p);
        f.code(code);
        std::fs::write(f.0.join("mods/example/mod.json"),serde_json::to_vec(&json!({"id":"example","api":1,"name":"Example","version":"1.0.0","author":"Test","description":"Test","entry":"main.lua","settings":{"count":{"type":"number","label":"Count","description":"Count","min":1,"max":10,"step":1,"default":3}}})).unwrap()).unwrap();
        f
    }
    fn code(&self, s: &str) {
        std::fs::write(self.0.join("mods/example/main.lua"), s).unwrap();
    }
    fn manager(&self) -> Manager {
        let mut m = Manager::new(self.0.join("mods"), self.0.join("settings"));
        m.scan(true);
        m
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let target = self.0.canonicalize().unwrap();
        assert!(target.starts_with(&temp) && target != temp);
        assert!(
            target
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("skate-sdk-")
        );
        let _ = std::fs::remove_dir_all(target);
    }
}
#[test]
fn disabled_by_default_reload_resets_and_removes_stale_commands() {
    let f =
        Fixture::new("local n=0; return {on_update=function() n=n+1; sdk.log(tostring(n)) end}");
    let mut m = f.manager();
    assert!(!m.packages["example"].running());
    m.enable("example", true).unwrap();
    m.dispatch("on_update", json!({}));
    m.dispatch("on_update", json!({}));
    assert_eq!(m.commands.len(), 2);
    m.reload("example");
    assert!(m.commands.is_empty());
    m.dispatch("on_update", json!({}));
    assert!(matches!(&m.commands[0].1,Command::Log{text} if text=="1"));
    m.enable("example", false).unwrap();
    assert!(m.commands.is_empty());
    assert!(!m.packages["example"].running());
}
#[test]
fn failed_callback_discards_all_commands_and_stops_mod() {
    let f =
        Fixture::new("return {on_load=function() sdk.ui.text('test','before'); error('fail') end}");
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    assert!(m.commands.is_empty());
    assert!(!m.packages["example"].running());
    assert!(
        m.packages["example"]
            .error
            .as_ref()
            .unwrap()
            .contains("fail")
    );
}
#[test]
fn instruction_and_memory_limits() {
    for code in [
        "return {on_load=function() while true do end end}",
        "return {on_load=function() local t={} for i=1,100000 do t[i]=string.rep('a',10000) end end}",
    ] {
        let f = Fixture::new(code);
        let mut m = f.manager();
        m.enable("example", true).unwrap();
        assert!(m.packages["example"].error.is_some());
        assert!(!m.packages["example"].running());
    }
}
#[test]
fn libraries_are_restricted() {
    let f = Fixture::new(
        "assert(io==nil and os==nil and debug==nil and package==nil and require==nil and load==nil and pcall==nil and coroutine==nil); return {}",
    );
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    assert!(
        m.packages["example"].running(),
        "{:?}",
        m.packages["example"].error
    );
}
#[test]
fn immediate_settings_persist_and_validate() {
    let f = Fixture::new(
        "return {on_settings=function(e) sdk.log(tostring(sdk.settings.count)..':'..e.key) end}",
    );
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    assert!(m.setting("example", "count", json!(11)).is_err());
    m.setting("example", "count", json!(7)).unwrap();
    assert!(matches!(&m.commands.last().unwrap().1,Command::Log{text} if text=="7:count"));
    let m = f.manager();
    assert_eq!(m.packages["example"].settings["count"], json!(7));
    assert!(m.packages["example"].running());
}
#[test]
fn files_changed_removed_and_invalid_manifest() {
    let f = Fixture::new("return {}");
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    f.code("return {on_load=function() sdk.log('new') end}");
    m.scan(true);
    assert!(matches!(&m.commands[0].1,Command::Log{text} if text=="new"));
    std::fs::write(f.0.join("mods/example/mod.json"), "{}").unwrap();
    m.scan(true);
    assert!(m.packages.is_empty());
    assert!(!m.diagnostics.is_empty());
}
#[test]
fn traversal_and_unknown_dependencies_rejected() {
    let f = Fixture::new("return {}");
    assert!(read_bounded(&f.0, "../anything", 100).is_err());
    assert!(read_bounded(&f.0, "C:/Windows/win.ini", 100).is_err());
    let mut v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.0.join("mods/example/mod.json")).unwrap()).unwrap();
    v["dependencies"] = json!(["other"]);
    assert!(serde_json::from_value::<Manifest>(v).is_err());
}
#[test]
fn examples_load_and_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    if !root.exists() {
        panic!("Examples missing");
    }
    let f = Fixture::new("return {}");
    let mut m = Manager::new(root, f.0.join("examples-settings"));
    m.snapshot = json!({"player":{"position":[0,0,0],"velocity":[0,0,0],"state":100,"on_board":true},"keys":{},"map":{"name":"test","generation":0},"tick":1,"actions":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]});
    m.scan(true);
    assert!(m.diagnostics.is_empty(), "{:?}", m.diagnostics);
    let ids: Vec<_> = m.packages.keys().cloned().collect();
    assert_eq!(ids, vec!["community.native-trainer"]);
    for id in ids {
        m.enable(&id, true).unwrap();
        for _ in 0..5 {
            m.dispatch("on_update", json!({"dt":0.016}));
            m.dispatch("on_fixed_update", json!({"dt":0.016}));
            m.commands.clear();
        }
        assert!(m.packages[&id].running(), "{:?}", m.packages[&id].error);
    }
}

#[test]
fn timers_are_ordered_replaced_and_reset() {
    let f = Fixture::new(
        "return {on_load=function() sdk.time.after('z',0,function() sdk.log('z') end); sdk.time.after('a',0,function() sdk.log('old') end); sdk.time.after('a',0,function() sdk.log('a') end) end}",
    );
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    m.dispatch("on_update", json!({"dt":0.1}));
    let logs: Vec<_> = m
        .commands
        .iter()
        .filter_map(|(_, c)| {
            if let Command::Log { text } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(logs, vec!["a", "z"]);
    m.commands.clear();
    m.dispatch("on_update", json!({"dt":0.1}));
    assert!(m.commands.is_empty());
    m.reload("example");
    m.dispatch("on_update", json!({"dt":0.1}));
    assert_eq!(m.commands.len(), 2);
}
#[test]
fn one_mod_error_does_not_stop_other_mods() {
    let f = Fixture::new("return {on_update=function() error('oops') end}");
    let other = f.0.join("mods/other");
    std::fs::create_dir_all(&other).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.0.join("mods/example/mod.json")).unwrap()).unwrap();
    manifest["id"] = json!("other");
    std::fs::write(
        other.join("mod.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(
        other.join("main.lua"),
        "return {on_update=function() sdk.log('alive') end}",
    )
    .unwrap();
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    m.enable("other", true).unwrap();
    m.dispatch("on_update", json!({"dt":0.1}));
    assert!(!m.packages["example"].running());
    assert!(m.packages["other"].running());
    assert_eq!(m.commands.len(), 1);
}
#[test]
fn duplicate_ids_and_unload_failure_cleanup() {
    let f = Fixture::new("return {on_unload=function() sdk.log('discard'); error('unload') end}");
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    m.enable("example", false).unwrap();
    assert!(m.commands.is_empty());
    assert!(
        m.packages["example"]
            .error
            .as_ref()
            .unwrap()
            .contains("unload")
    );
    let other = f.0.join("mods/other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::copy(f.0.join("mods/example/mod.json"), other.join("mod.json")).unwrap();
    std::fs::write(other.join("main.lua"), "return {}").unwrap();
    m.scan(true);
    assert!(m.packages.is_empty());
    assert!(m.diagnostics.iter().any(|s| s.contains("Duplicate")));
}

#[test]
fn schema_upgrade_keeps_only_compatible_keys() {
    let f = Fixture::new("return {}");
    let mut m = f.manager();
    m.setting("example", "count", json!(7)).unwrap();
    let path = f.0.join("mods/example/mod.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    v["version"] = json!("2.0.0");
    v["settings"]["count"]["max"] = json!(5);
    v["settings"]["renamed"] = v["settings"]["count"].clone();
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    m.scan(true);
    assert_eq!(m.packages["example"].settings["count"], json!(3));
    assert_eq!(m.packages["example"].settings["renamed"], json!(3));
}
#[test]
fn changed_source_waits_for_debounce() {
    let f = Fixture::new("return {}");
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    f.code("return {on_load=function() sdk.log('changed') end}");
    std::thread::sleep(std::time::Duration::from_millis(510));
    m.scan(false);
    assert!(m.commands.is_empty());
    std::thread::sleep(std::time::Duration::from_millis(510));
    m.scan(false);
    assert!(m.commands.is_empty());
    std::thread::sleep(std::time::Duration::from_millis(510));
    m.scan(false);
    assert_eq!(m.commands.len(), 1);
}

#[test]
fn trainer_commands_validate_and_follow_settings() {
    let f = Fixture::new(
        "return {on_load=function() sdk.trainer.apply{pop=2,push_speed=3,wobble=0} end, on_settings=function() sdk.trainer.apply{pop=sdk.settings.count} end}",
    );
    let mut m = f.manager();
    m.enable("example", true).unwrap();
    assert!(
        matches!(&m.commands[0].1,Command::Trainer{tuning} if tuning.pop==2. && tuning.push_speed==3. && tuning.wobble==0. && tuning.braking==1.)
    );
    m.setting("example", "count", json!(4)).unwrap();
    assert!(matches!(&m.commands.last().unwrap().1,Command::Trainer{tuning} if tuning.pop==4.));
    m.setting("example", "count", json!(5)).unwrap();
    assert!(!m.packages["example"].running());
    assert!(m.commands.is_empty());
    assert!(
        !TrainerTuning {
            pop: f32::NAN,
            ..Default::default()
        }
        .valid()
    );
    m.enable("example", false).unwrap();
    assert!(m.retired.contains(&"example".into()));
}
