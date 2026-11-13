//! Menus extension (`sdk.menus`, capability `retail_menus` = 1): edit the game's
//! retail menu structure (pause menu categories and items, Game Settings rows)
//! and register handlers for any entry. Entries are keyed by retail internal
//! names (`GameSettings`, `SkateFeed`, `SinglePlayer`, `ID_GAMESETTINGS_SFXVOLUME`);
//! new entries a mod adds are named `<mod id>.<name>`. Everything a mod did is
//! removed when it is disabled.
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES_PER_MOD: usize = 64;

/// Retail ids are upper/mixed case (`ID_GAMESETTINGS_*`, `GameSettings`).
pub fn valid_menu_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The pause menu kinds, and the Challenge Map node kinds (vault classes: `fe_locations` = a teleport location, ...).
pub fn valid_entry_kind(s: &str) -> bool {
    matches!(
        s,
        "category" | "item" | "setting" | "map_listing" | "map_filter" | "map_category" | "fe_locations" | "world" | "challenges"
            | "map_commands" | "code_unlocks"
    )
}

fn valid_label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 96 && !s.chars().any(char::is_control)
}

fn valid_path(s: &str) -> bool {
    !s.is_empty() && s.len() <= 160 && !s.contains("..") && !s.starts_with(['/', '\\']) && !s.contains(':')
}

/// Add-or-override options for a category, item or settings row. Fields left
/// out keep the current value (retail's for retail entries).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryOptions {
    /// Display text (a language string id or plain text).
    #[serde(default)]
    pub label: Option<String>,
    /// A retail icon frame label (`map`, `settings`, `skatefeed`, ...).
    #[serde(default)]
    pub icon: Option<String>,
    /// Or an image inside the mod (drawn once the menus render; wins over `icon`).
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    /// Item: the tab (category id) it goes into.
    #[serde(default)]
    pub category: Option<String>,
    /// Mode keys (Career, CareerPark, FreePlay, FreePlayPark, FreeSkateRestricted, ...);
    /// empty = every mode.
    #[serde(default, deserialize_with = "crate::lua_list::list")]
    pub modes: Vec<String>,
    /// 0-based position in its tab / screen / tab bar.
    #[serde(default)]
    pub position: Option<usize>,
    /// Setting: `option`, `slider` or `selector`.
    #[serde(default)]
    pub widget: Option<String>,
    /// Setting: the settings screen index it goes on.
    #[serde(default)]
    pub screen: Option<usize>,
}

impl EntryOptions {
    pub fn validate(&self) -> bool {
        self.label.as_deref().is_none_or(valid_label)
            && self.help.as_deref().is_none_or(|h| h.len() <= 512 && !h.chars().any(char::is_control))
            && self.icon.as_deref().is_none_or(valid_menu_id)
            && self.image.as_deref().is_none_or(valid_path)
            && self.category.as_deref().is_none_or(valid_menu_id)
            && self.modes.len() <= 16
            && self.modes.iter().all(|m| valid_menu_id(m))
            && self.position.is_none_or(|p| p < 64)
            && self.widget.as_deref().is_none_or(|w| matches!(w, "option" | "slider" | "selector"))
            && self.screen.is_none_or(|s| s < 64)
    }
}

/// A setting value: boolean, integer or number.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MenuValue {
    Bool(bool),
    Int(i64),
    Float(f64),
}

impl MenuValue {
    pub fn validate(&self) -> bool {
        !matches!(self, Self::Float(v) if !v.is_finite())
    }
}

/// A data rule: `true` / `false`, or a table with one of
/// `fact` + `eq|ne|lt|le|gt|ge`, `modes`, `all`, `any`, `not`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum RuleSpec {
    Bool(bool),
    Table(Box<RuleTable>),
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleTable {
    #[serde(default)]
    pub fact: Option<String>,
    #[serde(default)]
    pub eq: Option<f64>,
    #[serde(default)]
    pub ne: Option<f64>,
    #[serde(default)]
    pub lt: Option<f64>,
    #[serde(default)]
    pub le: Option<f64>,
    #[serde(default)]
    pub gt: Option<f64>,
    #[serde(default)]
    pub ge: Option<f64>,
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub modes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub all: Option<Vec<RuleSpec>>,
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub any: Option<Vec<RuleSpec>>,
    #[serde(default)]
    pub not: Option<RuleSpec>,
}

impl RuleSpec {
    pub fn validate(&self) -> bool {
        self.valid_at(0)
    }
    fn valid_at(&self, depth: usize) -> bool {
        let Self::Table(t) = self else { return true };
        if depth > 8 {
            return false;
        }
        let comparisons = [t.eq, t.ne, t.lt, t.le, t.gt, t.ge];
        let compared = comparisons.iter().flatten().count();
        let forms = usize::from(t.fact.is_some()) + usize::from(t.modes.is_some()) + usize::from(t.all.is_some())
            + usize::from(t.any.is_some()) + usize::from(t.not.is_some());
        let list_ok = |l: &Option<Vec<RuleSpec>>| l.as_ref().is_none_or(|l| l.len() <= 16 && l.iter().all(|r| r.valid_at(depth + 1)));
        forms == 1
            && (t.fact.is_some() == (compared == 1))
            && (t.fact.is_some() || compared == 0)
            && t.fact.as_deref().is_none_or(valid_menu_id)
            && comparisons.iter().flatten().all(|v| v.is_finite())
            && t.modes.as_ref().is_none_or(|m| m.len() <= 16 && m.iter().all(|m| valid_menu_id(m)))
            && list_ok(&t.all)
            && list_ok(&t.any)
            && t.not.as_ref().is_none_or(|r| r.valid_at(depth + 1))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandleOptions {
    /// Receive `menu_select` (enables the entry). Default true.
    #[serde(default = "yes")]
    pub select: bool,
    /// Receive `menu_highlight`.
    #[serde(default)]
    pub highlight: bool,
    /// Bind a settings row's value (kept by the game; changes arrive as `menu_value`).
    #[serde(default)]
    pub value: Option<MenuValue>,
    /// Override when the entry is enabled / visible (stacked over retail's rule).
    #[serde(default)]
    pub enabled: Option<RuleSpec>,
    #[serde(default)]
    pub visible: Option<RuleSpec>,
    /// Ask this confirmation first when `when` holds (default always).
    #[serde(default)]
    pub confirm: Option<ConfirmSpec>,
    /// Left / Right rule for a settings row (stacked over retail's; reverted on disable).
    #[serde(default)]
    pub rule: Option<ValueRuleSpec>,
}

/// `{kind='slider', min=0, max=1, step=0.1, snap=0.05, bars=10}` (retail's volume rule),
/// `{kind='toggle', labels={'ID_COMMON_OFF','ID_COMMON_ON'}}`, `{kind='cycle', count=3, labels={...}}`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ValueRuleSpec {
    Slider {
        #[serde(default)]
        min: f64,
        #[serde(default = "one")]
        max: f64,
        step: f64,
        #[serde(default)]
        snap: f64,
        #[serde(default = "ten")]
        bars: f64,
    },
    Toggle {
        #[serde(default = "off_on")]
        labels: [String; 2],
    },
    Cycle {
        count: i64,
        #[serde(default, deserialize_with = "crate::lua_list::list")]
        labels: Vec<String>,
    },
}

fn one() -> f64 {
    1.0
}
fn ten() -> f64 {
    10.0
}
fn off_on() -> [String; 2] {
    ["ID_COMMON_OFF".into(), "ID_COMMON_ON".into()]
}

impl ValueRuleSpec {
    pub fn validate(&self) -> bool {
        match self {
            Self::Slider { min, max, step, snap, bars } => {
                [min, max, step, snap, bars].iter().all(|v| v.is_finite() && v.abs() <= 1e6) && min < max && *step > 0.0 && *bars > 0.0
            }
            Self::Toggle { labels } => labels.iter().all(|l| valid_menu_id(l)),
            Self::Cycle { count, labels } => (1..=64).contains(count) && labels.len() <= 64 && labels.iter().all(|l| valid_menu_id(l)),
        }
    }
}

impl Default for HandleOptions {
    fn default() -> Self {
        Self { select: true, highlight: false, value: None, enabled: None, visible: None, confirm: None, rule: None }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmSpec {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub yes: Option<String>,
    #[serde(default)]
    pub no: Option<String>,
    #[serde(default)]
    pub when: Option<RuleSpec>,
}

impl HandleOptions {
    pub fn validate(&self) -> bool {
        self.value.as_ref().is_none_or(MenuValue::validate)
            && self.rule.as_ref().is_none_or(ValueRuleSpec::validate)
            && self.enabled.as_ref().is_none_or(RuleSpec::validate)
            && self.visible.as_ref().is_none_or(RuleSpec::validate)
            && self.confirm.as_ref().is_none_or(|c| {
                valid_label(&c.title)
                    && c.description.len() <= 512
                    && [&c.yes, &c.no].iter().all(|l| l.as_deref().is_none_or(valid_label))
                    && c.when.as_ref().is_none_or(RuleSpec::validate)
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn value_rule_specs_parse_and_validate() {
        use super::*;
        let h: HandleOptions = serde_json::from_value(serde_json::json!({"value":0.5,"rule":{"kind":"slider","step":0.25}})).unwrap();
        assert_eq!(h.rule, Some(ValueRuleSpec::Slider { min: 0.0, max: 1.0, step: 0.25, snap: 0.0, bars: 10.0 }));
        assert!(h.validate());
        let bad: HandleOptions = serde_json::from_value(serde_json::json!({"rule":{"kind":"cycle","count":0}})).unwrap();
        assert!(!bad.validate());
        assert!(serde_json::from_value::<HandleOptions>(serde_json::json!({"rule":{"kind":"wheel"}})).is_err());
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn options_and_rules_validate() {
        let o: EntryOptions = serde_json::from_value(json!({"label":"Races","icon":"map","category":"SinglePlayer","modes":["Career"],"position":0})).unwrap();
        assert!(o.validate());
        let o: EntryOptions = serde_json::from_value(json!({"image":"../x.png"})).unwrap();
        assert!(!o.validate());
        assert!(serde_json::from_value::<EntryOptions>(json!({"colour":1})).is_err());
        let r: RuleSpec = serde_json::from_value(json!({"all":[{"fact":"sfx_packs","gt":1}, {"not":{"modes":["CareerPark"]}}, true]})).unwrap();
        assert!(r.validate());
        let bad: RuleSpec = serde_json::from_value(json!({"fact":"x","gt":1,"lt":2})).unwrap();
        assert!(!bad.validate(), "one comparison per fact");
        let bad: RuleSpec = serde_json::from_value(json!({"fact":"x","modes":["Career"],"gt":1})).unwrap();
        assert!(!bad.validate(), "one form per table");
        let h: HandleOptions = serde_json::from_value(json!({"value":0.5,"highlight":true,"confirm":{"title":"Sure?"}})).unwrap();
        assert!(h.validate() && h.select);
        assert_eq!(serde_json::from_value::<MenuValue>(json!(3)).unwrap(), MenuValue::Int(3));
        assert_eq!(serde_json::from_value::<MenuValue>(json!(true)).unwrap(), MenuValue::Bool(true));
        assert!(valid_menu_id("ID_GAMESETTINGS_SFXVOLUME") && valid_menu_id("my-mod.Races") && !valid_menu_id("a b"));
    }
}

/// An FE screen id: the 16 lowercase hex digits of retail's per-screen key, plus `:n` when screens share the key
/// (`sdk.menus.info().screens` lists them).
pub fn valid_screen_key(s: &str) -> bool {
    let (key, n) = s.split_once(':').unwrap_or((s, "0"));
    key.len() == 16
        && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !n.is_empty()
        && n.len() <= 2
        && n.bytes().all(|b| b.is_ascii_digit())
}

/// `sdk.menus.trick_guide(entry, key, options)` entry kinds.
pub const TRICK_GUIDE_ENTRIES: [&str; 3] = ["category", "subcategory", "trick"];

/// One input slot of a Trick Guide entry: ids into the gesture / button name tables, connector 0 none, 1 Plus,
/// 2 Arrow, 3 Or (doc 31 "Trick Guide").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrickGuideSlot {
    #[serde(default)]
    pub gesture: u32,
    #[serde(default)]
    pub button: u32,
    #[serde(default)]
    pub connector: u32,
}

/// `sdk.menus.trick_guide(entry, key, options)`: add or change a Trick Guide entry by key (a retail record's 16 hex
/// digits, or the mod's own id). Unset fields keep the entry's value.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrickGuideOptions {
    /// Name and description: string ids (FELanguage) or `#literal text`.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Up to 4 input slots.
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub slots: Option<Vec<TrickGuideSlot>>,
    /// Demo animation name (tricks need one to be listed).
    #[serde(default)]
    pub animation: Option<String>,
    /// Demo camera track: an index into the ICE ViewScene tracks (retail 0..20; `trickbook_camera_tracks`).
    #[serde(default)]
    pub camera_track: Option<i16>,
    /// The category a subcategory lists under, or the subcategory a trick lists under (key).
    #[serde(default)]
    pub parent: Option<String>,
    /// A trick's place in its list (ascending).
    #[serde(default)]
    pub order: Option<i32>,
    /// A subcategory opens a trick list (true) or shows its own slots (false).
    #[serde(default)]
    pub has_tricks: Option<bool>,
    /// A category's place in the top-level list (0-based).
    #[serde(default)]
    pub position: Option<u32>,
    /// Take the entry out of its list.
    #[serde(default)]
    pub hidden: bool,
}

/// A retail popup a mod raises (`sdk.menus.popup`, Sk8Popup / Sk8SmallPopup); choice as a `menu_popup` event.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopupOptions {
    /// Shown by the small popup only (retail's Sk8Popup shows no title).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Up to 8 choices (index 0 first); one choice shows A Continue, more a list.
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub options: Option<Vec<String>>,
    /// "standard" (Sk8Popup, the default) or "small" (Sk8SmallPopup).
    #[serde(default)]
    pub kind: Option<String>,
    /// The highest priority shows; equal priorities show the newest (retail's list).
    #[serde(default)]
    pub priority: Option<i32>,
    /// B answers choice 255.
    #[serde(default)]
    pub cancelable: bool,
}

pub const MAX_POPUP_OPTIONS: usize = 8;

impl PopupOptions {
    pub fn validate(&self) -> bool {
        let text = |t: &Option<String>| t.as_ref().is_none_or(|t| t.len() <= 512);
        text(&self.title)
            && text(&self.description)
            && self.options.as_ref().is_none_or(|o| o.len() <= MAX_POPUP_OPTIONS && o.iter().all(|t| t.len() <= 512))
            && self.kind.as_deref().is_none_or(|k| matches!(k, "standard" | "small"))
            && self.priority.is_none_or(|p| (-1000..=1000).contains(&p))
    }
}

/// The Skate Profile pages a mod fills (`sdk.menus.profile(page, key, options)`): the two overall bars and the two
/// tables.
pub const PROFILE_PAGES: [&str; 4] = ["career", "challenges", "stats", "personal_bests"];

/// A table cell: a number (the movie formats it) or text (a string id or `#literal text`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ProfileValue {
    Number(f64),
    Text(String),
}

/// `sdk.menus.profile(page, key, options)`: change an overall bar (`career` / `challenges`: title, icon, current,
/// total; key ignored) or add / change a table row by key (`stats` / `personal_bests`: label, value, order, hidden).
/// Unset fields keep the current value.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOptions {
    /// Bar title: a string id (FELanguage) or `#literal text`.
    #[serde(default)]
    pub title: Option<String>,
    /// Bar icon: an asset name the movie loads (`LoadAssetByName`).
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub current: Option<f64>,
    #[serde(default)]
    pub total: Option<f64>,
    /// Row label: a string id or `#literal text`.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<ProfileValue>,
    /// A row's place in its table (ascending; rows without one keep their order after those with one).
    #[serde(default)]
    pub order: Option<i32>,
    /// Take the row out of its table.
    #[serde(default)]
    pub hidden: bool,
}

impl ProfileOptions {
    pub fn validate(&self, page: &str) -> bool {
        let text = |t: &Option<String>| t.as_ref().is_none_or(|t| t.len() <= 512);
        let number = |n: &Option<f64>| n.is_none_or(|n| n.is_finite());
        let bar = page == "career" || page == "challenges";
        let bar_fields = self.title.is_some() || self.icon.is_some() || self.current.is_some() || self.total.is_some();
        let row_fields = self.label.is_some() || self.value.is_some() || self.order.is_some() || self.hidden;
        let value = match &self.value {
            Some(ProfileValue::Number(n)) => n.is_finite(),
            Some(ProfileValue::Text(t)) => t.len() <= 512,
            None => true,
        };
        PROFILE_PAGES.contains(&page)
            && if bar { !row_fields } else { !bar_fields }
            && text(&self.title)
            && text(&self.icon)
            && text(&self.label)
            && number(&self.current)
            && number(&self.total)
            && value
    }
}

/// One line of `sdk.menus.credits(key, lines)`: a string id (`ID_...`, localised) or `#literal text`, its font size
/// and its line advance as a multiple of the line's height (retail lines: size 18 to 30, spacing 2).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditLine {
    pub text: String,
    #[serde(default = "credit_size")]
    pub size: i32,
    #[serde(default = "credit_spacing")]
    pub spacing: f64,
}

fn credit_size() -> i32 {
    18
}

fn credit_spacing() -> f64 {
    2.0
}

/// Lines one credits section may hold.
pub const MAX_CREDIT_LINES: usize = 512;

/// A loading screen line of a mod (`sdk.loading`): a hint panel's title and text, a Coach Frank line, or a Stats panel
/// row (label and value). A string id is localised, any other text shows as written.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "panel", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoadingLine {
    Hint { title: String, text: String },
    CoachFrank { text: String },
    Stat { label: String, value: String },
}

impl LoadingLine {
    pub fn validate(&self) -> bool {
        let ok = |t: &str| !t.is_empty() && t.len() <= 512;
        match self {
            Self::Hint { title, text } => ok(title) && ok(text),
            Self::CoachFrank { text } => ok(text),
            Self::Stat { label, value } => ok(label) && ok(value),
        }
    }
}

impl CreditLine {
    pub fn validate(&self) -> bool {
        !self.text.is_empty() && self.text.len() <= 512 && (8..=96).contains(&self.size) && (0.0..=10.0).contains(&self.spacing)
    }
}

pub fn valid_trick_guide_key(s: &str) -> bool {
    (s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())) || valid_menu_id(s)
}

impl TrickGuideOptions {
    pub fn validate(&self) -> bool {
        let text = |t: &Option<String>| t.as_ref().is_none_or(|t| t.len() <= 512);
        text(&self.label)
            && text(&self.description)
            && text(&self.animation)
            && self.parent.as_ref().is_none_or(|p| valid_trick_guide_key(p))
            && self.slots.as_ref().is_none_or(|s| s.len() <= 4 && s.iter().all(|s| s.gesture < 256 && s.button < 256 && s.connector <= 3))
            && self.position.is_none_or(|p| p < 256)
    }
}

/// `sdk.menus.backdrop(screen, options)`: the world behind an FE screen (blur on / off and the ramp). Unset fields
/// keep the retail value (screen stack info table + FE manager ramp, doc 31 "Pause blur").
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackdropOptions {
    #[serde(default)]
    pub blur: Option<bool>,
    /// Full blur strength (retail 8.0).
    #[serde(default)]
    pub strength: Option<f32>,
    /// Strength change per FE tick (retail 0.8).
    #[serde(default)]
    pub step: Option<f32>,
    /// Tint floor {r, g, b} 0..255 (retail {90, 85, 81}).
    #[serde(default, deserialize_with = "crate::lua_list::opt_list")]
    pub tint: Option<Vec<i32>>,
    /// Tint change per FE tick (retail 15).
    #[serde(default)]
    pub tint_step: Option<i32>,
}

impl BackdropOptions {
    pub fn validate(&self) -> bool {
        self.strength.is_none_or(|v| v.is_finite() && (0.0..=64.0).contains(&v))
            && self.step.is_none_or(|v| v.is_finite() && v > 0.0 && v <= 64.0)
            && self.tint.as_ref().is_none_or(|t| t.len() == 3 && t.iter().all(|c| (0..=255).contains(c)))
            && self.tint_step.is_none_or(|v| (1..=255).contains(&v))
    }
}
