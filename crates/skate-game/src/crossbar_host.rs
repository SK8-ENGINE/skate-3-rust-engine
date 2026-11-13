//! The CoreMenu script object core_menu.apt reads (registrar 825B68E0, 21 members).
//! Details: docs/hails-additions/31-retail-menus.md#coremenu-host
use crate::apt_vm::{to_integer, Value, Vm};
use skate_core::{
    crossbar::{CrossbarState, CrossbarView, RegistryView, OPTION_LEVEL},
    menus::{Icon, MenuContext, MenuRegistry},
};

/// Retail GetMaxSubOptions (825B7238): the width of the sub-option tables.
pub const MAX_SUB_OPTIONS: usize = 6;

/// Context facts the progression panel reads (GetProgressionData, 825B73D8); absent = retail's "no career data"
/// value -1 in slot 0, which hides the panel.
pub mod progression_facts {
    pub const MILESTONE: &str = "progression_milestone";
    pub const COUNT: &str = "progression_count";
    pub const START: &str = "progression_start";
    pub const GOAL: &str = "progression_goal";
    pub const BOARD_SALES: &str = "progression_board_sales";
    pub const EXTRA: &str = "progression_extra";
}

pub struct CoreMenu<'a> {
    pub registry: &'a MenuRegistry,
    pub context: &'a MenuContext,
    pub view: &'a RegistryView,
    pub state: &'a mut CrossbarState,
    /// Sub-option label / helper rows per kind (menu tables).
    pub sub_options: &'a [SubOptionRow],
}

pub use skate_data::menu_tables::SubOptionRow;

/// Context facts the sub-option rules read (set by the systems that own them).
pub mod sub_option_facts {
    /// Retail global [0x830B7AE8] == 1: the park kind's last row (tutorial) shows.
    pub const PARK_TUTORIAL: &str = "crossbar.park_tutorial";
    /// On-screen help toggles currently on: their row offers "off" (ID_..._OFF).
    pub const TRICK_ANALYZER: &str = "crossbar.trick_analyzer";
    pub const MANUAL_METER: &str = "crossbar.manual_meter";
}

/// GetNumSubOptions (sub_8261DB48) per kind: the non-empty labels of the kind's row; the park kind (0) one less
/// unless the park tutorial flag is set; offline counts for the online kinds (contacts 3, online team 3, party 1;
/// their online rules are not ported).
pub fn sub_option_counts(rows: &[SubOptionRow], context: &MenuContext) -> std::collections::BTreeMap<i32, usize> {
    let fact = |name: &str| context.facts.get(name).copied().unwrap_or(0.0);
    rows.iter()
        .enumerate()
        .map(|(kind, row)| {
            let labels = row.labels.iter().filter(|l| !l.is_empty()).count();
            let count = match kind {
                0 if fact(sub_option_facts::PARK_TUTORIAL) != 1.0 => labels.saturating_sub(1),
                1 | 5 => labels.min(3),
                6 => labels.min(1),
                _ => labels,
            };
            (kind as i32, count)
        })
        .collect()
}

fn icon(icon: &Icon) -> Value {
    match icon {
        Icon::Retail(label) | Icon::Image(label) => Value::Text(label.clone()),
    }
}

impl CoreMenu<'_> {
    fn arg(args: &[Value], i: usize) -> i32 {
        args.get(i).map_or(0, to_integer)
    }
    fn item_id(&self, menu: i32, option: i32) -> Option<&str> {
        if menu < 0 || option < 0 {
            return None;
        }
        self.view.item(menu as usize, option as usize)
    }

    /// One CoreMenu member call. Unknown members are an error (the movie only calls the 21 retail ones).
    pub fn call(&mut self, vm: &mut Vm, method: &str, args: &[Value]) -> Result<Value, String> {
        let registry = self.registry;
        let data = registry.data();
        let n = |v: usize| Value::Number(v as f64);
        let i = |v: i32| Value::Number(v as f64);
        Ok(match method {
            "GetNumMenus" => n(self.view.tab_count()),
            "GetMenuName" | "GetMenuIconName" => {
                let menu = Self::arg(args, 0);
                let category = (menu >= 0)
                    .then(|| self.view.tabs.get(menu as usize))
                    .flatten()
                    .and_then(|id| data.category(id));
                match category {
                    Some(c) if method == "GetMenuName" => Value::Text(c.label.clone()),
                    Some(c) => icon(&c.icon),
                    None => Value::Undefined,
                }
            }
            "GetNumOptions" => n(self.view.row_count(Self::arg(args, 0).max(0) as usize)),
            "GetOptionName" | "GetOptionIconName" => {
                let item = self.item_id(Self::arg(args, 0), Self::arg(args, 1)).and_then(|id| data.item(id));
                match item {
                    // NOT PORTED YET: retail's label overrides (SignIn -> sign out when signed in, ReplayEditor in
                    // instant replay, PartyPlay per-language literal; sub_8261DCF0).
                    Some(item) if method == "GetOptionName" => Value::Text(item.label.clone()),
                    Some(item) => icon(&item.icon),
                    None => Value::Undefined,
                }
            }
            "GetNumSubOptions" => {
                let (menu, option) = (Self::arg(args, 0), Self::arg(args, 1));
                if menu < 0 || option < 0 {
                    i(0)
                } else {
                    n(self.view.sub_option_count(menu as usize, option as usize))
                }
            }
            // sub_8261DE88: the label table by (kind, sub); on-screen help rows name the action, so an option that
            // is on offers "off". NOT PORTED: the online kinds' label swaps (host options, online team).
            "GetSubOptionName" => {
                let (menu, option, sub) = (Self::arg(args, 0), Self::arg(args, 1), Self::arg(args, 2));
                let kind = (menu >= 0 && option >= 0).then(|| self.view.sub_option_kind(menu as usize, option as usize));
                let label = kind
                    .filter(|k| *k >= 0 && sub >= 0)
                    .and_then(|k| self.sub_options.get(k as usize)?.labels.get(sub as usize).cloned())
                    .filter(|l| !l.is_empty());
                let on = |name: &str| self.context.facts.get(name).copied().unwrap_or(0.0) != 0.0;
                let label = match (kind, sub, label) {
                    (Some(4), 0, Some(_)) if on(sub_option_facts::TRICK_ANALYZER) => Some("ID_CROSSBAR_TRICK_ANALYZER_OFF".into()),
                    (Some(4), 1, Some(_)) if on(sub_option_facts::MANUAL_METER) => Some("ID_CROSSBAR_MANUAL_METER_OFF".into()),
                    (_, _, label) => label,
                };
                Value::Text(label.unwrap_or_else(|| "#".into()))
            }
            "GetCurrentMenuIndex" => i(self.state.menu),
            "GetCurrentOptionIndex" => i(self.state.option),
            "GetCurrentSubOptionIndex" => i(self.state.sub),
            "GetCurrentMenuLevel" => i(self.state.level),
            "IsItemEnabled" => {
                let (menu, option) = (Self::arg(args, 0), Self::arg(args, 1));
                Value::Bool(menu >= 0 && option >= 0 && self.view.row_enabled(menu as usize, option as usize))
            }
            "IsDLCEnabled" => Value::Bool(self.state.dlc && self.view.extra_available()),
            "GetMaxSubOptions" => n(MAX_SUB_OPTIONS),
            "GetCurrentModeTitle" => {
                data.mode(&self.context.mode).map_or(Value::Undefined, |m| Value::Text(m.title.clone()))
            }
            "GetOptionHelperDescription" => {
                // sub_8261E270: the item's helper at the option level, the sub-option helper table by (kind, sub)
                // at level 2, else the empty literal.
                let help = if self.state.level == OPTION_LEVEL {
                    self.item_id(self.state.menu, self.state.option).and_then(|id| data.item(id)).map(|item| item.help.clone())
                } else {
                    let kind = (self.state.menu >= 0 && self.state.option >= 0)
                        .then(|| self.view.sub_option_kind(self.state.menu as usize, self.state.option as usize))
                        .filter(|k| *k >= 0);
                    kind.and_then(|k| self.sub_options.get(k as usize)?.helpers.get(self.state.sub.max(0) as usize).cloned())
                        .filter(|h| !h.is_empty())
                };
                Value::Text(help.unwrap_or_else(|| "#".into()))
            }
            "GetCurrentDLCBannerIndex" => i(self.state.dlc_banner),
            "GetProgressionData" => {
                use progression_facts::*;
                let fact = |name: &str, default: f64| self.context.facts.get(name).copied().unwrap_or(default);
                let values = vec![
                    Value::Number(fact(MILESTONE, -1.0)),
                    Value::Number(fact(COUNT, 0.0)),
                    Value::Number(fact(START, 0.0)),
                    Value::Number(fact(GOAL, 0.0)),
                    Value::Number(fact(BOARD_SALES, 0.0)),
                    Value::Number(fact(EXTRA, 0.0)),
                ];
                Value::Object(vm.array(values)?)
            }
            "GetIsDemo" | "GetIsMarketingDemo" => Value::Bool(false),
            "SetOnlineTeamSubOptionDone" => {
                self.state.online_team_pending = false;
                Value::Undefined
            }
            _ => return Err(format!("CoreMenu has no member {method}")),
        })
    }
}

/// The front-end host objects on the stage (retail sub_825A2190 creates them): CoreMenu answered from the
/// registry and crossbar state; Game's screen callbacks are recorded for the screen stack (next milestone).
pub struct FrontEnd<'a> {
    pub registry: &'a MenuRegistry,
    pub context: &'a MenuContext,
    pub view: RegistryView,
    pub state: CrossbarState,
    /// The language table (label id -> text) and number separators for FELanguage.
    pub language: &'a std::collections::BTreeMap<String, String>,
    pub numbers: crate::fe_language::NumberFormat,
    /// Game / Audio / FELanguage calls not handled yet, in order (diagnostics, tests).
    pub calls: Vec<String>,
    /// The Game Settings screen (settings_host) and its screen titles (menu tables).
    pub settings: crate::settings_host::SettingsState,
    pub settings_titles: Vec<String>,
    /// Crossbar sub-option tables (menu tables).
    pub sub_options: std::sync::Arc<Vec<SubOptionRow>>,
    /// The Trick Guide screen (trick_guide_host).
    pub trick_guide: crate::trick_guide_host::TrickGuideHost,
    /// The instant replay screen FE 12 (replay_menu_host).
    pub replay_menu: crate::replay_menu_host::ReplayMenuHost,
    /// The Challenge Map screen (challenge_map_host).
    pub challenge_map: crate::challenge_map_host::ChallengeMapHost,
    /// The Skate Profile screen (offline_profile_host).
    pub offline_profile: crate::offline_profile_host::OfflineProfileHost,
    /// The Extras screen (extras_host).
    pub extras: crate::extras_host::ExtrasHost,
    /// The text viewer FE 3's `tos` object (extras_host).
    pub tos: crate::extras_host::TosHost,
    /// The credits FE 25's `FECredits` object (extras_host).
    pub credits: crate::extras_host::CreditsHost,
    /// FE 62 SpawnSkater's `SkaterSpawner` object (spawn_skater_host).
    pub spawn_skater: crate::spawn_skater_host::SpawnSkaterHost,
    /// FE 52 loading screen's `TipsLoader` object (tips_loader_host).
    pub tips_loader: crate::tips_loader_host::TipsLoaderHost,
    /// FE 34 FreeSkateOption's object (free_skate_options_host).
    pub free_skate: crate::free_skate_options_host::FreeSkateOptionsHost,
    /// The image slots (`Game.ImageRenderer_*`, one global manager across screens).
    pub image_slots: crate::image_renderer_host::ImageSlots,
    /// The popup host (FE screen 29): the shown popup and the choices made.
    pub popup: crate::popup_host::PopupHost,
    /// The action overlay's teleporter title (`SignUpPoster.GetTeleportTitle`, 825C1020; teleporters.rs).
    pub signup_title: Option<String>,
}

impl<'a> FrontEnd<'a> {
    pub fn new(
        registry: &'a MenuRegistry,
        context: &'a MenuContext,
        language: &'a std::collections::BTreeMap<String, String>,
    ) -> Self {
        let view = RegistryView::new(registry, context, Default::default());
        let state = CrossbarState::open(&view);
        Self {
            registry,
            context,
            view,
            state,
            language,
            numbers: Default::default(),
            calls: Vec::new(),
            settings: Default::default(),
            settings_titles: Vec::new(),
            sub_options: Default::default(),
            trick_guide: Default::default(),
            replay_menu: Default::default(),
            challenge_map: Default::default(),
            offline_profile: Default::default(),
            extras: Default::default(),
            tos: Default::default(),
            credits: Default::default(),
            spawn_skater: Default::default(),
            tips_loader: Default::default(),
            free_skate: Default::default(),
            image_slots: Default::default(),
            popup: Default::default(),
            signup_title: None,
        }
    }
}

impl crate::apt_stage::StageNatives for FrontEnd<'_> {
    fn objects(&self) -> Vec<String> {
        ["Game", "CoreMenu", "Audio", "FELanguage", "FETrickTutorial", "FEChallengeMap", "FEReplayMenu", "HUDComponents", "Popup", "Progression", "Stats", "SignUpPoster", "tos", "FECredits", "SkaterSpawner", "FreeSkateOption", "TipsLoader"]
            .map(String::from)
            .to_vec()
    }
    fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
        if object == "CoreMenu" {
            let mut core = CoreMenu {
                registry: self.registry,
                context: self.context,
                view: &self.view,
                state: &mut self.state,
                sub_options: &self.sub_options,
            };
            return core.call(vm, method, &args);
        }
        // A shown popup is the focus screen (sub_82D09F28): its natives and the Game menu callbacks go to it first.
        if let Some(value) = self.popup.call(object, method, &args) {
            return Ok(value);
        }
        if let Some(value) = self.image_slots.call(object, method, &args) {
            return Ok(value);
        }
        if let Some(value) = self.challenge_map.call(vm, self.registry, self.context, self.language, object, method, &args)? {
            return Ok(value);
        }
        if let Some(value) = self.trick_guide.call(vm, object, method, &args)? {
            return Ok(value);
        }
        if let Some(value) = self.replay_menu.call(vm, self.language, object, method) {
            return Ok(value);
        }
        if let Some(value) = self.offline_profile.call(vm, object, method)? {
            return Ok(value);
        }
        // The action overlay's sign-up text (research b426): the teleporter's title in the Teleport state.
        if (object, method) == ("SignUpPoster", "GetTeleportTitle") || (object, method) == ("HUDComponents", "SignUp_GetChallengeTitle") {
            return Ok(Value::Text(self.signup_title.clone().unwrap_or_default()));
        }
        if object == "tos" {
            if let Some(value) = self.tos.call(vm, method, &args)? {
                return Ok(value);
            }
        }
        if object == "FECredits" {
            if let Some(value) = self.credits.call(self.language, method, &args) {
                return Ok(value);
            }
        }
        if let Some(value) = self.free_skate.call(object, method) {
            return Ok(value);
        }
        if let Some(value) = self.spawn_skater.call(vm, object, method, &args)? {
            return Ok(value);
        }
        if let Some(value) = self.tips_loader.call(vm, object, method, &args)? {
            return Ok(value);
        }
        if let Some(value) = (object == "Game").then(|| game_constant(method)).flatten() {
            return Ok(value);
        }
        // The Extras rows and its picker callbacks (only while FE 18 is on top).
        if let Some(value) = (object == "Game").then(|| self.extras.call(method, &args)).flatten() {
            return Ok(value);
        }
        if object == "Game" {
            let mut settings = crate::settings_host::GameSettings {
                registry: self.registry,
                context: self.context,
                titles: &self.settings_titles,
                state: &mut self.settings,
            };
            if let Some(value) = settings.call(method, &args) {
                if std::env::var_os("SKATE3_RETAIL_MENUS_TRACE").is_some() {
                    let args: Vec<String> = args.iter().map(Value::text).collect();
                    self.calls.push(format!("= Game.{method}({}) -> {}", args.join(","), value.text()));
                }
                return Ok(value);
            }
        }
        if object == "FELanguage" {
            use crate::fe_language::*;
            let arg = |v: &Value| match v {
                Value::Number(n) => Arg::Number(*n),
                Value::Bool(b) => Arg::Bool(*b),
                Value::Text(t) => Arg::Text(t.clone()),
                _ => Arg::Undefined,
            };
            let text = |i: usize| args.get(i).map(Value::text).unwrap_or_default();
            match method {
                "GetLocalizedFormatNumberString" => {
                    let decimals = args.get(1).map(|d| crate::apt_vm::to_integer(d).max(0) as usize);
                    let value = args.first().map_or(Arg::Undefined, arg);
                    return Ok(Value::Text(number_string(&self.numbers, &value, decimals)));
                }
                "GetLocalizedFormatString" => {
                    // (id, count, args...)
                    let id = text(0);
                    let count = args.get(1).map_or(0, |c| crate::apt_vm::to_integer(c).max(0) as usize);
                    let values: Vec<Arg> = args.iter().skip(2).take(count.min(6)).map(arg).collect();
                    return Ok(Value::Text(format_string(&self.numbers, template(self.language, &id), &values)));
                }
                "GetLocalizedFormatStringCustom" => {
                    let id = text(0);
                    return Ok(Value::Text(format_string_custom(&self.numbers, template(self.language, &id), &text(1))));
                }
                _ => {}
            }
        }
        self.calls.push(format!("{object}.{method}({})", args.iter().map(Value::text).collect::<Vec<_>>().join(",")));
        Ok(Value::Undefined)
    }
}

/// `Game` natives with a fixed answer. `IsDemo`: false, not the demo build (the replay editor movies grey out and never
/// highlight the Advanced Camera rows when it is true; unanswered, their `== false` check failed and rows 4..6 lost
/// the highlight: laptop 2026-10-25, USER-ASKS 339, research b591). Retail's other demo flags are constant false too
/// (`coremenu.GetIsDemo`, stub 825B3C80).
fn game_constant(method: &str) -> Option<Value> {
    match method {
        "IsDemo" => Some(Value::Bool(false)),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn test_registry() -> MenuRegistry {
    tests::registry()
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::menus::{Category, Item, MenuData, Mode, Tab};

    pub(super) fn registry() -> MenuRegistry {
        let item = |id: &str, kind: i32| Item {
            id: id.into(),
            label: format!("ID_{id}"),
            icon: Icon::Retail(id.to_lowercase()),
            help: format!("ID_{id}_HELP"),
            sub_option_kind: kind,
        };
        MenuRegistry::new(MenuData { map_nodes: Vec::new(), map_pictures: Default::default(), map_markers: Default::default(), map_debug: Default::default(),
            categories: vec![
                Category { id: "SinglePlayer".into(), label: "ID_CROSSBAR_SINGLE_PLAYER".into(), icon: Icon::Retail("singleplayer".into()) },
                Category { id: "Options".into(), label: "ID_CROSSBAR_OPTIONS".into(), icon: Icon::Retail("options".into()) },
            ],
            items: vec![item("ChallengeMap", -1), item("SkateReel", 3), item("GameSettings", -1)],
            settings: vec![],
            screens: vec![],
            modes: vec![Mode {
                key: "FreePlay".into(),
                title: "ID_CROSSBAR_FREEPLAY".into(),
                slots: 8,
                tabs: vec![
                    Tab { category: "SinglePlayer".into(), items: vec!["ChallengeMap".into(), "SkateReel".into()] },
                    Tab { category: "Options".into(), items: vec!["GameSettings".into()] },
                ],
            }],
        })
    }

    #[test]
    fn core_menu_members_read_the_registry_and_state() {
        let registry = registry();
        let context = MenuContext::new("FreePlay");
        let view = RegistryView::new(&registry, &context, [(3, 2)].into_iter().collect());
        let mut state = CrossbarState::open(&view);
        let mut vm = Vm::new();
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state, sub_options: &[] };
        let num = |v: f64| Value::Number(v);
        assert_eq!(m.call(&mut vm, "GetNumMenus", &[]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetMenuName", &[num(1.0)]), Ok(Value::Text("ID_CROSSBAR_OPTIONS".into())));
        assert_eq!(m.call(&mut vm, "GetMenuIconName", &[num(0.0)]), Ok(Value::Text("singleplayer".into())));
        assert_eq!(m.call(&mut vm, "GetNumOptions", &[num(0.0)]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetOptionName", &[num(0.0), num(1.0)]), Ok(Value::Text("ID_SkateReel".into())));
        assert_eq!(m.call(&mut vm, "GetNumSubOptions", &[num(0.0), num(1.0)]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetMaxSubOptions", &[]), Ok(num(6.0)));
        assert_eq!(m.call(&mut vm, "GetCurrentMenuLevel", &[]), Ok(num(1.0)));
        assert_eq!(m.call(&mut vm, "GetCurrentModeTitle", &[]), Ok(Value::Text("ID_CROSSBAR_FREEPLAY".into())));
        // Greyed until a handler enables it (the registry's "greyed until built" rule).
        assert_eq!(m.call(&mut vm, "IsItemEnabled", &[num(0.0), num(0.0)]), Ok(Value::Bool(false)));
        let Ok(Value::Object(p)) = m.call(&mut vm, "GetProgressionData", &[]) else { panic!() };
        assert_eq!((vm.get(p, "0"), vm.get(p, "length")), (num(-1.0), num(6.0)));
        assert!(m.call(&mut vm, "Nope", &[]).is_err());
        state.online_team_pending = true;
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state, sub_options: &[] };
        m.call(&mut vm, "SetOnlineTeamSubOptionDone", &[]).unwrap();
        assert!(!state.online_team_pending);
    }

    #[test]
    fn game_is_demo_is_false() {
        assert_eq!(game_constant("IsDemo"), Some(Value::Bool(false)));
        assert_eq!(game_constant("Nope"), None);
    }

    #[test]
    fn sub_options_follow_the_retail_tables() {
        let registry = registry();
        let mut context = MenuContext::new("FreePlay");
        let row = |labels: &[&str], helpers: &[&str]| SubOptionRow {
            labels: labels.iter().map(|s| s.to_string()).collect(),
            helpers: helpers.iter().map(|s| s.to_string()).collect(),
        };
        let mut rows = vec![row(&["ID_NEW", "ID_MINE", "ID_COMMUNITY", "ID_TUTORIAL", "", ""], &["H0", "H1", "H2", "H3", "", ""])];
        rows.extend((1..3).map(|_| row(&["", "", "", "", "", ""], &["", "", "", "", "", ""])));
        rows.push(row(&["ID_REPLAYS", "ID_PHOTOS", "", "", "", ""], &["H_REPLAYS", "H_PHOTOS", "", "", "", ""]));
        rows.push(row(&["ID_CROSSBAR_TRICK_ANALYZER_ON", "ID_CROSSBAR_MANUAL_METER_ON", "", "", "", ""], &["", "", "", "", "", ""]));
        // Park: the tutorial row hides unless its flag is set; Skate Reel: two.
        let counts = sub_option_counts(&rows, &context);
        assert_eq!((counts[&0], counts[&3], counts[&4]), (3, 2, 2));
        context.facts.insert(sub_option_facts::PARK_TUTORIAL.into(), 1.0);
        assert_eq!(sub_option_counts(&rows, &context)[&0], 4);
        // SkateReel (kind 3) is row 1 of tab 0 in the test registry.
        let view = RegistryView::new(&registry, &context, sub_option_counts(&rows, &context));
        let mut state = CrossbarState::open(&view);
        let mut vm = Vm::new();
        let num = |v: f64| Value::Number(v);
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state, sub_options: &rows };
        assert_eq!(m.call(&mut vm, "GetSubOptionName", &[num(0.0), num(1.0), num(1.0)]), Ok(Value::Text("ID_PHOTOS".into())));
        assert_eq!(m.call(&mut vm, "GetSubOptionName", &[num(0.0), num(1.0), num(4.0)]), Ok(Value::Text("#".into())));
        assert_eq!(m.call(&mut vm, "GetNumSubOptions", &[num(0.0), num(1.0)]), Ok(num(2.0)));
        state.level = skate_core::crossbar::SUB_OPTION_LEVEL;
        state.menu = 0;
        state.option = 1;
        state.sub = 0;
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state, sub_options: &rows };
        assert_eq!(m.call(&mut vm, "GetOptionHelperDescription", &[]), Ok(Value::Text("H_REPLAYS".into())));
    }

}
