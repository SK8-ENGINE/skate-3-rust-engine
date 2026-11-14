//! JSON that keeps its key order, for the files the launcher rewrites (config.json): serde_json's own map sorts keys
//! here, and its order-keeping feature would change every crate in the workspace that is built with it.
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

#[derive(Clone, Debug, PartialEq)]
pub enum OValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<OValue>),
    Object(Vec<(String, OValue)>),
}

impl OValue {
    /// Sets a dotted path (`recomp.exe`), adding objects on the way; a non-object on the way is replaced.
    pub fn set_path(&mut self, path: &str, value: OValue) {
        let mut cur = self;
        let parts: Vec<&str> = path.split('.').collect();
        for (i, part) in parts.iter().enumerate() {
            if !matches!(cur, OValue::Object(_)) {
                *cur = OValue::Object(Vec::new());
            }
            let OValue::Object(fields) = cur else { unreachable!() };
            let at = match fields.iter().position(|(k, _)| k == part) {
                Some(at) => at,
                None => {
                    fields.push(((*part).to_owned(), OValue::Null));
                    fields.len() - 1
                }
            };
            if i == parts.len() - 1 {
                fields[at].1 = value;
                return;
            }
            if matches!(fields[at].1, OValue::Null) {
                fields[at].1 = OValue::Object(Vec::new());
            }
            cur = &mut fields[at].1;
        }
    }
}

impl Serialize for OValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            OValue::Null => s.serialize_unit(),
            OValue::Bool(b) => s.serialize_bool(*b),
            OValue::Number(n) => n.serialize(s),
            OValue::String(t) => s.serialize_str(t),
            OValue::Array(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            OValue::Object(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (k, v) in fields {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for OValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = OValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<OValue, E> { Ok(OValue::Null) }
            fn visit_none<E>(self) -> Result<OValue, E> { Ok(OValue::Null) }
            fn visit_bool<E>(self, b: bool) -> Result<OValue, E> { Ok(OValue::Bool(b)) }
            fn visit_i64<E>(self, n: i64) -> Result<OValue, E> { Ok(OValue::Number(n.into())) }
            fn visit_u64<E>(self, n: u64) -> Result<OValue, E> { Ok(OValue::Number(n.into())) }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<OValue, E> {
                serde_json::Number::from_f64(n).map(OValue::Number).ok_or_else(|| E::custom("not a JSON number"))
            }
            fn visit_str<E>(self, s: &str) -> Result<OValue, E> { Ok(OValue::String(s.to_owned())) }
            fn visit_string<E>(self, s: String) -> Result<OValue, E> { Ok(OValue::String(s)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<OValue, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(OValue::Array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OValue, A::Error> {
                let mut fields: Vec<(String, OValue)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, OValue>()? {
                    match fields.iter_mut().find(|(x, _)| *x == k) {
                        Some(slot) => slot.1 = v,
                        None => fields.push((k, v)),
                    }
                }
                Ok(OValue::Object(fields))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keep_their_order_and_dotted_paths_merge() {
        let mut v: OValue = serde_json::from_str(r#"{"zeta": 1, "alpha": {"b": 2, "a": 1}}"#).unwrap();
        v.set_path("alpha.c", OValue::Bool(true));
        v.set_path("new.deep", OValue::String("x".into()));
        v.set_path("zeta", OValue::Null);
        assert_eq!(serde_json::to_string(&v).unwrap(), r#"{"zeta":null,"alpha":{"b":2,"a":1,"c":true},"new":{"deep":"x"}}"#);
    }
}
