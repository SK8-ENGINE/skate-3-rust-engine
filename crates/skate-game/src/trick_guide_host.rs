//! The Trick Guide screen's natives (FETrickTutorial 825AE7E0, GetMenuItems, PlayerStance_IsRegular).
//! Details: docs/hails-additions/31-retail-menus.md#trick-guide-natives
use crate::apt_vm::{to_integer, Value, Vm};
use skate_core::trick_guide::{Stance, TrickGuide, TrickGuideState};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct TrickGuideHost {
    pub guide: Arc<TrickGuide>,
    pub state: TrickGuideState,
    pub stance: Stance,
    /// The screen is on top of the stack (its picker callbacks are ours).
    pub open: bool,
    /// Screen states to post to the movie, in order.
    pub states: Vec<i32>,
}

impl TrickGuideHost {
    pub fn new(guide: Arc<TrickGuide>) -> Self {
        Self { state: TrickGuideState::open(&guide), guide, ..Default::default() }
    }

    /// Screen init (0x82618A40): level 0, row 0.
    pub fn reset(&mut self) {
        self.state = TrickGuideState::open(&self.guide);
        self.states.clear();
    }

    /// One native call; None = not ours.
    pub fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let index = |i: usize| args.get(i).map_or(0, to_integer).max(0) as usize;
        let guide = &*self.guide;
        let text = |s: String| Value::Text(s);
        Ok(Some(match (object, method) {
            // 825AE8A8: [level, has submenu, description id].
            ("FETrickTutorial", "GetMenuItemData") => {
                let values = vec![
                    Value::Number(self.state.level as f64),
                    Value::Bool(self.state.has_submenu(guide)),
                    text(self.state.description(guide)),
                ];
                Value::Object(vm.array(values)?)
            }
            // 825AE968: [used count, then gesture, button, connector per slot] (13 values).
            ("FETrickTutorial", "GetMenuItemGesturesButtons") => {
                let (used, slots) = self.state.gestures_buttons(guide, self.stance);
                let mut values = vec![Value::Number(used as f64)];
                values.extend(slots.into_iter().flatten().map(Value::Text));
                Value::Object(vm.array(values)?)
            }
            // 825AEBF0: always false in retail (the stance swap comes from PlayerStance_IsRegular).
            ("FETrickTutorial", "IsGoofy") => Value::Bool(false),
            ("FETrickTutorial", "GetCurrentMenuItem") => Value::Number(self.state.index as f64),
            // 826635A0: CAS byte +120 of the player's skater record.
            ("HUDComponents", "PlayerStance_IsRegular") => Value::Bool(self.stance.regular),
            ("Game", "GetMenuItems") if self.open => {
                let items = self.state.items(guide).into_iter().map(Value::Text).collect();
                Value::Object(vm.array(items)?)
            }
            // 825A5078 -> screen vt+72 = 0x8281DD70 (base): every row enabled.
            ("Game", "OnMenuGetItemEnabled") if self.open => Value::Bool(true),
            ("Game", "OnMenuHilight") if self.open => {
                let state = self.state.hilight(guide, index(1));
                self.states.push(state);
                Value::Undefined
            }
            ("Game", "OnMenuSelect") if self.open => {
                let state = self.state.select(guide, index(1));
                self.states.push(state);
                Value::Undefined
            }
            _ => return Ok(None),
        }))
    }
}

/// Mods' Trick Guide changes (`sdk.menus.trick_guide`), applied in order over the retail guide; `generation` tells
/// the stage to rebuild.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct TrickGuideMods {
    layers: Vec<(String, skate_core::trick_guide::EntryKind, skate_core::trick_guide::Key, skate_core::trick_guide::EntryPatch)>,
    pub generation: u64,
}

/// A mod key: a retail record's 16 hex digits, else the id hashed like an Attrib name.
fn mod_key(key: &str) -> skate_core::trick_guide::Key {
    let name = if key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit()) { format!("Hash_{key}") } else { key.to_owned() };
    let numeric = skate_data::attrib_hash::numeric_name(&name);
    u64::from_str_radix(numeric.trim_start_matches("Hash_"), 16).unwrap_or(0)
}

impl TrickGuideMods {
    pub(crate) fn set(&mut self, owner: &str, entry: &str, key: &str, options: Option<skate_mods::menus::TrickGuideOptions>) {
        use skate_core::trick_guide::{EntryKind, EntryPatch, Slot};
        let kind = match entry {
            "category" => EntryKind::Category,
            "subcategory" => EntryKind::Subcategory,
            _ => EntryKind::Trick,
        };
        let key = mod_key(key);
        self.layers.retain(|(o, k, id, _)| !(o == owner && *k == kind && *id == key));
        if let Some(o) = options {
            let patch = EntryPatch {
                label: o.label,
                description: o.description,
                slots: o.slots.map(|s| s.iter().map(|s| Slot { gesture: s.gesture, button: s.button, connector: s.connector }).collect()),
                animation: o.animation,
                camera_track: o.camera_track,
                parent: o.parent.as_deref().map(mod_key),
                order: o.order,
                has_tricks: o.has_tricks,
                position: o.position.map(|p| p as usize),
                hidden: o.hidden,
            };
            self.layers.push((owner.to_owned(), kind, key, patch));
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
    /// The retail guide with every mod layer applied.
    pub(crate) fn apply(&self, base: &TrickGuide) -> TrickGuide {
        let mut guide = base.clone();
        for (_, kind, key, patch) in &self.layers {
            guide.apply(*kind, *key, patch);
        }
        guide
    }
}
