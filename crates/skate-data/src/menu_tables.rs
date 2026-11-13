//! The front-end menu tables from an unpacked retail executable, written by
//! setup as `assets/private/menu-tables.json` (`skate3rust --extract-menu-tables`).
//!
//! Every table is located by its content or by the code that reads it, never by
//! a fixed address, so any build of the executable works (TU3 addresses in
//! docs/hails-additions are for verification only):
//! - settings rows: (string id, widget type) pairs found through the pointer to
//!   `ID_GAMESETTINGS_SFXVOLUME` next to a `slider` pointer; the settings screens
//!   (16 row indices each, 0-terminated) follow the rows;
//! - crossbar items: 20-byte records (name, label, icon, sub-option kind, help
//!   text) found through the pointer to `ReplayEditor`; the categories (12-byte
//!   records name, label, icon) end right before them;
//! - Apt key table: 20-byte records (input action, name, Apt key code, released
//!   flag, kind) found through the pointer to `AptStart`;
//! - per-mode menus: each crossbar mode class has a vtable whose slots hold a
//!   tab accessor (`tabs[r4]`), a row accessor (`rows[r4 * slots + r5]`), the tab
//!   count (`li r3,n`) and the mode title (a string id). The accessors are tiny
//!   leaf functions; a small evaluator runs them to read the table addresses and
//!   the row stride (8 slots normally, 9 in the skate-park variants).
//!
//! No game data lives in this module: the JSON is produced from the user's disc.
use crate::xex::XexImage;
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
const WIDGETS: [&str; 3] = ["option", "slider", "selector"];
const SCREEN_ROWS: usize = 16;
const ITEM_RECORD: u32 = 20;
const CATEGORY_RECORD: u32 = 12;
const KEY_RECORD: u32 = 20;
/// Instructions the accessor evaluator runs before giving up.
const LEAF_LIMIT: usize = 12;
/// Vtable slots after the tab accessor searched for the tab count and title.
const VTABLE_SPAN: usize = 9;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsRow {
    /// Language string id (`ID_GAMESETTINGS_*`); retail leaves a few rows blank.
    pub id: String,
    /// `option` (a link or action), `slider` or `selector`.
    pub widget: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub label: String,
    pub icon: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub label: String,
    pub icon: String,
    /// Sub-option list kind (a second column), -1 = none.
    pub sub_option_kind: i32,
    pub help: String,
}

/// A front-end screen template (retail table 0x8302F1A8 in TU3, 16-byte records {name, swf path, type, -1}):
/// the screen manager registers them in table order, so a screen's AS id is 600 + its index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenTemplate {
    pub name: String,
    /// Path under `source/screens/` (`main/core_menu.swf`); empty for the "Ignore" record.
    pub path: String,
    pub kind: u32,
}

/// One FE screen stack record. Numeric ids differ between builds (TU3 inserts two), so `id` is the identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeScreen {
    /// Stable identity: `key`, or `key:n` (n = position among the records sharing that key) when shared.
    pub id: String,
    /// The 64-bit value at +16 as 16 hex digits (posted as the focus message; four keys are shared by 2 to 5
    /// screens, e.g. TU3 ids 0 / 32 / 49).
    pub key: String,
    /// Byte +4: the world behind this screen blurs while it is on top (the pause crossbar has it set).
    pub blur: bool,
    /// Byte +0: open suspends gameplay (context 3), close resumes it.
    #[serde(default)]
    pub suspends_gameplay: bool,
    /// Byte +1: the chyron shows while it is open.
    #[serde(default)]
    pub chyron: bool,
    /// Word +8: HUD mode while it is the main stack's top (0 on, 1 off, 2 online HUDs only, 3 keep).
    #[serde(default = "hud_off")]
    pub hud: i32,
    /// Byte +12: pad input its own handler leaves goes on to the movie's UpdateInput.
    #[serde(default)]
    pub input_to_movie: bool,
}

fn hud_off() -> i32 {
    1
}

/// One row of the career stats table (the Stats object's +0x18 table, TU3 0x83043AE8, 12-byte records: enabled
/// byte, label string id, value): the loading screen's Stats panel and the Skate Profile read it (doc 31 "Loading
/// screen (FE 52)", research b589).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatRow {
    pub label: String,
    pub enabled: bool,
}

/// One sub-option kind's row: label and helper string ids by column ("" = empty).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubOptionRow {
    pub labels: Vec<String>,
    pub helpers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AptKey {
    pub name: String,
    /// The game input action polled for this key.
    pub action: u32,
    /// The Apt key code in the record (record + 8). Retail does NOT register this: AptUp etc. are set to the
    /// key's INDEX in the table (ScreenManagerReady 825D9450), the same index UpdateInput passes.
    pub apt_key: u32,
    pub released: bool,
    pub kind: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeTab {
    /// Category name (`SinglePlayer`, `Multiplayer`, ...).
    pub category: String,
    /// Item names in display order.
    pub items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeMenu {
    /// Stable key (see `mode_keys`): Career, CareerPark, FreePlay, FreePlayPark,
    /// FreeSkateRestricted, or the title without `ID_CROSSBAR_` / `_MODE_TITLE`.
    pub key: String,
    /// The mode's title string id.
    pub title: String,
    /// Row slots per tab (8, or 9 in the skate-park variants).
    pub slots: u32,
    pub tabs: Vec<ModeTab>,
    /// Where the vtable was found (diagnostics only).
    pub vtable: String,
}

/// The Trick Guide screen's tables (FE screen 14, template TrickTutorial; doc 31 "Trick Guide").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrickGuideTables {
    /// The top-level list in menu order: Attrib keys of `scoring_trickcategory` records, 16 hex digits (TU3
    /// 0x830274B0, 8 keys; read by the list fill sub_82619368 and sub_82618C10).
    pub categories: Vec<String>,
    /// Gesture animation names by a slot's gesture id (TU3 0x830273C8, 58 names; GetMenuItemGesturesButtons
    /// 825AE968).
    pub gestures: Vec<String>,
    /// Button names by a slot's button id (TU3 0x83027350, 28 names; sub_82619FC0).
    pub buttons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuTables {
    pub schema: u32,
    pub source_sha256: String,
    pub settings_rows: Vec<SettingsRow>,
    /// Row indices per settings screen.
    pub settings_screens: Vec<Vec<u32>>,
    pub categories: Vec<Category>,
    pub items: Vec<Item>,
    pub keys: Vec<AptKey>,
    pub modes: Vec<ModeMenu>,
    /// Screen templates in registration order (empty in tables extracted before they were added).
    #[serde(default)]
    pub templates: Vec<ScreenTemplate>,
    /// FE screens in id order (screen stack info table, 24-byte records; TU3 0x83026088, 72 records, the disc
    /// build has 70). Empty in tables extracted before it was added.
    #[serde(default)]
    pub screens: Vec<FeScreen>,
    /// Game Settings screen titles per settings screen index (string ids; TU3 0x83026E90, 13 pointers, read by
    /// GameSettings_GetScreenTitle 825A6DA8). Empty in tables extracted before they were added.
    #[serde(default)]
    pub settings_titles: Vec<String>,
    /// Crossbar sub-option rows per sub-option kind (0 parks, 1 contacts, 2 host options, 3 Skate Reel, 4 on-screen
    /// help, 5 online team, 6 party): labels (TU3 0x83027958) and helper texts (0x83027A00), 7 x 6 string ids,
    /// "" for an empty slot. Empty in tables extracted before they were added.
    #[serde(default)]
    pub sub_options: Vec<SubOptionRow>,
    /// The Trick Guide's category order and slot name tables. Empty in tables extracted before they were added.
    #[serde(default)]
    pub trick_guide: TrickGuideTables,
    /// The career stats rows in index order (65 in TU3). Empty in tables extracted before they were added.
    #[serde(default)]
    pub career_stats: Vec<StatRow>,
    /// Table addresses in this build (diagnostics only).
    pub addresses: std::collections::BTreeMap<String, String>,
}

impl MenuTables {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("menu tables serialize")
    }
    pub fn from_json(text: &str) -> Result<Self, String> {
        let tables: Self = serde_json::from_str(text).map_err(|e| format!("menu tables: {e}"))?;
        if tables.schema != SCHEMA {
            return Err(format!("menu tables: schema {} (expected {SCHEMA})", tables.schema));
        }
        Ok(tables)
    }
    pub fn mode(&self, key: &str) -> Option<&ModeMenu> {
        self.modes.iter().find(|m| m.key == key)
    }
}

struct Image<'a> {
    image: &'a XexImage,
}

impl Image<'_> {
    fn word(&self, address: u32) -> Option<u32> {
        self.image.at(address, 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn end(&self) -> u32 {
        self.image.base_address.wrapping_add(self.image.image.len() as u32)
    }
    fn inside(&self, address: u32) -> bool {
        address >= self.image.base_address && address < self.end()
    }
    /// A NUL-terminated printable ASCII string (possibly empty).
    fn cstr(&self, address: u32) -> Option<String> {
        if !self.inside(address) {
            return None;
        }
        let start = (address - self.image.base_address) as usize;
        let bytes = &self.image.image[start..(start + 256).min(self.image.image.len())];
        let end = bytes.iter().position(|b| *b == 0)?;
        let text = &bytes[..end];
        text.iter().all(|b| (0x20..0x7f).contains(b)).then(|| String::from_utf8_lossy(text).into_owned())
    }
    fn string_at(&self, pointer: u32) -> Option<String> {
        self.word(pointer).and_then(|p| self.cstr(p))
    }
    /// Addresses of `text` as a whole C string.
    fn strings(&self, text: &str) -> Vec<u32> {
        let needle: Vec<u8> = text.bytes().chain([0]).collect();
        let data = &self.image.image;
        let mut found = Vec::new();
        let mut at = 0;
        while let Some(i) = find(&data[at..], &needle) {
            let offset = at + i;
            if offset == 0 || data[offset - 1] == 0 {
                found.push(self.image.base_address + offset as u32);
            }
            at = offset + 1;
        }
        found
    }
    /// Aligned words equal to `value`.
    fn pointers_to(&self, value: u32) -> Vec<u32> {
        let needle = value.to_be_bytes();
        self.image
            .image
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| self.image.base_address + (i * 4) as u32)
            .collect()
    }
    fn pointers_to_string(&self, text: &str) -> Vec<u32> {
        self.strings(text).into_iter().flat_map(|s| self.pointers_to(s)).collect()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn hex(address: u32) -> String {
    format!("0x{address:08X}")
}

// ---------------------------------------------------------------- settings

fn settings_row(img: &Image, at: u32) -> Option<SettingsRow> {
    let widget = img.string_at(at + 4)?;
    if !WIDGETS.contains(&widget.as_str()) {
        return None;
    }
    let pointer = img.word(at)?;
    let id = if pointer == 0 { String::new() } else { img.cstr(pointer)? };
    Some(SettingsRow { id, widget })
}

fn settings(img: &Image) -> Result<(u32, Vec<SettingsRow>, Vec<Vec<u32>>), String> {
    let anchors: Vec<u32> = img
        .pointers_to_string("ID_GAMESETTINGS_SFXVOLUME")
        .into_iter()
        .filter(|p| img.string_at(p + 4).as_deref() == Some("slider"))
        .collect();
    let [anchor] = anchors.as_slice() else {
        return Err(format!("settings rows: {} anchors", anchors.len()));
    };
    let mut start = *anchor;
    while settings_row(img, start - 8).is_some() {
        start -= 8;
    }
    let mut rows = Vec::new();
    let mut at = start;
    while let Some(row) = settings_row(img, at) {
        rows.push(row);
        at += 8;
    }
    let mut screens = Vec::new();
    loop {
        let values: Option<Vec<u32>> = (0..SCREEN_ROWS as u32).map(|k| img.word(at + k * 4)).collect();
        let Some(values) = values else { break };
        if values.iter().any(|v| *v as usize >= rows.len()) {
            break;
        }
        let list: Vec<u32> = values.iter().copied().take_while(|v| *v != 0).collect();
        if list.is_empty() {
            break;
        }
        screens.push(list);
        at += (SCREEN_ROWS * 4) as u32;
    }
    if rows.len() < 8 || screens.is_empty() {
        return Err(format!("settings tables incomplete: {} rows, {} screens", rows.len(), screens.len()));
    }
    Ok((start, rows, screens))
}

// ---------------------------------------------------------------- crossbar

fn item(img: &Image, at: u32) -> Option<Item> {
    let kind = img.word(at + 12)? as i32;
    if !(-1..=64).contains(&kind) {
        return None;
    }
    let item = Item {
        name: img.string_at(at)?,
        label: img.string_at(at + 4)?,
        icon: img.string_at(at + 8)?,
        sub_option_kind: kind,
        help: img.string_at(at + 16)?,
    };
    (!item.name.is_empty() && !item.label.is_empty()).then_some(item)
}

fn category(img: &Image, at: u32) -> Option<Category> {
    let category = Category { name: img.string_at(at)?, label: img.string_at(at + 4)?, icon: img.string_at(at + 8)? };
    // Name and icon are identifiers (`SinglePlayer`, `singleplayer`); string ids follow
    // the disc build's categories.
    let identifier = |s: &str| !s.is_empty() && !s.starts_with("ID_") && s.bytes().all(|b| b.is_ascii_alphanumeric());
    (identifier(&category.name) && identifier(&category.icon) && category.label.starts_with("ID_CROSSBAR_")).then_some(category)
}

fn crossbar(img: &Image) -> Result<(u32, Vec<Item>, u32, Vec<Category>), String> {
    let anchors: Vec<u32> = img
        .pointers_to_string("ReplayEditor")
        .into_iter()
        .filter(|p| item(img, *p).is_some())
        .collect();
    let [anchor] = anchors.as_slice() else {
        return Err(format!("crossbar items: {} anchors", anchors.len()));
    };
    let mut start = *anchor;
    while item(img, start - ITEM_RECORD).is_some() {
        start -= ITEM_RECORD;
    }
    let mut items = Vec::new();
    let mut at = start;
    while let Some(item) = item(img, at) {
        items.push(item);
        at += ITEM_RECORD;
    }
    // The categories sit next to the items: right before them (TU3) or right
    // after them (the disc build).
    let mut first = start;
    while category(img, first - CATEGORY_RECORD).is_some() {
        first -= CATEGORY_RECORD;
    }
    let (first, last) = if first < start {
        (first, start)
    } else {
        let mut end = at;
        while category(img, end).is_some() {
            end += CATEGORY_RECORD;
        }
        (at, end)
    };
    let categories: Vec<Category> = (first..last).step_by(CATEGORY_RECORD as usize).filter_map(|a| category(img, a)).collect();
    if items.len() < 8 || categories.is_empty() {
        return Err(format!("crossbar tables incomplete: {} items, {} categories", items.len(), categories.len()));
    }
    Ok((start, items, first, categories))
}

// ---------------------------------------------------------------- Apt keys

fn key(img: &Image, at: u32) -> Option<AptKey> {
    let name = img.string_at(at + 4)?;
    let released = img.word(at + 12)?;
    let kind = img.word(at + 16)?;
    (name.starts_with("Apt") && released <= 1 && kind <= 16).then(|| AptKey {
        name,
        action: img.word(at).unwrap_or(0),
        apt_key: img.word(at + 8).unwrap_or(0),
        released: released == 1,
        kind,
    })
}

fn keys(img: &Image) -> Result<(u32, Vec<AptKey>), String> {
    let anchors: Vec<u32> = img
        .pointers_to_string("AptStart")
        .into_iter()
        .map(|p| p - 4)
        .filter(|r| key(img, *r).is_some())
        .collect();
    let [anchor] = anchors.as_slice() else {
        return Err(format!("Apt key table: {} anchors", anchors.len()));
    };
    let mut start = *anchor;
    while key(img, start - KEY_RECORD).is_some() {
        start -= KEY_RECORD;
    }
    let mut keys = Vec::new();
    let mut at = start;
    while let Some(key) = key(img, at) {
        keys.push(key);
        at += KEY_RECORD;
    }
    Ok((start, keys))
}

// ---------------------------------------------------------------- modes

#[derive(Debug, PartialEq)]
enum Leaf {
    /// The function returns the word loaded from this address.
    Load(u32),
    /// The function returns this value in r3.
    Value(u32),
}

/// Runs a small leaf function with r4 / r5 set; only the handful of integer
/// instructions table accessors use.
fn leaf(img: &Image, function: u32, r4: u32, r5: u32) -> Option<Leaf> {
    let mut regs: [Option<u32>; 32] = [None; 32];
    regs[4] = Some(r4);
    regs[5] = Some(r5);
    let mut loaded: Option<(usize, u32)> = None;
    for k in 0..LEAF_LIMIT {
        let w = img.word(function.checked_add(k as u32 * 4)?)?;
        if w == 0x4e80_0020 {
            return match loaded {
                Some((3, address)) => Some(Leaf::Load(address)),
                _ => regs[3].map(Leaf::Value),
            };
        }
        let (op, rd, ra, rb) = (w >> 26, ((w >> 21) & 31) as usize, ((w >> 16) & 31) as usize, ((w >> 11) & 31) as usize);
        let simm = (w & 0xffff) as u16 as i16 as i32 as u32;
        let base = |regs: &[Option<u32>; 32]| if ra == 0 { Some(0) } else { regs[ra] };
        match op {
            14 => regs[rd] = Some(base(&regs)?.wrapping_add(simm)),
            15 => regs[rd] = Some(base(&regs)?.wrapping_add(simm << 16)),
            7 => regs[rd] = Some(regs[ra]?.wrapping_mul(simm)),
            24 => regs[ra] = Some(regs[rd]? | (w & 0xffff)),
            21 => {
                let (sh, mb, me) = ((w >> 11) & 31, (w >> 6) & 31, (w >> 1) & 31);
                let mask = if mb <= me {
                    (u32::MAX >> mb) & (u32::MAX << (31 - me))
                } else {
                    (u32::MAX >> mb) | (u32::MAX << (31 - me))
                };
                regs[ra] = Some(regs[rd]?.rotate_left(sh) & mask);
            }
            31 => match (w >> 1) & 1023 {
                266 => regs[rd] = Some(regs[ra]?.wrapping_add(regs[rb]?)),
                23 => {
                    let address = base(&regs)?.wrapping_add(regs[rb]?);
                    loaded = Some((rd, address));
                    regs[rd] = img.word(address);
                }
                444 if rd == rb => regs[ra] = regs[rd], // mr
                _ => return None,
            },
            32 => {
                let address = base(&regs)?.wrapping_add(simm);
                loaded = Some((rd, address));
                regs[rd] = img.word(address);
            }
            _ => return None,
        }
    }
    None
}

/// (table address, row stride in words) when `function` reads `table[r4]`
/// (stride 1) or `table[r4 * stride + r5]`.
fn accessor(img: &Image, function: u32, two_d: bool) -> Option<(u32, u32)> {
    let Leaf::Load(origin) = leaf(img, function, 0, 0)? else { return None };
    if two_d {
        let Leaf::Load(next_slot) = leaf(img, function, 0, 1)? else { return None };
        let Leaf::Load(next_row) = leaf(img, function, 1, 0)? else { return None };
        let stride = next_row.wrapping_sub(origin) / 4;
        (next_slot == origin + 4 && next_row > origin && (2..=16).contains(&stride)).then_some((origin, stride))
    } else {
        let Leaf::Load(next) = leaf(img, function, 1, 0)? else { return None };
        (next == origin + 4).then_some((origin, 1))
    }
}

struct RawMode {
    vtable: u32,
    title: String,
    slots: u32,
    tabs: Vec<(i32, Vec<i32>)>,
}

fn raw_modes(img: &Image) -> Vec<RawMode> {
    let words: Vec<u32> = img
        .image
        .image
        .chunks_exact(4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let base = img.image.base_address;
    let code = |w: u32| w % 4 == 0 && img.inside(w);
    let mut modes = Vec::new();
    for i in 0..words.len().saturating_sub(VTABLE_SPAN + 1) {
        let (tabs_fn, rows_fn) = (words[i], words[i + 1]);
        if !code(tabs_fn) || !code(rows_fn) || tabs_fn == rows_fn {
            continue;
        }
        let Some((tabs_at, _)) = accessor(img, tabs_fn, false) else { continue };
        let Some((rows_at, slots)) = accessor(img, rows_fn, true) else { continue };
        let (mut count, mut title) = (None, None);
        for &slot in &words[i + 2..i + 1 + VTABLE_SPAN] {
            if !code(slot) {
                continue;
            }
            // Only two-instruction getters (`li r3,n` / `lis+addi r3`; blr).
            let short = img.word(slot + 4) == Some(0x4e80_0020) || img.word(slot + 8) == Some(0x4e80_0020);
            match (short, leaf(img, slot, 0, 0)) {
                (true, Some(Leaf::Value(n))) if (1..=16).contains(&n) => {
                    count.get_or_insert(n);
                }
                (true, Some(Leaf::Value(p))) => {
                    if let Some(text) = img.cstr(p).filter(|t| !t.is_empty()) {
                        title.get_or_insert(text);
                    }
                }
                _ => {}
            }
        }
        let (Some(count), Some(title)) = (count, title) else { continue };
        let read = |a: u32| img.word(a).map(|w| w as i32);
        let tabs: Option<Vec<(i32, Vec<i32>)>> = (0..count)
            .map(|t| {
                let category = read(tabs_at + t * 4)?;
                let row: Option<Vec<i32>> = (0..slots).map(|s| read(rows_at + (t * slots + s) * 4)).collect();
                Some((category, row?.into_iter().take_while(|v| *v != -1).collect()))
            })
            .collect();
        if let Some(tabs) = tabs {
            // The tab accessor is the slot after the destructor.
            modes.push(RawMode { vtable: base + (i as u32) * 4 - 4, title, slots, tabs });
        }
    }
    modes
}

/// Stable keys. Retail has no names for the mode classes; Career / Free Play
/// share their titles with their park variants (9-slot rows) and Free Play with
/// the restricted free-skate menu (no ResumeCareer on its Main tab).
fn mode_key(mode: &ModeMenu) -> String {
    let main_has = |name: &str| {
        mode.tabs.iter().any(|t| t.category == "SinglePlayer" && t.items.iter().any(|i| i == name))
    };
    match (mode.title.as_str(), mode.slots) {
        ("ID_CROSSBAR_SINGLE_PLAYER_MODE_TITLE", 9) => "CareerPark".into(),
        ("ID_CROSSBAR_SINGLE_PLAYER_MODE_TITLE", _) => "Career".into(),
        ("ID_CROSSBAR_OFFLINE_FREESKATE_MODE_TITLE", 9) => "FreePlayPark".into(),
        ("ID_CROSSBAR_OFFLINE_FREESKATE_MODE_TITLE", _) if main_has("ResumeCareer") => "FreePlay".into(),
        ("ID_CROSSBAR_OFFLINE_FREESKATE_MODE_TITLE", _) => "FreeSkateRestricted".into(),
        (title, _) => {
            let core = title.trim_start_matches("ID_CROSSBAR_").trim_end_matches("_MODE_TITLE");
            core.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
        }
    }
}

fn modes(img: &Image, categories: &[Category], items: &[Item]) -> Vec<ModeMenu> {
    let name = |list: &[String], i: i32| usize::try_from(i).ok().and_then(|i| list.get(i)).cloned();
    let category_names: Vec<String> = categories.iter().map(|c| c.name.clone()).collect();
    let item_names: Vec<String> = items.iter().map(|c| c.name.clone()).collect();
    let mut out: Vec<ModeMenu> = Vec::new();
    for raw in raw_modes(img) {
        let tabs: Option<Vec<ModeTab>> = raw
            .tabs
            .iter()
            .map(|(category, row)| {
                Some(ModeTab {
                    category: name(&category_names, *category)?,
                    items: row.iter().map(|i| name(&item_names, *i)).collect::<Option<_>>()?,
                })
            })
            .collect();
        let Some(tabs) = tabs else { continue };
        let mut mode = ModeMenu { key: String::new(), title: raw.title, slots: raw.slots, tabs, vtable: hex(raw.vtable) };
        let key = mode_key(&mode);
        let taken = |k: &str| out.iter().any(|m| m.key == k);
        mode.key = if taken(&key) { (2..).map(|n| format!("{key}_{n}")).find(|k| !taken(k)).unwrap() } else { key };
        out.push(mode);
    }
    out
}

// ---------------------------------------------------------------- screen templates

fn template(img: &Image, at: u32) -> Option<ScreenTemplate> {
    let name = img.string_at(at)?;
    let path_ptr = img.word(at + 4)?;
    let path = if path_ptr == 0 { String::new() } else { img.cstr(path_ptr)? };
    let kind = img.word(at + 8)?;
    (!name.is_empty() && (path.is_empty() || path.ends_with(".swf")) && kind <= 1 && img.word(at + 12)? == u32::MAX)
        .then_some(ScreenTemplate { name, path, kind })
}

/// Located by content: the record whose path is "main/core_menu.swf" (the pause menu), then every valid record
/// before and after it.
fn templates(img: &Image) -> Result<(u32, Vec<ScreenTemplate>), String> {
    let anchor = img
        .pointers_to_string("main/core_menu.swf")
        .into_iter()
        .map(|p| p - 4)
        .find(|at| template(img, *at).is_some())
        .ok_or("screen template table not found")?;
    let mut start = anchor;
    while template(img, start - 16).is_some() {
        start -= 16;
    }
    let mut out = Vec::new();
    let mut at = start;
    while let Some(t) = template(img, at) {
        out.push(t);
        at += 16;
    }
    Ok((start, out))
}

// ---------------------------------------------------------------- crossbar sub-options

const SUB_OPTION_KINDS: u32 = 7;
const SUB_OPTION_COLUMNS: u32 = 6;

/// Located by content: the label table starts with a pointer to "ID_CROSSBAR_NEW_PARK" (kind 0, column 0); the
/// helper table follows it directly (7 x 6 pointers each, null = empty).
fn sub_options(img: &Image) -> Result<(u32, Vec<SubOptionRow>), String> {
    let size = SUB_OPTION_KINDS * SUB_OPTION_COLUMNS * 4;
    let entry = |at: u32| -> Option<String> {
        match img.word(at)? {
            0 => Some(String::new()),
            p => img.cstr(p).filter(|s| s.starts_with("ID_")),
        }
    };
    let start = img
        .pointers_to_string("ID_CROSSBAR_NEW_PARK")
        .into_iter()
        .find(|at| (0..size * 2).step_by(4).all(|o| entry(at + o).is_some()))
        .ok_or("crossbar sub-option tables not found")?;
    let rows = (0..SUB_OPTION_KINDS)
        .map(|kind| {
            let cell = |table: u32, column: u32| entry(table + (kind * SUB_OPTION_COLUMNS + column) * 4).unwrap_or_default();
            SubOptionRow {
                labels: (0..SUB_OPTION_COLUMNS).map(|c| cell(start, c)).collect(),
                helpers: (0..SUB_OPTION_COLUMNS).map(|c| cell(start + size, c)).collect(),
            }
        })
        .collect();
    Ok((start, rows))
}

// ---------------------------------------------------------------- career stats

/// Located by content: the record whose label points at "ID_RECORD_DMO_01" and that starts a run of 12-byte records
/// (enabled byte 0 / 1, label "ID_RECORD_*"); the run ends at the first record that is not one.
fn career_stats(img: &Image) -> Result<(u32, Vec<StatRow>), String> {
    let row = |at: u32| -> Option<StatRow> {
        let enabled = *img.image.at(at, 1)?.first()?;
        let label = img.string_at(at + 4).filter(|l| l.starts_with("ID_RECORD_"))?;
        (enabled <= 1).then_some(StatRow { label, enabled: enabled == 1 })
    };
    img.pointers_to_string("ID_RECORD_DMO_01")
        .into_iter()
        .map(|label_at| label_at - 4)
        .map(|start| (start, (0..).map_while(|i| row(start + i * 12)).collect::<Vec<_>>()))
        .filter(|(_, rows)| rows.len() >= 40)
        .max_by_key(|(_, rows)| rows.len())
        .ok_or_else(|| "career stats table not found".into())
}

// ---------------------------------------------------------------- settings screen titles

/// Located by content: two consecutive pointers to "ID_GAMESETTINGS_TITLE" (screens 0 and 1), then one string id
/// per settings screen.
fn settings_titles(img: &Image, screens: usize) -> Result<(u32, Vec<String>), String> {
    let start = img
        .pointers_to_string("ID_GAMESETTINGS_TITLE")
        .into_iter()
        .find(|at| img.string_at(at + 4).as_deref() == Some("ID_GAMESETTINGS_TITLE"))
        .ok_or("settings screen title table not found")?;
    let titles: Vec<String> = (0..screens as u32)
        .map_while(|i| img.string_at(start + 4 * i).filter(|t| t.starts_with("ID_GAMESETTINGS_")))
        .collect();
    if titles.len() != screens {
        return Err(format!("settings screen title table has {} titles for {screens} screens", titles.len()));
    }
    Ok((start, titles))
}

// ---------------------------------------------------------------- FE screen info table

const SCREEN_INFO_RECORD: u32 = 24;

/// Located by code: the FE screen stack init (TU3 sub_825E8C40) loads the table with `lis rH,hi; li rN,count;
/// addi rT,rH,lo` and stores the count with `stw rN,132(rStack)` a few instructions later.
fn screen_info(img: &Image) -> Result<(u32, Vec<FeScreen>), String> {
    let words: Vec<u32> = img.image.image.chunks_exact(4).map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]])).collect();
    let op = |w: u32| w >> 26;
    let rd = |w: u32| (w >> 21) & 31;
    let ra = |w: u32| (w >> 16) & 31;
    let imm = |w: u32| w & 0xFFFF;
    let mut found = Vec::new();
    for i in 1..words.len().saturating_sub(8) {
        let (o, n_reg, a, count) = (op(words[i]), rd(words[i]), ra(words[i]), imm(words[i]));
        if o != 14 || a != 0 || !(16..=256).contains(&count) {
            continue;
        }
        let (lo_op, h_reg, lo) = (op(words[i + 1]), ra(words[i + 1]), imm(words[i + 1]));
        let (hi_op, hi_reg, hi_a, hi) = (op(words[i - 1]), rd(words[i - 1]), ra(words[i - 1]), imm(words[i - 1]));
        if lo_op != 14 || hi_op != 15 || hi_a != 0 || hi_reg != h_reg {
            continue;
        }
        let stores_count = words[i + 2..i + 8].iter().any(|w| op(*w) == 36 && rd(*w) == n_reg && imm(*w) == 132);
        if !stores_count {
            continue;
        }
        let table = (hi << 16).wrapping_add(lo as i16 as i32 as u32);
        let flags: Option<Vec<FeScreen>> = (0..count)
            .map(|id| {
                let record = img.image.at(table + id * SCREEN_INFO_RECORD, SCREEN_INFO_RECORD as usize)?;
                let key = record[16..24].iter().map(|b| format!("{b:02x}")).collect();
                let hud = i32::from_be_bytes([record[8], record[9], record[10], record[11]]);
                ([0, 1, 4, 12].iter().all(|i| record[*i] <= 1) && (0..=3).contains(&hud)).then(|| FeScreen {
                    id: String::new(),
                    key,
                    blur: record[4] == 1,
                    suspends_gameplay: record[0] == 1,
                    chyron: record[1] == 1,
                    hud,
                    input_to_movie: record[12] == 1,
                })
            })
            .collect();
        if let Some(mut flags) = flags {
            for i in 0..flags.len() {
                let key = flags[i].key.clone();
                let before = flags[..i].iter().filter(|s| s.key == key).count();
                let shared = flags.iter().filter(|s| s.key == key).count() > 1;
                flags[i].id = if shared { format!("{key}:{before}") } else { key };
            }
            found.push((table, flags));
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err("FE screen info table not found".into()),
        n => Err(format!("FE screen info table ambiguous ({n} candidates)")),
    }
}

// ---------------------------------------------------------------- trick guide

/// A table of string pointers around a pointer to `anchor`: entry 0 points at "", every other entry at a non-empty
/// string (walked back from the anchor to the "" entry, then forward to the last string).
fn name_table(img: &Image, anchor: &str) -> Option<(u32, Vec<String>)> {
    let named = |at: u32| img.string_at(at).filter(|s| !s.is_empty());
    img.pointers_to_string(anchor).into_iter().find_map(|at| {
        let mut start = at;
        while named(start - 4).is_some() {
            start -= 4;
        }
        start -= 4;
        if img.string_at(start).as_deref() != Some("") {
            return None;
        }
        let mut names = vec![String::new()];
        let mut cursor = start + 4;
        while let Some(name) = named(cursor) {
            names.push(name);
            cursor += 4;
        }
        Some((start, names))
    })
}

/// Located by code: the list fill (TU3 sub_82619368) and the level reset (sub_82618C10) both copy the category keys
/// with `lis rH,hi; addi rT,rH,lo; ...; ld; ...; addi rN,rN,8; addi rX,rT,size; cmpw rN,rX`. The one table copied that
/// way from two places; size / 8 keys.
fn trick_categories(img: &Image) -> Result<(u32, Vec<String>), String> {
    let words: Vec<u32> = img.image.image.chunks_exact(4).map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]])).collect();
    let op = |w: u32| w >> 26;
    let rd = |w: u32| (w >> 21) & 31;
    let ra = |w: u32| (w >> 16) & 31;
    let imm = |w: u32| w & 0xFFFF;
    let mut sites: std::collections::BTreeMap<(u32, u32), usize> = Default::default();
    for i in 40..words.len().saturating_sub(3) {
        let (step, end, cmp) = (words[i], words[i + 1], words[i + 2]);
        if op(step) != 14 || rd(step) != ra(step) || imm(step) != 8 {
            continue;
        }
        let (cursor, bound, table_reg, size) = (rd(step), rd(end), ra(end), imm(end));
        if op(end) != 14 || size % 8 != 0 || !(16..=256).contains(&size) {
            continue;
        }
        // cmpw cr, cursor, bound (X-form, xo 0).
        if op(cmp) != 31 || (cmp >> 1) & 0x3FF != 0 || ra(cmp) != cursor || (cmp >> 11) & 31 != bound {
            continue;
        }
        let body = &words[i - 40..i];
        let loads_u64 = body.iter().any(|w| op(*w) == 58 && w & 3 == 0);
        let Some(j) = body.iter().rposition(|w| op(*w) == 14 && rd(*w) == table_reg && ra(*w) != 0) else { continue };
        let hi_reg = ra(body[j]);
        let Some(hi) = body[j.saturating_sub(12)..j].iter().rev().find(|w| op(**w) == 15 && rd(**w) == hi_reg && ra(**w) == 0) else {
            continue;
        };
        if loads_u64 {
            let table = (imm(*hi) << 16).wrapping_add(imm(body[j]) as i16 as i32 as u32);
            *sites.entry((table, size / 8)).or_default() += 1;
        }
    }
    let found: Vec<(u32, u32)> = sites.into_iter().filter(|(_, n)| *n >= 2).map(|(k, _)| k).collect();
    let [(table, count)] = found[..] else {
        return Err(format!("trick guide category table not found ({} candidates)", found.len()));
    };
    let keys = (0..count)
        .map(|i| img.image.at(table + i * 8, 8).map(|b| b.iter().map(|b| format!("{b:02x}")).collect::<String>()))
        .collect::<Option<Vec<_>>>()
        .ok_or("trick guide category table outside the image")?;
    Ok((table, keys))
}

fn trick_guide(img: &Image) -> Result<(u32, u32, u32, TrickGuideTables), String> {
    let (categories_at, categories) = trick_categories(img)?;
    // Gesture names end with "revert"; button names include "Stick_Left_Up" (both located by content).
    let (gestures_at, gestures) = name_table(img, "revert").ok_or("trick guide gesture names not found")?;
    let (buttons_at, buttons) = name_table(img, "Stick_Left_Up").ok_or("trick guide button names not found")?;
    Ok((categories_at, gestures_at, buttons_at, TrickGuideTables { categories, gestures, buttons }))
}

pub fn extract(image: &XexImage) -> Result<MenuTables, String> {
    let img = Image { image };
    let (rows_at, settings_rows, settings_screens) = settings(&img)?;
    let (items_at, items, categories_at, categories) = crossbar(&img)?;
    let (keys_at, keys) = keys(&img)?;
    let modes = modes(&img, &categories, &items);
    let (templates_at, templates) = templates(&img)?;
    let (screen_info_at, screens) = screen_info(&img)?;
    let (titles_at, settings_titles) = settings_titles(&img, settings_screens.len())?;
    let (sub_options_at, sub_options) = sub_options(&img)?;
    let (trick_categories_at, gestures_at, buttons_at, trick_guide) = trick_guide(&img)?;
    let (stats_at, career_stats) = career_stats(&img)?;
    if modes.is_empty() {
        return Err("no per-mode crossbar menus found".into());
    }
    let addresses = [
        ("settings_rows", rows_at),
        ("settings_screens", rows_at + settings_rows.len() as u32 * 8),
        ("crossbar_categories", categories_at),
        ("crossbar_items", items_at),
        ("apt_keys", keys_at),
        ("screen_templates", templates_at),
        ("screen_info", screen_info_at),
        ("settings_titles", titles_at),
        ("sub_options", sub_options_at),
        ("trick_guide_categories", trick_categories_at),
        ("trick_guide_gestures", gestures_at),
        ("trick_guide_buttons", buttons_at),
        ("career_stats", stats_at),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), hex(v)))
    .collect();
    Ok(MenuTables {
        schema: SCHEMA,
        source_sha256: crate::sha256::digest(&image.image),
        settings_rows,
        settings_screens,
        categories,
        items,
        keys,
        modes,
        templates,
        screens,
        settings_titles,
        sub_options,
        trick_guide,
        career_stats,
        addresses,
    })
}

/// Unpacks `xex`, extracts the tables and returns the game's JSON.
pub fn json_from_xex(xex: &[u8]) -> Result<String, String> {
    let image = XexImage::parse(xex)?;
    Ok(extract(&image)?.to_json())
}

impl MenuTables {
    /// The engine menu structure with these tables as its defaults: entries keyed
    /// by retail internal names, settings rows by string id, retail links.
    pub fn to_menu_data(&self) -> skate_core::menus::MenuData {
        use skate_core::menus as m;
        let keys: Vec<String> =
            self.settings_rows.iter().enumerate().map(|(i, r)| m::setting_key(i, &r.id)).collect();
        m::MenuData {
            categories: self
                .categories
                .iter()
                .map(|c| m::Category { id: c.name.clone(), label: c.label.clone(), icon: m::Icon::Retail(c.icon.clone()) })
                .collect(),
            items: self
                .items
                .iter()
                .map(|i| m::Item {
                    id: i.name.clone(),
                    label: i.label.clone(),
                    icon: m::Icon::Retail(i.icon.clone()),
                    help: i.help.clone(),
                    sub_option_kind: i.sub_option_kind,
                })
                .collect(),
            settings: self
                .settings_rows
                .iter()
                .zip(&keys)
                .map(|(r, key)| m::Setting {
                    id: key.clone(),
                    label: r.id.clone(),
                    widget: m::Widget::parse(&r.widget).unwrap_or(m::Widget::Option),
                    link: m::retail_link(key),
                })
                .collect(),
            screens: self
                .settings_screens
                .iter()
                .map(|rows| rows.iter().filter_map(|r| keys.get(*r as usize).cloned()).collect())
                .collect(),
            modes: self
                .modes
                .iter()
                .map(|mode| m::Mode {
                    key: mode.key.clone(),
                    title: mode.title.clone(),
                    slots: mode.slots as usize,
                    tabs: mode
                        .tabs
                        .iter()
                        .map(|t| m::Tab { category: t.category.clone(), items: t.items.clone() })
                        .collect(),
                })
                .collect(),
            // The Challenge Map tree comes from the vault collections, not these tables (added by the game's loader).
            map_nodes: Vec::new(), map_pictures: Default::default(), map_markers: Default::default(), map_debug: Default::default(),
        }
    }
}
