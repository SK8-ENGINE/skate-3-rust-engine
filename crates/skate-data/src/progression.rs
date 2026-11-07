//! Loads `assets/private/progression.json` (setup `progression.py`, format `skate3rust.progression` v1) into the
//! engine-independent career types (`skate_core::career`). Doc 43.
use serde_json::Value;
use skate_core::career::graph::{Cond, Event, EventMode, Graph, Op, State, Target};
use skate_core::career::{Action, ChallengeRewards, Condition, Progression, Reward};
use skate_core::challenges::Completion;
use std::collections::BTreeMap;

pub const FORMAT: &str = "skate3rust.progression";

/// Everything the career needs from the export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub progression: Progression,
    pub graphs: BTreeMap<String, Graph>,
    /// reward id -> (unlock kind, unlock name)
    pub rewards: BTreeMap<String, (u32, String)>,
    /// reward id -> the reward summary's text (title id, image, description id).
    pub reward_text: BTreeMap<String, RewardText>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RewardText {
    pub title: String,
    pub image: String,
    pub description: String,
}

fn s(v: &Value, key: &str) -> Result<String, String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| format!("missing {key}"))
}

fn condition(c: &Value) -> Result<Condition, String> {
    let threshold = c.get("threshold").and_then(Value::as_u64).unwrap_or(0);
    Ok(match c.get("type").and_then(Value::as_str).unwrap_or("") {
        "board_sales" => Condition::BoardSales(threshold),
        "group_done" => Condition::GroupDone { group: s(c, "ref")?, at_least: threshold },
        "challenge_done" => Condition::ChallengeDone(s(c, "ref")?),
        "never" => Condition::Never,
        "money_earned" => Condition::MoneyEarned(threshold),
        "money" => Condition::Money(threshold),
        other => Condition::Ignored(other.strip_prefix("ignored_").and_then(|n| n.parse().ok()).unwrap_or(0)),
    })
}

fn reward(v: Option<&Value>) -> Reward {
    let Some(v) = v else { return Reward::default() };
    Reward {
        money: v.get("money").and_then(Value::as_u64).unwrap_or(0),
        sales: v.get("sales").and_then(Value::as_u64).unwrap_or(0),
        rewards: v.get("rewards").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
    }
}

fn target(v: &Value) -> Target {
    match v.as_str() {
        Some("$picked") => Target::Picked,
        Some(name) => Target::Named(name.to_owned()),
        None => Target::Named(String::new()),
    }
}

fn level(v: &str) -> Completion {
    match v {
        "owned" => Completion::Owned,
        "killed" => Completion::Killed,
        _ => Completion::Abandoned,
    }
}

fn cond(v: &Value) -> Result<Cond, String> {
    if let Some(flag) = v.get("flag_false").and_then(Value::as_str) {
        return Ok(Cond::FlagFalse(flag.to_owned()));
    }
    if v.get("reply_matches").is_some() {
        return Ok(Cond::ReplyMatches);
    }
    if v.get("picked_none").is_some() {
        return Ok(Cond::PickedNone);
    }
    if let Some(c) = v.get("completion_is") {
        return Ok(Cond::CompletionIs { challenge: s(c, "challenge")?, level: level(&s(c, "level")?) });
    }
    Err(format!("unknown condition {v}"))
}

fn ops(list: &Value) -> Result<Vec<Op>, String> {
    list.as_array().ok_or("ops not a list")?.iter().map(op).collect()
}

fn op(v: &Value) -> Result<Op, String> {
    let name = s(v, "op")?;
    let flag = || v.get("flag").and_then(Value::as_bool).unwrap_or(false);
    Ok(match name.as_str() {
        "change_state" => Op::ChangeState {
            state: s(v, "state")?,
            events: match v.get("events").and_then(Value::as_str) {
                Some("execute") => EventMode::Execute,
                Some("defer") => EventMode::Defer,
                _ => EventMode::None,
            },
        },
        "arm_action" => Op::ArmAction(s(v, "action")?),
        "unlock_reward" => Op::UnlockReward { reward: s(v, "reward")?, big: flag() },
        "activate_challenge" => Op::ActivateChallenge(s(v, "challenge")?),
        "deactivate_challenge" => Op::DeactivateChallenge(s(v, "challenge")?),
        "activate_group" => Op::ActivateGroup(s(v, "group")?),
        "brand_tier" => Op::BrandTier(s(v, "tier")?),
        "grant_achievement" => Op::GrantAchievement(s(v, "name")?),
        "skatefeed" => Op::SkateFeed(s(v, "text")?),
        "auto_sign_up" => Op::AutoSignUp(target(v.get("challenge").unwrap_or(&Value::Null))),
        "unlock_teammate" => Op::UnlockTeammate,
        "milestone_earned" => Op::MilestoneEarned,
        "allow_discovery" => Op::AllowDiscovery,
        "set_discovery" => Op::SetDiscovery { challenge: s(v, "challenge")?, status: s(v, "status")? },
        "hide_screen" => Op::HideScreen,
        "unhide_screen" => Op::UnhideScreen,
        "show_101" => Op::Show101 { title: s(v, "title")?, description: s(v, "description")? },
        "show_yes_no" => Op::ShowYesNo { title: s(v, "title")?, description: s(v, "description")? },
        "load_career_photo" => Op::LoadCareerPhoto { challenge: s(v, "challenge")? },
        "unload_career_photo" => Op::UnloadCareerPhoto(s(v, "challenge")?),
        "teleport" => Op::Teleport { locator: s(v, "locator")?, district: s(v, "district")? },
        "teleport_to_challenge" => Op::TeleportToChallenge(target(v.get("challenge").unwrap_or(&Value::Null))),
        "set_flag" => Op::SetFlag { name: s(v, "name")?, value: v.get("value").and_then(Value::as_bool).unwrap_or(false) },
        "pick" => Op::Pick(s(v, "challenge")?),
        "if" => Op::If {
            cond: cond(v.get("cond").ok_or("if without cond")?)?,
            then: ops(v.get("then").unwrap_or(&Value::Array(vec![])))?,
            otherwise: ops(v.get("else").unwrap_or(&Value::Array(vec![])))?,
        },
        other => return Err(format!("unknown op {other}")),
    })
}

/// A mod's progression overlay (`progression/*.json` in its folder, same schema as the export, rows by id) merged
/// over the data: objects merge key by key (so a partial action or graph row changes only what it names), any other
/// value replaces, `null` removes the key. Overlays apply in mod id order (b887 §8).
pub fn merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (key, value) in o {
                if value.is_null() {
                    b.remove(key);
                } else if let Some(slot) = b.get_mut(key) {
                    merge(slot, value);
                } else {
                    b.insert(key.clone(), value.clone());
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// A mod folder's overlays: `progression/*.json` in name order (none = empty).
pub fn read_overlays(mod_root: &std::path::Path) -> Result<Vec<Value>, String> {
    let Ok(dir) = std::fs::read_dir(mod_root.join("progression")) else { return Ok(Vec::new()) };
    let mut files: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    files.iter().map(|p| {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str::<Value>(&text).map_err(|e| format!("{}: {e}", p.display()))
    }).collect()
}

/// Parses the export; an unknown op or condition is an error (setup and the engine must agree).
pub fn parse(text: &str) -> Result<Loaded, String> {
    parse_value(&serde_json::from_str(text).map_err(|e| e.to_string())?)
}

/// The export (with any overlays merged) into the career types.
pub fn parse_value(root: &Value) -> Result<Loaded, String> {
    if root.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("not a progression export".into());
    }
    let mut out = Loaded::default();
    for (id, a) in root.get("actions").and_then(Value::as_object).into_iter().flatten() {
        let conditions = a.get("conditions").and_then(Value::as_array).map(|c| c.iter().map(condition).collect::<Result<Vec<_>, _>>())
            .transpose()?.unwrap_or_default();
        out.progression.actions.insert(id.clone(), Action {
            id: id.clone(),
            parent: a.get("parent").and_then(Value::as_str).map(str::to_owned),
            conditions,
            graph: a.get("graph").and_then(Value::as_str).map(str::to_owned),
            record_fired: a.get("record_fired").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    for (id, members) in root.get("groups").and_then(Value::as_object).into_iter().flatten() {
        let list = members.as_array().map(|m| m.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
        out.progression.groups.insert(id.clone(), list);
    }
    for (id, r) in root.get("challenge_rewards").and_then(Value::as_object).into_iter().flatten() {
        out.progression.challenge_rewards.insert(id.clone(), ChallengeRewards { owned: reward(r.get("owned")), killed: reward(r.get("killed")) });
    }
    out.progression.killable =
        root.get("killable").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    if let Some(c) = root.get("career") {
        out.progression.config.milestone_parent = c.get("milestone_parent").and_then(Value::as_str).unwrap_or_default().to_owned();
        out.progression.config.crossbar_gates =
            c.get("crossbar_gates").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    }
    for (id, r) in root.get("rewards").and_then(Value::as_object).into_iter().flatten() {
        // A reward row without an unlock name (milestone and photo cards) still shows in the reward summary.
        let kind = r.get("kind").and_then(Value::as_u64).unwrap_or(0) as u32;
        let name = r.get("name").and_then(Value::as_str).unwrap_or_default();
        out.rewards.insert(id.clone(), (kind, name.to_owned()));
        let text = |key: &str| r.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        out.reward_text.insert(id.clone(), RewardText { title: text("title"), image: text("image"), description: text("description") });
    }
    for (id, g) in root.get("graphs").and_then(Value::as_object).into_iter().flatten() {
        let mut graph = Graph::default();
        for (name, st) in g.get("states").and_then(Value::as_object).into_iter().flatten() {
            let events = st.get("events").and_then(Value::as_array).into_iter().flatten().map(|e| Event {
                kind: e.get("type").and_then(Value::as_str).unwrap_or_default().to_owned(),
                args: [0, 1].map(|i| e.get("args").and_then(|a| a.get(i)).and_then(Value::as_str).map(str::to_owned)),
                mask: e.get("mask").and_then(Value::as_u64).unwrap_or(0) as u32,
            }).collect();
            let mut state = State { events, on: BTreeMap::new() };
            for (message, list) in st.get("on").and_then(Value::as_object).into_iter().flatten() {
                state.on.insert(message.clone(), ops(list).map_err(|e| format!("{id} / {name} / {message}: {e}"))?);
            }
            graph.states.insert(name.clone(), state);
        }
        out.graphs.insert(id.clone(), graph);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_change_rows_by_id_and_null_removes() {
        let mut base = serde_json::json!({"format": FORMAT, "version": 1,
            "groups": {"ots_all": ["ots_a"]},
            "actions": {"m1": {"conditions": [{"type": "board_sales", "threshold": 15000}], "graph": "g", "record_fired": false},
                        "cut": {"conditions": [], "graph": null, "record_fired": false}}});
        merge(&mut base, &serde_json::json!({
            "groups": {"ots_all": ["ots_a", "mod_spot"]},
            "actions": {"m1": {"conditions": [{"type": "board_sales", "threshold": 5000}]}, "cut": null,
                        "mod_action": {"conditions": [{"type": "challenge_done", "ref": "mod_spot"}], "graph": null}}}));
        let l = parse_value(&base).unwrap();
        assert_eq!(l.progression.groups["ots_all"], ["ots_a", "mod_spot"]);
        assert_eq!(l.progression.actions["m1"].conditions, [Condition::BoardSales(5_000)]);
        assert_eq!(l.progression.actions["m1"].graph.as_deref(), Some("g"), "fields the overlay does not name stay");
        assert!(!l.progression.actions.contains_key("cut"));
        assert!(l.progression.actions.contains_key("mod_action"));
    }

    #[test]
    fn the_export_loads_into_the_career_types() {
        let text = r#"{"format":"skate3rust.progression","version":1,
            "groups":{"ots_all":["ots_a"]},
            "actions":{"milestone_01_complete":{"conditions":[{"type":"board_sales","threshold":15000}],"graph":"milestone_1","record_fired":false},
                       "starting_action":{"conditions":[],"graph":"starting_stategraph","record_fired":true}},
            "rewards":{"pcu":{"kind":0,"name":"pcu_training"}},
            "challenge_rewards":{"ots_a":{"owned":{"money":0,"sales":10000,"rewards":["pcu"]},"killed":{"money":0,"sales":2500,"rewards":[]}}},
            "graphs":{"milestone_1":{"initial":"initial","states":{
                "initial":{"events":[{"type":"NIS","args":["06_CoachIntro",null],"mask":0}],"on":{"stateenter":[
                    {"op":"unlock_reward","reward":"pcu","flag":true},
                    {"op":"if","cond":{"flag_false":"A"},"then":[{"op":"pick","challenge":"x"}],"else":[]},
                    {"op":"change_state","state":"complete","events":"defer"}]}},
                "complete":{"events":[],"on":{}}}}}}"#;
        let l = parse(text).unwrap();
        assert_eq!(l.progression.actions["milestone_01_complete"].conditions, [Condition::BoardSales(15_000)]);
        assert!(l.progression.actions["starting_action"].record_fired);
        assert_eq!(l.progression.challenge_rewards["ots_a"].killed.sales, 2_500);
        assert_eq!(l.rewards["pcu"], (0, "pcu_training".to_owned()));
        assert_eq!(l.reward_text["pcu"].description, "", "older exports have no description");
        let init = &l.graphs["milestone_1"].states["initial"];
        assert_eq!(init.events, [Event { kind: "NIS".into(), args: [Some("06_CoachIntro".into()), None], mask: 0 }]);
        assert_eq!(init.on["stateenter"][0], Op::UnlockReward { reward: "pcu".into(), big: true });
        assert_eq!(init.on["stateenter"][2], Op::ChangeState { state: "complete".into(), events: EventMode::Defer });
    }

    /// `SKATE_PROGRESSION_JSON=<export>`: the user's own export loads completely.
    #[test]
    #[ignore = "needs a progression.json from setup"]
    fn the_real_export_loads() {
        let path = std::env::var("SKATE_PROGRESSION_JSON").expect("SKATE_PROGRESSION_JSON");
        let l = parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!((l.progression.actions.len(), l.progression.groups.len(), l.graphs.len()), (204, 53, 181));
        assert!(l.graphs.values().all(|g| g.states.contains_key("complete")));
    }

    /// `SKATE_PROGRESSION_JSON=<export>`: a new career runs retail's starting graph: the first skate school tutorial
    /// and the first University challenge become active (research b861).
    #[test]
    #[ignore = "needs a progression.json from setup"]
    fn a_new_career_runs_the_starting_graph() {
        use skate_core::career::graph::Request;
        let path = std::env::var("SKATE_PROGRESSION_JSON").expect("SKATE_PROGRESSION_JSON");
        let l = parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut career = skate_core::career::Career::default();
        let requests = career.start(&l.progression, &l.graphs);
        assert!(requests.contains(&Request::GraphStarted("starting_stategraph".into())), "{requests:?}");
        assert!(requests.contains(&Request::ActivateChallenge("tut_sksc_01".into())), "{requests:?}");
        assert!(requests.contains(&Request::ActivateChallenge("ftl_univ_02".into())), "{requests:?}");
        assert_eq!(career.tracker.fired, ["starting_action"]);
        assert!(career.start(&l.progression, &l.graphs).is_empty(), "starting_action fires once per career");
    }

    #[test]
    fn unknown_ops_are_errors() {
        let text = r#"{"format":"skate3rust.progression","graphs":{"g":{"states":{"initial":{"on":{"stateenter":[{"op":"mystery"}]}}}}}}"#;
        assert!(parse(text).unwrap_err().contains("unknown op mystery"));
        assert!(parse(r#"{"format":"other"}"#).is_err());
    }
}
