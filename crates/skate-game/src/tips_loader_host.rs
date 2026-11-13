//! FE 52 loading screen's `TipsLoader` natives (registrar sub_825B9C58; research b581) while it is up. Details:
//! docs/hails-additions/31-retail-menus.md#loading-screen-fe-52
use crate::apt_vm::{Value, Vm};
use skate_core::tips_loader::{Panel, TipsLoader};

#[derive(Clone, Default)]
pub struct TipsLoaderHost {
    pub tips: TipsLoader,
    /// The `tips_loader/default` lines (string ids).
    pub lines: skate_data::tips_loader::TipsLines,
    /// Stats rows (label, value) for the Stats panel; empty = no Stats panel (retail's rule without stats).
    pub stats: Vec<(String, String)>,
    /// The tips RNG (T+244; retail's seed is open): a per-boot LCG.
    pub seed: u32,
    /// FE 52 is up.
    pub open: bool,
    /// The methods the movie called (tests).
    #[cfg(test)]
    pub asked: Vec<String>,
}

/// One draw of the tips RNG.
pub fn lcg(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *seed >> 16
}

impl TipsLoaderHost {
    /// The panels that can show offline: Coach Frank and the hint (with their lines), Stats with rows.
    pub fn available(&self) -> [bool; 6] {
        [!self.lines.coach.is_empty(), !self.stats.is_empty(), !self.lines.hints.is_empty(), false, false, false]
    }

    pub fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if !self.open || object != "TipsLoader" {
            return Ok(None);
        }
        #[cfg(test)]
        self.asked.push(method.to_owned());
        let enabled = |p: Panel| Value::Bool(self.tips.shown() == Some(p));
        Ok(Some(match method {
            "IsCoachFrankEnabled" => enabled(Panel::CoachFrank),
            "IsStatsEnabled" => enabled(Panel::Stats),
            "IsHintEnabled" => enabled(Panel::Hint),
            "IsLeaderboardEnabled" => enabled(Panel::Leaderboard),
            "IsFeedEnabled" => enabled(Panel::Feed),
            "IsReelEnabled" => enabled(Panel::Reel),
            "GetCoachFrankText" => {
                let mut seed = self.seed;
                let i = self.tips.coach_line(self.lines.coach.len(), &mut || lcg(&mut seed));
                self.seed = seed;
                Value::Text(self.lines.coach.get(i).cloned().unwrap_or_default())
            }
            "GetHint" => {
                let mut seed = self.seed;
                let i = self.tips.hint_line(self.lines.hints.len(), &mut || lcg(&mut seed));
                self.seed = seed;
                let (title, text) = self.lines.hints.get(i).cloned().unwrap_or_default();
                Value::Object(vm.array(vec![Value::Text(title), Value::Text(text)])?)
            }
            // 825BA018: up to n rows of [label, value].
            "GetStats" => {
                let n = args.first().map_or(0, crate::apt_vm::to_integer).max(0) as usize;
                // Shuffled again on every call (sub_826060F0), then up to n rows.
                let mut order: Vec<usize> = (0..self.stats.len()).collect();
                let mut seed = self.seed;
                skate_core::career_stats::shuffle(&mut order, &mut || lcg(&mut seed));
                self.seed = seed;
                let rows = order.into_iter().take(n).map(|i| self.stats[i].clone()).collect::<Vec<_>>();
                let mut out = Vec::new();
                for (label, value) in rows {
                    out.push(Value::Object(vm.array(vec![Value::Text(label), Value::Text(value)])?));
                }
                Value::Object(vm.array(out)?)
            }
            // Online panels: never enabled offline; empty answers.
            "GetLeaderboard" | "GetFeedItems" => Value::Object(vm.array(Vec::new())?),
            "GetReelItem" => Value::Undefined,
            _ => return Ok(None),
        }))
    }
}

/// How long a mod's hold may keep the loading screen up after a load's own work is done (so a mod never leaves it
/// stuck; `StartupLoading`'s cap is the same idea).
pub const MAX_MOD_HOLD_SECS: f32 = 30.0;

/// Mods' loading screen lines and holds (`sdk.loading`), cleared with the mod.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct LoadingMods {
    lines: Vec<(String, String, skate_mods::menus::LoadingLine)>,
    retail_hidden: std::collections::BTreeSet<String>,
    holds: std::collections::BTreeSet<(String, String)>,
    /// Panel timings by mod, in the order set (the newest wins).
    timings: Vec<(String, (Option<f32>, Option<f32>))>,
    /// `loading_opened` / `loading_closed` for every mod's `on_event`.
    events: Vec<serde_json::Value>,
}

impl LoadingMods {
    pub(crate) fn set_line(&mut self, owner: &str, key: &str, line: Option<skate_mods::menus::LoadingLine>) {
        match (self.lines.iter_mut().find(|(o, k, _)| o == owner && k == key), line) {
            (Some(slot), Some(line)) => slot.2 = line,
            (None, Some(line)) => self.lines.push((owner.to_owned(), key.to_owned(), line)),
            (_, None) => self.lines.retain(|(o, k, _)| !(o == owner && k == key)),
        }
    }

    pub(crate) fn set_retail(&mut self, owner: &str, shown: bool) {
        if shown {
            self.retail_hidden.remove(owner);
        } else {
            self.retail_hidden.insert(owner.to_owned());
        }
    }

    pub(crate) fn set_hold(&mut self, owner: &str, key: &str, held: bool) {
        let slot = (owner.to_owned(), key.to_owned());
        if held {
            self.holds.insert(slot);
        } else {
            self.holds.remove(&slot);
        }
    }

    /// Mods' Stats panel rows (label, value), in the order added.
    pub(crate) fn stat_rows(&self) -> Vec<(String, String)> {
        self.lines.iter().filter_map(|(_, _, l)| match l {
            skate_mods::menus::LoadingLine::Stat { label, value } => Some((label.clone(), value.clone())),
            _ => None,
        }).collect()
    }

    pub(crate) fn set_timing(&mut self, owner: &str, timing: Option<(Option<f32>, Option<f32>)>) {
        self.timings.retain(|(o, _)| o != owner);
        if let Some(timing) = timing {
            self.timings.push((owner.to_owned(), timing));
        }
    }

    /// The panel timing: the newest mod's values over retail's.
    pub(crate) fn timing(&self) -> skate_core::tips_loader::Timing {
        let retail = skate_core::tips_loader::Timing::RETAIL;
        self.timings.last().map_or(retail, |(_, (show, outro))| skate_core::tips_loader::Timing { show: show.unwrap_or(retail.show), outro: outro.unwrap_or(retail.outro) })
    }

    pub(crate) fn opened(&mut self, map: Option<&str>) {
        self.events.push(serde_json::json!({"name": "loading_opened", "map": map}));
    }

    pub(crate) fn closed(&mut self, map: Option<&str>) {
        self.events.push(serde_json::json!({"name": "loading_closed", "map": map}));
    }

    pub(crate) fn drain_events(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.events)
    }

    /// Drop one mod's lines, holds and timing (None: every mod's).
    pub(crate) fn clear_owner(&mut self, owner: Option<&str>) {
        let keep = |o: &String| owner.is_some_and(|owner| o != owner);
        self.timings.retain(|(o, _)| keep(o));
        self.lines.retain(|(o, _, _)| keep(o));
        self.retail_hidden.retain(|o| keep(o));
        self.holds.retain(|(o, _)| keep(o));
    }

    /// A mod hold keeps the screen up `after` seconds past the load's own work, up to the cap.
    pub(crate) fn holding(&self, after: f32) -> bool {
        !self.holds.is_empty() && after < MAX_MOD_HOLD_SECS
    }

    /// The lines the screen picks from: the retail ones (unless a mod hides them and mods give lines of that panel),
    /// then the mods' in the order they were added.
    pub(crate) fn lines(&self, retail: &skate_data::tips_loader::TipsLines) -> skate_data::tips_loader::TipsLines {
        use skate_mods::menus::LoadingLine;
        let hints: Vec<(String, String)> = self.lines.iter().filter_map(|(_, _, l)| match l {
            LoadingLine::Hint { title, text } => Some((title.clone(), text.clone())),
            _ => None,
        }).collect();
        let coach: Vec<String> = self.lines.iter().filter_map(|(_, _, l)| match l {
            LoadingLine::CoachFrank { text } => Some(text.clone()),
            _ => None,
        }).collect();
        let hide = !self.retail_hidden.is_empty();
        let mut out = skate_data::tips_loader::TipsLines::default();
        if !(hide && !coach.is_empty()) {
            out.coach.extend(retail.coach.iter().cloned());
        }
        if !(hide && !hints.is_empty()) {
            out.hints.extend(retail.hints.iter().cloned());
        }
        out.coach.extend(coach);
        out.hints.extend(hints);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_mods::menus::LoadingLine;

    #[test]
    fn mod_lines_join_the_retail_ones_and_go_with_the_mod() {
        let retail = skate_data::tips_loader::TipsLines { coach: vec!["ID_TIPS_COACHFRANK_1".into()], hints: vec![("ID_HINTS_TITLE_1".into(), "ID_HINTS_TEXT_1".into())] };
        let mut mods = LoadingMods::default();
        mods.set_line("a", "tip", Some(LoadingLine::Hint { title: "#Tip".into(), text: "#Ollie".into() }));
        mods.set_line("b", "frank", Some(LoadingLine::CoachFrank { text: "#Push".into() }));
        let lines = mods.lines(&retail);
        assert_eq!(lines.hints.len(), 2);
        assert_eq!(lines.coach, ["ID_TIPS_COACHFRANK_1", "#Push"]);
        // Hiding the retail lines only drops a panel the mods give lines for.
        mods.set_retail("a", false);
        let lines = mods.lines(&retail);
        assert_eq!(lines.hints, [("#Tip".to_owned(), "#Ollie".to_owned())]);
        assert_eq!(lines.coach, ["#Push"]);
        mods.set_hold("a", "bank", true);
        assert!(mods.holding(0.0) && !mods.holding(MAX_MOD_HOLD_SECS));
        mods.clear_owner(Some("a"));
        let lines = mods.lines(&retail);
        assert_eq!(lines.hints.len(), 1);
        assert_eq!(lines.coach.len(), 2);
        assert!(!mods.holding(0.0));
        mods.clear_owner(None);
        assert_eq!(mods.lines(&retail), retail);
    }

    #[test]
    fn mod_timing_overrides_retail_and_goes_with_the_mod() {
        use skate_core::tips_loader::Timing;
        let mut mods = LoadingMods::default();
        assert_eq!(mods.timing(), Timing::RETAIL);
        mods.set_timing("a", Some((Some(4.0), None)));
        assert_eq!(mods.timing(), Timing { show: 4.0, outro: 0.5 });
        mods.set_timing("b", Some((None, Some(0.0))));
        assert_eq!(mods.timing(), Timing { show: 9.0, outro: 0.0 });
        mods.clear_owner(Some("b"));
        assert_eq!(mods.timing(), Timing { show: 4.0, outro: 0.5 });
        mods.set_timing("a", None);
        assert_eq!(mods.timing(), Timing::RETAIL);
        mods.set_line("a", "kicks", Some(LoadingLine::Stat { label: "#Kickflips".into(), value: "#12".into() }));
        assert_eq!(mods.stat_rows(), [("#Kickflips".to_owned(), "#12".to_owned())]);
        assert_eq!(mods.lines(&skate_data::tips_loader::TipsLines::default()), Default::default(), "a stat row is no hint or Coach Frank line");
        mods.opened(Some("University"));
        mods.closed(None);
        let events = mods.drain_events();
        assert_eq!(events[0], serde_json::json!({"name": "loading_opened", "map": "University"}));
        assert_eq!(events[1]["name"], "loading_closed");
        assert!(mods.drain_events().is_empty());
    }
}
