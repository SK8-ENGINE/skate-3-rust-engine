use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub api: u32,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub entry: String,
    #[serde(default)]
    pub settings: BTreeMap<String, Setting>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub label: String,
    pub description: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
}
pub fn valid_id(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or("");
    let reserved = matches!(
        stem,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    );
    !reserved
        && !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || b == b'.'
        })
        && s != "."
        && s != ".."
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) || self.api != 1 {
            return Err("Invalid ID or unsupported API (expected 1)".into());
        }
        if self.name.is_empty()
            || self.name.len() > 120
            || self.author.len() > 120
            || self.description.len() > 2048
        {
            return Err("Invalid metadata length".into());
        }
        if self.version.split('.').count() != 3
            || self.version.split('.').any(|p| p.parse::<u32>().is_err())
        {
            return Err("version must be MAJOR.MINOR.PATCH".into());
        }
        if self.settings.len() > 24 {
            return Err("At most 24 settings".into());
        }
        for (key, s) in &self.settings {
            if !valid_id(key) || s.label.len() > 120 || s.description.len() > 512 {
                return Err(format!("Invalid setting {key}"));
            }
            if s.kind == "number"
                && !(s
                    .min
                    .zip(s.max)
                    .is_some_and(|(a, b)| a.is_finite() && b.is_finite() && a <= b)
                    && s.step.is_some_and(|v| v.is_finite() && v > 0.))
            {
                return Err(format!(
                    "{key}: numbers need finite min, max and positive step"
                ));
            }
            if s.kind == "choice"
                && (s.choices.is_empty()
                    || s.choices.len() > 32
                    || s.choices.iter().any(|s| s.len() > 128))
            {
                return Err(format!("{key}: invalid choices"));
            }
            if !s.accepts(&s.default) {
                return Err(format!("{key}: invalid type/default"));
            }
        }
        Ok(())
    }
}
impl Setting {
    pub fn accepts(&self, v: &Value) -> bool {
        match self.kind.as_str() {
            "boolean" => v.is_boolean(),
            "number" => v.as_f64().is_some_and(|v| {
                v.is_finite()
                    && self.min.is_some_and(|a| v >= a)
                    && self.max.is_some_and(|b| v <= b)
            }),
            "string" => v
                .as_str()
                .is_some_and(|s| s.chars().count() <= 128 && !s.chars().any(char::is_control)),
            "choice" => v
                .as_str()
                .is_some_and(|s| self.choices.iter().any(|c| c == s)),
            _ => false,
        }
    }
}
