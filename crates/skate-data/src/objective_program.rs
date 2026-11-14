//! Loads the objective programs setup writes (`tools/asset_pipeline/objective_lua.py`, `skate3rust.objective-program`
//! v1) into `skate_core::challenges::predicate::Program`. Names (API functions, `vars` fields, globals) are interned
//! into the indices the evaluator uses; the game answers API calls by name through `Names::api`. Doc 42 "Decision:
//! objectives as data, not Lua".
use serde_json::Value as Json;
use skate_core::challenges::predicate::{ArithOp, CmpOp, Expr, Program, Stmt, Value};
use std::sync::Arc;

/// Interned names, shared by every program of a run so indices are stable across them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    pub api: Vec<String>,
    pub vars: Vec<String>,
    pub globals: Vec<String>,
}

fn intern(list: &mut Vec<String>, name: &str) -> Result<u16, String> {
    if let Some(i) = list.iter().position(|n| n == name) {
        return Ok(i as u16);
    }
    let i = u16::try_from(list.len()).map_err(|_| "too many names".to_owned())?;
    list.push(name.to_owned());
    Ok(i)
}

fn u16_of(v: &Json) -> Result<u16, String> {
    v.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| format!("not an index: {v}"))
}

fn str_of(v: &Json) -> Result<&str, String> {
    v.as_str().ok_or_else(|| format!("not a name: {v}"))
}

fn pair(v: &Json) -> Result<(&Json, &Json), String> {
    match v.as_array().map(Vec::as_slice) {
        Some([a, b]) => Ok((a, b)),
        _ => Err(format!("expected two operands: {v}")),
    }
}

fn literal(v: &Json) -> Value {
    match v {
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Num(n.as_f64().unwrap_or(0.0) as f32),
        Json::String(s) => Value::Str(Arc::from(s.as_str())),
        _ => Value::Nil,
    }
}

fn exprs(list: Option<&Json>, names: &mut Names) -> Result<Vec<Expr>, String> {
    list.and_then(Json::as_array).into_iter().flatten().map(|e| expr(e, names)).collect()
}

fn expr(e: &Json, names: &mut Names) -> Result<Expr, String> {
    let o = e.as_object().ok_or_else(|| format!("not an expression: {e}"))?;
    let b = |v: &Json, names: &mut Names| expr(v, names).map(Box::new);
    Ok(if let Some(v) = o.get("lit") {
        Expr::Lit(literal(v))
    } else if let Some(v) = o.get("local") {
        Expr::Local(u16_of(v)?)
    } else if let Some(v) = o.get("var") {
        Expr::Var(intern(&mut names.vars, str_of(v)?)?)
    } else if let Some(v) = o.get("global") {
        Expr::Global(intern(&mut names.globals, str_of(v)?)?)
    } else if let Some(v) = o.get("not") {
        Expr::Not(b(v, names)?)
    } else if let Some(v) = o.get("neg") {
        Expr::Neg(b(v, names)?)
    } else if let Some(v) = o.get("and") {
        let (l, r) = pair(v)?;
        Expr::And(b(l, names)?, b(r, names)?)
    } else if let Some(v) = o.get("or") {
        let (l, r) = pair(v)?;
        Expr::Or(b(l, names)?, b(r, names)?)
    } else if let Some(v) = o.get("cmp") {
        let op = match str_of(v)? {
            "eq" => CmpOp::Eq,
            "ne" => CmpOp::Ne,
            "lt" => CmpOp::Lt,
            "le" => CmpOp::Le,
            "gt" => CmpOp::Gt,
            "ge" => CmpOp::Ge,
            other => return Err(format!("unknown comparison {other}")),
        };
        let (l, r) = pair(o.get("args").unwrap_or(&Json::Null))?;
        Expr::Cmp(op, b(l, names)?, b(r, names)?)
    } else if let Some(v) = o.get("arith") {
        let op = if str_of(v)? == "add" { ArithOp::Add } else { ArithOp::Sub };
        let (l, r) = pair(o.get("args").unwrap_or(&Json::Null))?;
        Expr::Arith(op, b(l, names)?, b(r, names)?)
    } else if let Some(v) = o.get("api") {
        Expr::Api { func: intern(&mut names.api, str_of(v)?)?, args: exprs(o.get("args"), names)? }
    } else if let Some(v) = o.get("index") {
        let (l, r) = pair(v)?;
        Expr::Index(b(l, names)?, b(r, names)?)
    } else if let Some(v) = o.get("table") {
        Expr::Table(exprs(Some(v), names)?)
    } else if o.contains_key("missing") || o.contains_key("enum") || o.contains_key("vars") {
        // A function the API lacks, an enum the translator did not know, or `vars` used as a value: an error when
        // reached, so the condition gives its default (as retail's protected call does on an error).
        Expr::Missing
    } else {
        return Err(format!("unknown expression {e}"));
    })
}

fn body(list: Option<&Json>, names: &mut Names) -> Result<Vec<Stmt>, String> {
    list.and_then(Json::as_array).into_iter().flatten().map(|s| stmt(s, names)).collect()
}

fn stmt(s: &Json, names: &mut Names) -> Result<Stmt, String> {
    let o = s.as_object().ok_or_else(|| format!("not a statement: {s}"))?;
    let value = |names: &mut Names| expr(o.get("value").unwrap_or(&Json::Null), names);
    Ok(if let Some(v) = o.get("set_local") {
        Stmt::SetLocal(u16_of(v)?, value(names)?)
    } else if let Some(v) = o.get("set_var") {
        Stmt::SetVar(intern(&mut names.vars, str_of(v)?)?, value(names)?)
    } else if let Some(v) = o.get("set_global") {
        Stmt::SetGlobal(intern(&mut names.globals, str_of(v)?)?, value(names)?)
    } else if let Some(v) = o.get("init_var") {
        Stmt::InitVar(intern(&mut names.vars, str_of(v)?)?, value(names)?)
    } else if let Some(v) = o.get("call") {
        Stmt::Call(expr(v, names)?)
    } else if let Some(v) = o.get("if") {
        let mut arms = Vec::new();
        for arm in v.as_array().into_iter().flatten() {
            let (test, then) = pair(arm)?;
            arms.push((expr(test, names)?, body(Some(then), names)?));
        }
        Stmt::If { arms, otherwise: body(o.get("else"), names)? }
    } else if let Some(v) = o.get("for") {
        let part = |k: &str, names: &mut Names| expr(o.get(k).unwrap_or(&Json::Null), names);
        Stmt::For { local: u16_of(v)?, start: part("start", names)?, limit: part("limit", names)?,
            step: part("step", names)?, body: body(o.get("body"), names)? }
    } else if let Some(v) = o.get("return") {
        Stmt::Return(expr(v, names)?)
    } else {
        return Err(format!("unknown statement {s}"));
    })
}

/// The challenge trick groups (stock collections class `challenge_trick_group`, field `ScoringTricks`; research
/// b863): group key -> member trick keys, a group's own list joined with every descendant's (82D58320).
pub fn trick_groups(c: &crate::collections::Collections) -> std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>> {
    use crate::attrib_hash::hash;
    use std::collections::{BTreeMap, BTreeSet};
    let key_of = |k: &str| k.strip_prefix("Hash_").and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or_else(|| hash(k));
    let class = hash("challenge_trick_group");
    let field = format!("Hash_{:016X}", hash("ScoringTricks"));
    let mut own: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut children: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for e in c.entries().iter().filter(|e| key_of(&e.class_name) == class) {
        let key = key_of(&e.key);
        if !e.parent.is_empty() {
            children.entry(key_of(&e.parent)).or_default().push(key);
        }
        let members = e.fields.iter().find(|(k, _)| key_of(k) == key_of(&field)).and_then(|(_, f)| f.array.as_ref())
            .map(|a| a.items.iter().filter_map(|i| u64::from_str_radix(i.get(16..32)?, 16).ok()).filter(|&k| k != 0).collect())
            .unwrap_or_default();
        own.insert(key, members);
    }
    fn collect(g: u64, own: &BTreeMap<u64, BTreeSet<u64>>, children: &BTreeMap<u64, Vec<u64>>, depth: u32, out: &mut BTreeSet<u64>) {
        if depth > 32 {
            return;
        }
        out.extend(own.get(&g).into_iter().flatten().copied());
        for &c in children.get(&g).into_iter().flatten() {
            collect(c, own, children, depth + 1, out);
        }
    }
    own.keys().map(|&g| {
        let mut members = BTreeSet::new();
        collect(g, &own, &children, 0, &mut members);
        (g, members)
    }).collect()
}

/// One exported program (`{locals, body}`).
pub fn program(json: &Json, names: &mut Names) -> Result<Program, String> {
    Ok(Program { locals: u16_of(json.get("locals").unwrap_or(&Json::Null))?, body: body(json.get("body"), names)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::challenges::predicate::{condition, Api, Error};

    struct Entered(Vec<String>);
    impl Api for Entered {
        fn call(&mut self, _func: u16, args: &[Value]) -> Result<Value, Error> {
            let Some(Value::Str(volume)) = args.get(1) else { return Err(Error::Type("args")) };
            Ok(Value::Bool(self.0.iter().any(|v| **v == **volume)))
        }
    }

    /// The translator's form of an own the spot completion (`return ObjectiveTrigger.EnteredVolume("1", "D|spot")`).
    #[test]
    fn a_translated_condition_loads_and_runs_against_the_api() {
        let json = serde_json::json!({"locals": 0, "body": [{"return": {"and": [
            {"api": "ObjectiveTrigger.EnteredVolume", "args": [{"lit": "1"}, {"lit": "D|spot"}]},
            {"cmp": "ge", "args": [{"var": "count"}, {"lit": 0}]}]}}]});
        let mut names = Names::default();
        let p = program(&json, &mut names).unwrap();
        assert_eq!(names.api, ["ObjectiveTrigger.EnteredVolume"]);
        let mut vars = vec![Value::Num(1.0); names.vars.len()];
        assert!(!condition(&p, &mut Entered(vec![]), &mut vars, &mut [], true));
        assert!(condition(&p, &mut Entered(vec!["D|spot".into()]), &mut vars, &mut [], true));
    }

    /// Installed data (b863): `fliptricks` is an inner node whose members come from its children (kickflips etc.).
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_trick_groups_join_their_children() {
        use crate::attrib_hash::hash;
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let groups = trick_groups(&crate::collections::Collections::load(&root).unwrap());
        assert_eq!(groups.len(), 87);
        assert!(groups[&hash("fliptricks")].contains(&hash("kickflip")));
        assert!(groups[&hash("smithgrind")].is_empty(), "b863: matches nothing");
    }

    #[test]
    fn unknown_names_become_errors_that_give_the_default() {
        let json = serde_json::json!({"locals": 0, "body": [{"call": {"missing": "syntax error: x"}}]});
        let p = program(&json, &mut Names::default()).unwrap();
        assert!(condition(&p, &mut Entered(vec![]), &mut [], &mut [], true), "completion default");
        assert!(!condition(&p, &mut Entered(vec![]), &mut [], &mut [], false));
    }
}
