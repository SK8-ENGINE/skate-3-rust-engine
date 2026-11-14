//! Tuning writes at run time (capability `audio_tuning` = 1; doc 16 "Tuning writes"):
//! `sdk.audio.set_tuning(domain, patch)` patches one of the install's typed tuning sections while
//! the mod runs. A patch is a field merge onto the section (the `audio.json` `tuning` shape):
//! objects by field, arrays by decimal index, a leaf replaced by a leaf of the same type (numbers by
//! numbers, booleans, strings, number arrays of the same length). Unlike the content overlay, a
//! runtime patch is strict: a field the install lacks, an index beyond the array or a leaf of
//! another type is an error, so the mod hears about a typo at once.
//!
//! Domains: `player` (`player_tuning`: the components' vault tuning: surfaces, grinds, seams,
//! landing / collision materials, tricks, treatment, contacts…), `world` (`world_tuning`: traffic
//! engine records, ped footsteps / objects, speech events and voice…), `bus` (`bus_tuning`: the
//! reverb presets, the eEQChain buses, the FlangeSub returns) and `reverb` (`bus_tuning.reverb`:
//! preset key → its 44 values). The MixMap's layout (instances, pools) is not a domain.
use serde_json::Value;

pub const DOMAINS: [&str; 4] = ["player", "world", "bus", "reverb"];
/// Leaves per patch, nesting depth, serialized size.
pub const MAX_LEAVES: usize = 512;
pub const MAX_DEPTH: usize = 8;
pub const MAX_PATCH_BYTES: usize = 64 * 1024;
/// Numbers a patch may write (finite, within ±1e9).
pub const MAX_NUMBER: f64 = 1.0e9;

/// The manifest section a domain patches, and the field inside it (`reverb`).
pub fn section(domain: &str) -> Option<(&'static str, Option<&'static str>)> {
    match domain {
        "player" => Some(("player_tuning", None)),
        "world" => Some(("world_tuning", None)),
        "bus" => Some(("bus_tuning", None)),
        "reverb" => Some(("bus_tuning", Some("reverb"))),
        _ => None,
    }
}

/// The patch as a patch of its whole section.
pub fn section_patch(domain: &str, patch: &Value) -> Option<(&'static str, Value)> {
    let (section, field) = section(domain)?;
    Some((section, match field {
        Some(f) => serde_json::json!({ f: patch }),
        None => patch.clone(),
    }))
}

fn key_ok(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && !k.contains("..") && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
}

fn shape(v: &Value, depth: usize, leaves: &mut usize) -> bool {
    match v {
        Value::Object(m) => depth < MAX_DEPTH && !m.is_empty() && m.iter().all(|(k, x)| key_ok(k) && shape(x, depth + 1, leaves)),
        Value::Array(a) => {
            *leaves += 1;
            a.len() <= 256 && a.iter().all(|x| x.as_f64().is_some_and(|n| n.is_finite() && n.abs() <= MAX_NUMBER))
        }
        Value::Number(n) => {
            *leaves += 1;
            n.as_f64().is_some_and(|n| n.is_finite() && n.abs() <= MAX_NUMBER)
        }
        Value::Bool(_) => {
            *leaves += 1;
            true
        }
        Value::String(s) => {
            *leaves += 1;
            s.len() <= 64
        }
        Value::Null => false,
    }
}

/// The checks that need no install: a known domain; a non-empty object of bounded depth, leaves and
/// size; keys of letters, digits, `_ . -`; finite numbers within ±1e9; no nulls.
pub fn valid_patch(domain: &str, patch: &Value) -> bool {
    let mut leaves = 0;
    DOMAINS.contains(&domain)
        && patch.is_object()
        && shape(patch, 0, &mut leaves)
        && leaves <= MAX_LEAVES
        && serde_json::to_vec(patch).is_ok_and(|b| b.len() <= MAX_PATCH_BYTES)
}

/// Apply `patch` to `target` (both a section, `path` its name): strict field merge (see the module
/// docs). `take(leaf path)` decides whether a leaf is written (the owners' claims: first mod wins);
/// leaves it refuses are skipped. Errors name the first field that does not fit the install.
pub fn apply(target: &mut Value, patch: &Value, path: &str, take: &mut dyn FnMut(&str) -> bool) -> Result<(), String> {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                let at = format!("{path}/{k}");
                let x = t.get_mut(k).ok_or_else(|| format!("tuning field {at} is not in the install"))?;
                apply(x, v, &at, take)?;
            }
            Ok(())
        }
        (Value::Array(t), Value::Object(p)) => {
            for (k, v) in p {
                let at = format!("{path}/{k}");
                let x = k.parse::<usize>().ok().and_then(|i| t.get_mut(i)).ok_or_else(|| format!("tuning index {at} is not in the install"))?;
                apply(x, v, &at, take)?;
            }
            Ok(())
        }
        (t, p) => {
            let same = matches!((&*t, p), (Value::Number(_), Value::Number(_)) | (Value::Bool(_), Value::Bool(_)) | (Value::String(_), Value::String(_)))
                || matches!((&*t, p), (Value::Array(a), Value::Array(b)) if a.iter().chain(b).all(Value::is_number) && a.len() == b.len());
            if !same {
                return Err(format!("tuning field {path}: {p} does not fit the install's {t}"));
            }
            if take(path) {
                *t = p.clone();
            }
            Ok(())
        }
    }
}

/// `sdk.engine.inspect(key, 'audio_tuning:<domain>[/<path>]')`: a domain and an optional path of
/// fields / indices inside it.
pub fn valid_inspect(system: &str) -> bool {
    let Some(rest) = system.strip_prefix("audio_tuning:") else { return false };
    let mut parts = rest.split('/');
    rest.len() <= 256 && parts.next().is_some_and(|d| DOMAINS.contains(&d)) && parts.all(key_ok)
}

/// The leaf paths a patch writes (`section/field/…`), for the owners' claims.
pub fn leaves(patch: &Value, path: &str, out: &mut Vec<String>) {
    match patch {
        Value::Object(m) => {
            for (k, v) in m {
                leaves(v, &format!("{path}/{k}"), out);
            }
        }
        _ => out.push(path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_are_checked_and_applied_strictly() {
        assert!(valid_patch("world", &json!({"traffic_engine": {"c04_taxi01": {"idle_rpm": 1200}}})));
        assert!(valid_patch("reverb", &json!({"BEEFC8E3DE04FBAE": {"3": 0.5}})));
        assert!(valid_patch("player", &json!({"grind": {"0": {"v": [0.5, 0.5, 0.5, 0.5]}}})));
        for (domain, bad) in [("mixmap", json!({"a": 1})), ("world", json!(5)), ("world", json!({})), ("world", json!({"a": null})),
            ("world", json!({"a b": 1})), ("world", json!({"a": 2e10})), ("world", json!({"a": "x".repeat(65)})),
            ("world", json!({"a": {"b": {"c": {"d": {"e": {"f": {"g": {"h": {"i": 1}}}}}}}}}))] {
            assert!(!valid_patch(domain, &bad), "accepted {domain} {bad}");
        }
        let many: serde_json::Map<String, Value> = (0..600).map(|i| (format!("k{i}"), json!(1))).collect();
        assert!(!valid_patch("world", &Value::Object(many)), "leaf limit");
        let mut section = json!({"traffic_engine": {"c04_taxi01": {"idle_rpm": 850.0, "patch": 2, "name": "taxi"}}, "levels": [1.0, 2.0, 3.0]});
        let mut written = Vec::new();
        apply(&mut section, &json!({"traffic_engine": {"c04_taxi01": {"idle_rpm": 1200}}, "levels": {"1": 5}}), "world_tuning", &mut |p| { written.push(p.to_owned()); true }).unwrap();
        assert_eq!(section["traffic_engine"]["c04_taxi01"]["idle_rpm"], 1200);
        assert_eq!(section["levels"], json!([1.0, 5, 3.0]));
        assert_eq!(written, ["world_tuning/levels/1", "world_tuning/traffic_engine/c04_taxi01/idle_rpm"]);
        for bad in [json!({"nope": 1}), json!({"levels": {"9": 1}}), json!({"levels": [1, 2]}), json!({"traffic_engine": {"c04_taxi01": {"idle_rpm": "fast"}}}),
            json!({"traffic_engine": {"c04_taxi01": {"name": 3}}})] {
            assert!(apply(&mut section.clone(), &bad, "world_tuning", &mut |_| true).is_err(), "{bad}");
        }
        // A refused claim leaves the field.
        apply(&mut section, &json!({"levels": {"0": 9}}), "world_tuning", &mut |_| false).unwrap();
        assert_eq!(section["levels"][0], 1.0);
        let (s, p) = section_patch("reverb", &json!({"BEEF": {"0": 1}})).unwrap();
        assert_eq!((s, p), ("bus_tuning", json!({"reverb": {"BEEF": {"0": 1}}})));
        let mut out = Vec::new();
        leaves(&json!({"a": {"b": 1, "c": [1, 2]}}), "x", &mut out);
        assert_eq!(out, ["x/a/b", "x/a/c"]);
    }
}
