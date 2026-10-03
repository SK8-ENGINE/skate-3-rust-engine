//! Validated engine primitives shared by arbitrary Lua mods.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuOptions { pub title: String, #[serde(default)] pub section: Option<String>, pub items: Vec<MenuItem> }
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub id: String, pub label: String,
    #[serde(default)] pub description: String,
    #[serde(default="enabled")] pub enabled: bool,
    #[serde(default)] pub children: Vec<MenuItem>,
}
fn enabled() -> bool { true }
impl MenuOptions {
    pub fn validate(&self) -> bool {
        fn rows(items: &[MenuItem], depth: usize, ids: &mut BTreeSet<String>) -> bool {
            depth <= 4 && items.iter().all(|item| {
                crate::schema::valid_id(&item.id) && ids.insert(item.id.clone()) && ids.len() <= 64
                    && label(&item.label,96) && item.description.len() <= 512
                    && (item.children.is_empty() || rows(&item.children,depth+1,ids))
            })
        }
        self.section.as_ref().is_none_or(|s| label(s,32)) && label(&self.title,96) && !self.items.is_empty() && rows(&self.items,1,&mut BTreeSet::new())
    }
}
fn label(text: &str, max: usize) -> bool { !text.trim().is_empty() && text.len()<=max && !text.chars().any(char::is_control) }

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointOverride {
    #[serde(default)] pub swing_limit: Option<f32>,
    #[serde(default)] pub twist_limit: Option<f32>,
    #[serde(default)] pub free_swing: Option<bool>,
    #[serde(default)] pub free_twist: Option<bool>,
    #[serde(default)] pub drive_enabled: Option<bool>,
    #[serde(default)] pub enabled: Option<bool>,
    #[serde(default)] pub descendants: bool,
    #[serde(default)] pub possession_enabled: Option<bool>,
}
impl JointOverride {
    pub fn validate(&self) -> bool {
        [self.swing_limit,self.twist_limit].iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.01..=std::f32::consts::PI).contains(&v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_tree_rejects_ambiguous_actions_and_invalid_depth() {
        let mut m:MenuOptions=serde_json::from_value(serde_json::json!({"title":"Challenges","items":[{"id":"races","label":"Races","children":[{"id":"sprint","label":"Sprint"}]}]})).unwrap();
        assert!(m.validate());
        m.items[0].children[0].id="races".into();
        assert!(!m.validate());
    }
    #[test]
    fn free_joint_is_valid_but_nonphysical_angles_are_rejected() {
        let mut o=JointOverride {free_swing:Some(true),free_twist:Some(true),drive_enabled:Some(false),..Default::default()};
        assert!(o.validate());
        o.swing_limit=Some(f32::NAN); assert!(!o.validate());
        o.swing_limit=Some(-1.); assert!(!o.validate());
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBodyRef { pub kind: String, pub index: usize }
impl NativeBodyRef {
    pub fn validate(&self) -> bool { match self.kind.as_str() { "skater"=>self.index<26,"board"=>self.index<7,_=>false } }
}

#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartOverride {
    pub motion:Option<String>, pub collision:Option<bool>, pub friction:Option<f32>,
    pub animation_drives:Option<bool>, pub possession_drives:Option<bool>,
}
impl PartOverride {
    pub fn validate(&self)->bool {self.motion.as_deref().is_none_or(|m|matches!(m,"dynamic"|"frozen"|"static")) && self.friction.is_none_or(|f|f.is_finite() && (0.0..=10.0).contains(&f))}
}
