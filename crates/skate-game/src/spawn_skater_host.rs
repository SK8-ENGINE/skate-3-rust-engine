//! FE 62 SpawnSkater's `SkaterSpawner` natives (registrar sub_825B8F40, b543) and its picker callbacks while it is on
//! top. Details: docs/hails-additions/31-retail-menus.md#spawnskater-fe-62
use crate::apt_vm::{to_integer, Value, Vm};
use skate_core::spawn_skater::{Action, Group, SpawnSkater, SpawnSkaterRules};

#[derive(Clone, Default)]
pub struct SpawnSkaterHost {
    pub screen: SpawnSkater,
    pub rules: SpawnSkaterRules,
    /// FE 62 is on top (its picker callbacks are ours).
    pub open: bool,
    /// Screen states to post to the movie, in order.
    pub states: Vec<i32>,
    /// What the picks and keys asked of the game, in order.
    pub actions: Vec<Action>,
    pub sounds: Vec<String>,
}

impl SpawnSkaterHost {
    pub fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if !self.open {
            return Ok(None);
        }
        let row = || args.get(1).map_or(-1, to_integer);
        Ok(Some(match (object, method) {
            // 825B8FE8: [rowCount, names[], isHeader[], spawned[]].
            ("SkaterSpawner", "GetSkatersData") => {
                let rows = self.screen.rows();
                let names = vm.array(rows.iter().map(|r| Value::Text(r.label.clone())).collect())?;
                let headers = vm.array(rows.iter().map(|r| Value::Bool(r.header)).collect())?;
                let spawned = vm.array(rows.iter().map(|r| Value::Bool(r.spawned)).collect())?;
                let data = vec![Value::Number(rows.len() as f64), Value::Object(names), Value::Object(headers), Value::Object(spawned)];
                Value::Object(vm.array(data)?)
            }
            ("SkaterSpawner", "GetScreenMode") => Value::Number(self.screen.mode.code() as f64),
            ("SkaterSpawner", "GetLastMenuHighlightIndex") => Value::Number(self.screen.highlight.map_or(-1.0, |h| h as f64)),
            // NOT RETAIL YET (inferred): header rows are not pickable.
            ("Game", "OnMenuGetItemEnabled") => Value::Bool(row() >= 0 && self.screen.entry(row() as usize).is_some()),
            ("Game", "OnMenuHilight") => {
                if row() >= 0 {
                    let out = self.screen.hilight(&self.rules, row() as usize);
                    self.take(out);
                }
                Value::Undefined
            }
            ("Game", "OnMenuSelect") => {
                if row() >= 0 {
                    let out = self.screen.select(&self.rules, row() as usize);
                    self.take(out);
                }
                Value::Undefined
            }
            _ => return Ok(None),
        }))
    }

    pub fn take(&mut self, out: skate_core::spawn_skater::Outcome) {
        self.states.extend(out.states);
        self.actions.extend(out.actions);
        self.sounds.extend(out.sound);
    }
}

/// The characters the living world can put on a line: the free-roam pool of the export's skater profiles (any map,
/// like retail's list).
pub fn callable(asset_root: &std::path::Path) -> Vec<String> {
    std::fs::read(asset_root.join("private/living_world/skater_profiles.json"))
        .ok()
        .and_then(|b| skate_data::living_world::skater_characters(&b, &[]).ok())
        .map(|c| c.into_iter().map(|c| c.key).collect())
        .unwrap_or_default()
}

/// What FE 62 asked of the game (companions, the player's skater, the options and preview), for the systems that own
/// them and for mods.
#[derive(bevy::prelude::Message, Clone, Debug, PartialEq, Eq)]
pub struct SpawnSkaterRequest(pub Action);

/// The groups from our character library (custom_models): You (the stock skater = retail save slot 0), retail pros
/// (category Pro) and IP skaters (Special), then the player's own imported characters (ours, NOT RETAIL: retail has
/// no such group; its teammates and community groups have no source here yet). Retail headers (b544).
/// Call Skater lists only pros that are `callable` (in the living world's skater pool; retail mode 100 keeps pro-class
/// records with attribute 0xCABC362B1B05CA19, b544; pool membership stands in for the attribute, NOT RETAIL YET).
pub fn groups(characters: Option<&crate::custom_models::CustomModels>, callable: &[String]) -> Vec<Group> {
    use skate_core::spawn_skater::{Entry, Mode, Skater};
    let group = |id: &str, header: &str, modes: &[Mode]| Group { id: id.into(), header: header.into(), modes: modes.to_vec(), entries: Vec::new() };
    // NOT RETAIL YET: the You row's label (82558BD0's vt+0xC) is not read.
    let mut you = group("you", "ID_FREESKATE_YOU_HEADER", &[Mode::Freeskate]);
    you.entries.push(Entry { skater: Skater::Own, label: "#You".into() });
    let mut pros = group("pros", "ID_CIC_PRO_SKATERS", &[Mode::Freeskate]);
    let mut call = group("pros_call", "ID_CIC_PRO_SKATERS", &[Mode::SkateWith]);
    let mut ip = group("ip", "ID_CIC_IP_SKATERS", &[Mode::Freeskate]);
    let mut own = group("custom", "#Custom", &[Mode::Freeskate]);
    for (id, name, native) in characters.into_iter().flat_map(|c| c.roster()) {
        let id = native.map_or(id, |(key, _)| key);
        let entry = Entry { skater: Skater::Character(id.into()), label: format!("#{name}") };
        if matches!(native, Some((_, "Pro"))) && callable.iter().any(|k| k == id) {
            call.entries.push(entry.clone());
        }
        match native {
            Some((_, "Pro")) => pros.entries.push(entry),
            Some(_) => ip.entries.push(entry),
            None => own.entries.push(entry),
        }
    }
    vec![you, call, pros, ip, own]
}
