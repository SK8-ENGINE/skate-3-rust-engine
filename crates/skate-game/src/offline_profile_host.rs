//! The Skate Profile screen's natives (b308): Progression (0x825AFBA8) and Stats (0x825AB630) objects.
//! Details: docs/hails-additions/31-retail-menus.md#skate-profile-natives
use crate::apt_vm::{Value, Vm};
use skate_core::offline_profile::{OfflineProfile, ProfileData, ProfileOverall, ProfileRules, StatRow, StatValue};

#[derive(Clone, Default)]
pub struct OfflineProfileHost {
    pub screen: OfflineProfile,
    pub rules: ProfileRules,
    pub data: ProfileData,
    /// Screen states to post to the movie, in order (held until the movie loaded).
    pub states: Vec<i32>,
    /// The movie reported OnScreenLoaded.
    pub loaded: bool,
    /// The members the movie called (tests).
    #[cfg(test)]
    pub asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl OfflineProfileHost {
    /// Init 0x8264C530: tab 0 and its page.
    pub fn open(&mut self) {
        let (screen, states) = OfflineProfile::open(&self.rules);
        self.screen = screen;
        self.states = states;
        self.loaded = false;
    }

    /// A Progression / Stats member call; None for anything else.
    pub fn call(&self, vm: &mut Vm, object: &str, method: &str) -> Result<Option<Value>, String> {
        #[cfg(test)]
        if object == "Progression" || object == "Stats" {
            self.asked.lock().unwrap().push(format!("{object}.{method}"));
        }
        Ok(Some(match (object, method) {
            // 0x825AC888: the FE 61 screen's tab index (+0x3C).
            ("Stats", "GetCareerProfileSelectedTab") => Value::Number(self.screen.tab as f64),
            ("Stats", "GetStatsChunk") => rows(vm, &self.data.stats)?,
            ("Stats", "GetPersonalBestChunk") => rows(vm, &self.data.personal_bests)?,
            ("Progression", "GetOverallCareerData") => overall(vm, &self.data.career)?,
            ("Progression", "GetOverallChallengeData") => overall(vm, &self.data.challenges)?,
            // NOT RETAIL YET: no career milestones or summary groups exist in the engine, so the lists are empty. The
            // movie's layouts (its own constants): summary groups = [milestones (SUMMARY_GROUP_MILESTONES 0),
            // challenges (_CHALLENGES 1)], each a list of groups; dots = a list of [percent (DOT_DATA_PERCENT 0),
            // boards (DOT_DATA_BOARDS 1)].
            ("Progression", "GetMilestoneDotData") => Value::Object(vm.array(Vec::new())?),
            ("Progression", "GetSummaryGroupData") => {
                let groups = vec![Value::Object(vm.array(Vec::new())?), Value::Object(vm.array(Vec::new())?)];
                Value::Object(vm.array(groups)?)
            }
            _ => return Ok(None),
        }))
    }
}

/// [title, icon, current, total] (the movie clamps current to total).
fn overall(vm: &mut Vm, o: &ProfileOverall) -> Result<Value, String> {
    let items = vec![Value::Text(o.title.clone()), Value::Text(o.icon.clone()), Value::Number(o.current), Value::Number(o.total)];
    Ok(Value::Object(vm.array(items)?))
}

/// One [label, value] array per row.
fn rows(vm: &mut Vm, rows: &[StatRow]) -> Result<Value, String> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let value = match &r.value {
            StatValue::Number(n) => Value::Number(*n),
            StatValue::Text(t) => Value::Text(t.clone()),
        };
        out.push(Value::Object(vm.array(vec![Value::Text(r.label.clone()), value])?));
    }
    Ok(Value::Object(vm.array(out)?))
}

/// Mods' Skate Profile changes (`sdk.menus.profile`), applied in order over the engine's page data; `generation`
/// tells the stage to rebuild.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct ProfileMods {
    layers: Vec<(String, String, String, skate_mods::menus::ProfileOptions)>,
    pub generation: u64,
}

impl ProfileMods {
    pub(crate) fn set(&mut self, owner: &str, page: &str, key: &str, options: Option<skate_mods::menus::ProfileOptions>) {
        self.layers.retain(|(o, p, k, _)| !(o == owner && p == page && k == key));
        if let Some(options) = options {
            self.layers.push((owner.to_owned(), page.to_owned(), key.to_owned(), options));
        }
        self.generation += 1;
    }
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        let before = self.layers.len();
        self.layers.retain(|(o, ..)| o != owner);
        if self.layers.len() != before {
            self.generation += 1;
        }
    }
    pub(crate) fn clear(&mut self) {
        if !self.layers.is_empty() {
            self.layers.clear();
            self.generation += 1;
        }
    }
    /// The base data with every mod layer applied.
    pub(crate) fn apply(&self, base: &ProfileData) -> ProfileData {
        use skate_mods::menus::ProfileValue;
        let mut data = base.clone();
        // Row order: explicit orders first (ascending, stable), the rest after in the order they came.
        let mut orders: std::collections::BTreeMap<(String, String), i32> = Default::default();
        let mut hidden: std::collections::BTreeSet<(String, String)> = Default::default();
        for (_, page, key, o) in &self.layers {
            let bar = match page.as_str() {
                "career" => Some(&mut data.career),
                "challenges" => Some(&mut data.challenges),
                _ => None,
            };
            if let Some(bar) = bar {
                if let Some(t) = &o.title {
                    bar.title = t.clone();
                }
                if let Some(i) = &o.icon {
                    bar.icon = i.clone();
                }
                bar.current = o.current.unwrap_or(bar.current);
                bar.total = o.total.unwrap_or(bar.total);
                continue;
            }
            let table = if page == "stats" { &mut data.stats } else { &mut data.personal_bests };
            let at = match table.iter().position(|r| r.id == *key) {
                Some(at) => at,
                None => {
                    table.push(StatRow { id: key.clone(), label: String::new(), value: StatValue::Text(String::new()) });
                    table.len() - 1
                }
            };
            let row = &mut table[at];
            if let Some(l) = &o.label {
                row.label = l.clone();
            }
            match &o.value {
                Some(ProfileValue::Number(n)) => row.value = StatValue::Number(*n),
                Some(ProfileValue::Text(t)) => row.value = StatValue::Text(t.clone()),
                None => {}
            }
            let id = (page.clone(), key.clone());
            if let Some(order) = o.order {
                orders.insert(id.clone(), order);
            }
            // `hidden` cannot be unset (a plain flag): a hide stays until its layer goes.
            if o.hidden {
                hidden.insert(id);
            }
        }
        for (page, table) in [("stats", &mut data.stats), ("personal_bests", &mut data.personal_bests)] {
            let key = |r: &StatRow| (page.to_owned(), r.id.clone());
            table.retain(|r| !hidden.contains(&key(r)));
            table.sort_by_key(|r| orders.get(&key(r)).map_or((1, 0), |o| (0, *o)));
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_mods::menus::{ProfileOptions, ProfileValue};

    #[test]
    fn mod_layers_fill_bars_and_rows_and_go_with_their_mod() {
        let mut mods = ProfileMods::default();
        let opts = |f: fn(&mut ProfileOptions)| {
            let mut o = ProfileOptions::default();
            f(&mut o);
            Some(o)
        };
        mods.set("a", "career", "c", opts(|o| { o.title = Some("#Career".into()); o.current = Some(3.0); o.total = Some(10.0) }));
        mods.set("a", "stats", "flips", opts(|o| { o.label = Some("#Flips".into()); o.value = Some(ProfileValue::Number(42.0)) }));
        mods.set("b", "stats", "grinds", opts(|o| { o.label = Some("#Grinds".into()); o.order = Some(0) }));
        mods.set("b", "stats", "flips", opts(|o| o.value = Some(ProfileValue::Number(43.0))));
        let data = mods.apply(&ProfileData::default());
        assert_eq!((data.career.title.as_str(), data.career.current, data.career.total), ("#Career", 3.0, 10.0));
        let ids: Vec<&str> = data.stats.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["grinds", "flips"], "an explicit order goes first");
        assert_eq!(data.stats[1].value, StatValue::Number(43.0), "a later mod changes an earlier row");
        mods.set("b", "stats", "flips", opts(|o| o.hidden = true));
        assert_eq!(mods.apply(&ProfileData::default()).stats.len(), 1);
        mods.clear_owner("b");
        let data = mods.apply(&ProfileData::default());
        assert_eq!(data.stats.len(), 1);
        assert_eq!(data.stats[0].value, StatValue::Number(42.0));
        mods.clear();
        assert_eq!(mods.apply(&ProfileData::default()), ProfileData::default());
    }
}
