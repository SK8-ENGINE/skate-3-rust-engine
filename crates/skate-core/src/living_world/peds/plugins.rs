//! Ped plugins on world props: benches, trash bins, newspaper boxes, vending machines, ATMs, water fountains,
//! look-at and spectate spots, gathering places (retail TU3, evidence only; re-implemented;
//! `.local/research/peds/b81-ped-plugin-acquisition.md`, `b82-ped-plugin-search.md`; main checked the ped roll
//! `82E3B060` and the d100 roll `8269A588`):
//! - a plugin prop owns waypoint groups, one waypoint each for DMO hotpoints (`82C4E128`: hotpoint type 1 = trash bin,
//!   2 = newspaper box, 0 = the root (no waypoint), 8 skipped, any other type = a seat) or the placed groups of the
//!   world (`waypoints.json`: vending machines, trash bins);
//! - a ped type's `plugin_odds` (attribute `F89323E420A6AAA3`, a list of {prop class, probability}) drives two things:
//!   the ped's own yes / no when a prop is offered (`82E3B060`: odds x 100 >= `rand() % 100 + 1`; a refusal remembers
//!   the prop, `82E40BD0`, at most 16 entries with the value 30.0) and the spawner's weighted pick of a ped type to
//!   spawn at a free prop (`826B90F8`: weight = group weight x odds, draw = rand x 2^-32 x total; on success the prop's
//!   cooldown `+232` restarts from `+228`);
//! - a descriptor takes one participant unless it says otherwise (`82C17AF8`: `maxNumberOfParticipants`, default 1).
//!
//! Multiplayer: [`PluginProp`] and [`RefusalMemory`] are plain host-owned data; every roll takes the host's seeded RNG.

use crate::living_world::{Rng, Vec3};

/// Retail values; every field is data a mod can override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluginSettings {
    /// Refusal memory (`82E40BD0` / `82E40AF0`, b83): entries kept (16), the seconds stored with each (30.0) and the
    /// floor the countdown stops at (`0x822F88B0` -30.0).
    pub refusal_capacity: usize,
    pub refusal_value: f32,
    pub refusal_floor: f32,
    /// The offer scan (`826C0058`, b83): every `scan_period` manager ticks (11) at 1/60 s; a ped qualifies within
    /// `height_tolerance` (`0x8208EB60` 1.8 m) vertically and the offer radius horizontally.
    pub scan_period: u32,
    pub height_tolerance: f32,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self { refusal_capacity: 16, refusal_value: 30.0, refusal_floor: -30.0, scan_period: 11, height_tolerance: 1.8 }
    }
}

/// The plugin class a DMO hotpoint type makes (`82C4E128`); `None` for the root (0) and the skipped type (8).
pub fn hotpoint_class(hotpoint_type: u32) -> Option<&'static str> {
    match hotpoint_type {
        0 | 8 => None,
        1 => Some("waypoint_usetrashbin"),
        2 => Some("waypoint_newspaperbox"),
        _ => Some("waypoint_sit"),
    }
}

/// A plugin descriptor (`livingworld\PluginDescriptor\<name>.xml`, `82C17AF8`): its plugin graph, how many peds may
/// use it (`maxNumberOfParticipants`, default 1) and the transfer expression a ped must pass to take it.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    pub name: String,
    pub graph: String,
    pub max_participants: u32,
    pub transfer: Option<Transfer>,
}

/// A `TransferExpression` tree (`op` = and / or / not; [inference] no `op` = and) over named `TransferCondition`s
/// with their XML parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum Transfer {
    And(Vec<Transfer>),
    Or(Vec<Transfer>),
    Not(Vec<Transfer>),
    Condition { name: String, params: Vec<(String, String)> },
}

impl Transfer {
    /// Evaluate with `condition(name, params)`; an empty and / or is true / false, a `not` negates the and of its
    /// children.
    pub fn evaluate(&self, condition: &mut dyn FnMut(&str, &[(String, String)]) -> bool) -> bool {
        match self {
            Transfer::And(c) => c.iter().all(|t| t.evaluate(condition)),
            Transfer::Or(c) => c.iter().any(|t| t.evaluate(condition)),
            Transfer::Not(c) => !c.iter().all(|t| t.evaluate(condition)),
            Transfer::Condition { name, params } => condition(name, params),
        }
    }

    /// Every condition name in the tree (to report which are not ported).
    pub fn names(&self, out: &mut Vec<String>) {
        match self {
            Transfer::And(c) | Transfer::Or(c) | Transfer::Not(c) => c.iter().for_each(|t| t.names(out)),
            Transfer::Condition { name, .. } => out.push(name.clone()),
        }
    }
}

/// What a world prop descriptor's transfer conditions read of a ped (`UseWorldProp` descriptors: sit, usetrashbin,
/// newspaperbox, vend, ATM, water fountain).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TransferView {
    pub hand_prop: bool,
    pub disposable: bool,
    pub can_sit: bool,
    pub chasing: bool,
}

impl TransferView {
    /// A ped's view from its brain.
    pub fn of(brain: &super::brain::PedBrain) -> Self {
        let h = &brain.hand_prop;
        Self { hand_prop: h.has(), disposable: h.disposable, can_sit: h.can_sit, chasing: brain.chasee.is_some() }
    }

    /// One transfer condition for `ped` at `prop`: `HasHandProp` (`826AD1A8`), `HasDisposableHandProp` (`8269D170`),
    /// `CanSitWithHandProp` (`8269D268`), `IsChasing`; `IsOnRoad` is false (its condition slot is the `li r3,0` stub
    /// `8274CA90`). `HasFirstWaypointAvailable` (the prop's waypoint 0 is free) and `InFrontOfFirstWaypoint` (the ped
    /// is on the facing side of waypoint 0) are [inferred] from their names, their code is not read. Other names
    /// (`IsNearCrossWalk`, not ported) answer false like the brain's pending conditions.
    pub fn condition(&self, name: &str, prop: &PluginProp, ped: Vec3) -> bool {
        let first = prop.waypoints.first();
        match name {
            "HasFirstWaypointAvailable" => first.is_some_and(|w| w.occupant.is_none()),
            "InFrontOfFirstWaypoint" => first.is_some_and(|w| (ped[0] - w.position[0]) * w.facing[0] + (ped[2] - w.position[2]) * w.facing[2] >= 0.0),
            "HasHandProp" => self.hand_prop,
            "HasDisposableHandProp" => self.hand_prop && self.disposable,
            "CanSitWithHandProp" => !self.hand_prop || self.can_sit,
            "IsChasing" => self.chasing,
            _ => false,
        }
    }
}

/// One waypoint of a plugin prop: where the ped stands or sits, which way it faces, who holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluginWaypoint {
    pub position: Vec3,
    pub facing: Vec3,
    pub occupant: Option<u64>,
}

/// A world prop offering a plugin (its class names the descriptor, e.g. `waypoint_sit`).
#[derive(Clone, Debug, PartialEq)]
pub struct PluginProp {
    pub id: u64,
    pub class: String,
    pub waypoints: Vec<PluginWaypoint>,
    /// `+232` (counts down) and the value it restarts from (`+228`).
    pub cooldown: f32,
    pub cooldown_reset: f32,
}

impl PluginProp {
    /// A free waypoint (no occupant), nearest to `to`.
    pub fn free_waypoint(&self, to: Vec3) -> Option<usize> {
        let d2 = |p: Vec3| (p[0] - to[0]).powi(2) + (p[1] - to[1]).powi(2) + (p[2] - to[2]).powi(2);
        self.waypoints.iter().enumerate().filter(|(_, w)| w.occupant.is_none()).min_by(|a, b| d2(a.1.position).total_cmp(&d2(b.1.position))).map(|(i, _)| i)
    }

    /// Lock waypoint `i` for `ped`; false when it is held.
    pub fn lock(&mut self, i: usize, ped: u64) -> bool {
        match self.waypoints.get_mut(i) {
            Some(w) if w.occupant.is_none() => {
                w.occupant = Some(ped);
                true
            }
            _ => false,
        }
    }

    /// Release every waypoint `ped` holds (the participant's release, `82E1CD58`).
    pub fn release(&mut self, ped: u64) {
        for w in &mut self.waypoints {
            if w.occupant == Some(ped) {
                w.occupant = None;
            }
        }
    }

    /// The cooldown counts down; a prop is offered by the spawner only at 0 or below.
    pub fn tick(&mut self, dt: f32) {
        if self.cooldown > 0.0 {
            self.cooldown -= dt;
        }
    }

    /// A ped was spawned onto it: the cooldown restarts (`826B90F8`: `+232 = +228`).
    pub fn used(&mut self) {
        self.cooldown = self.cooldown_reset;
    }
}

/// A ped type's `plugin_odds`: the probability for a prop class (0 when the class is not listed).
pub fn odds_for(odds: &[(String, f32)], class: &str) -> f32 {
    odds.iter().find(|(c, _)| c == class).map_or(0.0, |(_, p)| *p)
}

/// `8269A588`: `p x 100 >= rand() % 100 + 1`.
pub fn roll_percent(p: f32, rng: &mut Rng) -> bool {
    let k = rng.modulo(100) + 1;
    p * 100.0 >= k as f32
}

/// The props a ped refused (`82E40BD0`): at most `refusal_capacity` entries with `refusal_value`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefusalMemory {
    pub entries: Vec<(u64, f32)>,
}

impl RefusalMemory {
    /// Slot 5 (`82E3AF90`): the prop is blocked while its value is above 0.
    pub fn contains(&self, prop: u64) -> bool {
        self.entries.iter().any(|e| e.0 == prop && e.1 > 0.0)
    }

    /// `82E40BD0`: a full map evicts one entry ([inference] the oldest) first; the prop's value becomes 30.
    pub fn remember(&mut self, prop: u64, s: &PluginSettings) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == prop) {
            e.1 = s.refusal_value;
            return;
        }
        if self.entries.len() >= s.refusal_capacity && !self.entries.is_empty() {
            self.entries.remove(0);
        }
        self.entries.push((prop, s.refusal_value));
    }

    /// `82E40AF0` (each ped update): every value at or above the floor loses `dt`; nothing is removed.
    pub fn tick(&mut self, dt: f32, s: &PluginSettings) {
        for e in &mut self.entries {
            if e.1 >= s.refusal_floor {
                e.1 -= dt;
            }
        }
    }
}

/// `82E3B060`: does this ped take the offered prop? A prop it refused before is skipped; a refusal is remembered.
pub fn ped_wants(odds: &[(String, f32)], prop: &PluginProp, memory: &mut RefusalMemory, s: &PluginSettings, rng: &mut Rng) -> bool {
    if memory.contains(prop.id) {
        return false;
    }
    let p = odds_for(odds, &prop.class);
    if roll_percent(p, rng) {
        return true;
    }
    memory.remember(prop.id, s);
    false
}

/// A ped as the offer scan sees it.
pub struct OfferPed<'a> {
    pub id: u64,
    pub position: Vec3,
    /// Participant slot 4: the ped's current plugin.
    pub has_plugin: bool,
    pub odds: &'a [(String, f32)],
    pub memory: &'a mut RefusalMemory,
}

/// A ped took a prop's waypoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attach {
    pub ped: u64,
    pub prop: u64,
    pub waypoint: usize,
}

/// One prop's offer in the scan (`826C0058` -> `826BFE18` for every ped, registry order, no early exit): a ped
/// without a plugin, not refusing the prop, within `height_tolerance` vertically and `radius` horizontally, accepted
/// by `qualifies` (the offer's own test, vfunc 10) and passing its `plugin_odds` roll takes the prop's nearest free
/// waypoint (vfunc 14); taking it also remembers the prop for 30 s (`8269F248`). NOT RETAIL YET: the offer's
/// vfuncs 18 / 9 / 10 are not decoded (ours: a free waypoint and below the descriptor's participant cap, then
/// `qualifies`); the radius source is open (ours: the class field `E2101F2B17A0E6B5`, [inference]).
#[allow(clippy::too_many_arguments)]
pub fn offer(prop: &mut PluginProp, radius: f32, max_participants: u32, peds: &mut [OfferPed], qualifies: &mut dyn FnMut(&PluginProp, &OfferPed) -> bool, s: &PluginSettings, rng: &mut Rng) -> Vec<Attach> {
    let mut out = Vec::new();
    for ped in peds.iter_mut() {
        let users = prop.waypoints.iter().filter(|w| w.occupant.is_some()).count() as u32;
        if users >= max_participants.max(1) {
            break;
        }
        if ped.has_plugin || ped.memory.contains(prop.id) {
            continue;
        }
        let Some(nearest) = prop.free_waypoint(ped.position) else { break };
        let w = prop.waypoints[nearest].position;
        let (dx, dy, dz) = (w[0] - ped.position[0], w[1] - ped.position[1], w[2] - ped.position[2]);
        if dy.abs() >= s.height_tolerance || dx * dx + dz * dz >= radius * radius {
            continue;
        }
        if !qualifies(prop, ped) {
            continue;
        }
        if !ped_wants(ped.odds, prop, ped.memory, s, rng) {
            continue;
        }
        if prop.lock(nearest, ped.id) {
            ped.has_plugin = true;
            ped.memory.remember(prop.id, s);
            out.push(Attach { ped: ped.id, prop: prop.id, waypoint: nearest });
        }
    }
    out
}

/// `826B90F8`'s pick: candidates (ped type index, group weight, the type's odds for the prop class); weight =
/// group weight x odds, draw = rand x 2^-32 x total. `None` when every weight is 0.
pub fn pick_ped_type(candidates: &[(usize, f32, f32)], rng: &mut Rng) -> Option<usize> {
    let total: f32 = candidates.iter().map(|c| (c.1 * c.2).max(0.0)).sum();
    if !(total > 0.0) {
        return None;
    }
    let mut draw = rng.unit() * total;
    for c in candidates {
        let w = (c.1 * c.2).max(0.0);
        if draw < w {
            return Some(c.0);
        }
        draw -= w;
    }
    candidates.iter().rev().find(|c| c.1 * c.2 > 0.0).map(|c| c.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// usetrashbin's transfer (`HasDisposableHandProp and not IsOnRoad and not IsNearCrossWalk and not IsChasing`):
    /// only a ped holding a disposable prop takes a bin; sit's `CanSitWithHandProp` passes without a prop.
    #[test]
    fn world_prop_transfer_reads_the_hand_prop() {
        let c = |n: &str| Transfer::Condition { name: n.into(), params: vec![] };
        let bin = Transfer::And(vec![c("HasDisposableHandProp"), Transfer::Not(vec![c("IsOnRoad")]), Transfer::Not(vec![c("IsNearCrossWalk")]), Transfer::Not(vec![c("IsChasing")])]);
        let prop = PluginProp { id: 1, class: "waypoint_usetrashbin".into(), waypoints: vec![PluginWaypoint { position: [0.0; 3], facing: [0.0, 0.0, 1.0], occupant: None }], cooldown: 0.0, cooldown_reset: 0.0 };
        let pass = |v: TransferView, t: &Transfer| t.evaluate(&mut |n, _| v.condition(n, &prop, [0.0, 0.0, 1.0]));
        let pop = TransferView { hand_prop: true, disposable: true, can_sit: true, chasing: false };
        assert!(pass(pop, &bin));
        assert!(!pass(TransferView::default(), &bin), "no prop");
        assert!(!pass(TransferView { disposable: false, ..pop }, &bin), "a newspaper is not disposable");
        assert!(!pass(TransferView { chasing: true, ..pop }, &bin));
        let sit = c("CanSitWithHandProp");
        assert!(pass(TransferView::default(), &sit));
        assert!(!pass(TransferView { hand_prop: true, ..Default::default() }, &sit));
        // The vending machine: waypoint 0 free and the ped in front of it, no hand prop.
        let vend = Transfer::And(vec![c("HasFirstWaypointAvailable"), c("InFrontOfFirstWaypoint"), Transfer::Not(vec![c("HasHandProp")])]);
        assert!(pass(TransferView::default(), &vend));
        assert!(!TransferView::default().condition("InFrontOfFirstWaypoint", &prop, [0.0, 0.0, -1.0]), "behind it");
        let mut held = prop.clone();
        held.waypoints[0].occupant = Some(9);
        assert!(!TransferView::default().condition("HasFirstWaypointAvailable", &held, [0.0; 3]));
    }

    fn bench() -> PluginProp {
        let w = |x: f32| PluginWaypoint { position: [x, 0.0, 0.0], facing: [0.0, 0.0, 1.0], occupant: None };
        PluginProp { id: 7, class: "waypoint_sit".into(), waypoints: vec![w(0.0), w(1.0)], cooldown: 0.0, cooldown_reset: 10.0 }
    }

    #[test]
    fn hotpoints_map_to_plugin_classes() {
        assert_eq!(hotpoint_class(0), None);
        assert_eq!(hotpoint_class(8), None);
        assert_eq!(hotpoint_class(1), Some("waypoint_usetrashbin"));
        assert_eq!(hotpoint_class(2), Some("waypoint_newspaperbox"));
        assert_eq!(hotpoint_class(5), Some("waypoint_sit"));
    }

    #[test]
    fn waypoints_lock_once_and_release_with_the_ped() {
        let mut b = bench();
        assert_eq!(b.free_waypoint([0.9, 0.0, 0.0]), Some(1));
        assert!(b.lock(1, 100));
        assert!(!b.lock(1, 101));
        assert_eq!(b.free_waypoint([0.9, 0.0, 0.0]), Some(0));
        b.release(100);
        assert_eq!(b.free_waypoint([0.9, 0.0, 0.0]), Some(1));
        b.used();
        b.tick(4.0);
        assert!((b.cooldown - 6.0).abs() < 1e-6);
    }

    #[test]
    fn the_ped_roll_follows_its_odds_and_remembers_refusals() {
        let s = PluginSettings::default();
        let b = bench();
        let mut rng = Rng::new(9);
        let never = vec![("waypoint_sit".to_string(), 0.0)];
        let mut m = RefusalMemory::default();
        assert!(!ped_wants(&never, &b, &mut m, &s, &mut rng));
        assert!(m.contains(7));
        // Remembered: skipped without a roll even with odds 1.
        let always = vec![("waypoint_sit".to_string(), 1.0)];
        assert!(!ped_wants(&always, &b, &mut m, &s, &mut rng));
        m.tick(30.0, &s);
        assert!(!m.contains(7));
        assert!(ped_wants(&always, &b, &mut m, &s, &mut rng));
        // Unlisted class: odds 0.
        assert_eq!(odds_for(&always, "waypoint_useatm"), 0.0);
        let mut m = RefusalMemory::default();
        for k in 0..20 {
            m.remember(k, &s);
        }
        assert_eq!(m.entries.len(), 16);
        assert_eq!(m.entries[0].0, 4, "the oldest left first");
        // The countdown stops at the floor.
        for _ in 0..100 {
            m.tick(1.0, &s);
        }
        assert!(m.entries.iter().all(|e| e.1 < s.refusal_floor && e.1 >= s.refusal_floor - 1.0));
    }

    #[test]
    fn the_spawn_pick_weights_group_weight_by_odds() {
        let mut rng = Rng::new(1);
        assert_eq!(pick_ped_type(&[(0, 1.0, 0.0), (1, 2.0, 0.0)], &mut rng), None);
        assert_eq!(pick_ped_type(&[(0, 1.0, 0.0), (1, 2.0, 0.5)], &mut rng), Some(1));
        let mut counts = [0; 2];
        for _ in 0..4000 {
            counts[pick_ped_type(&[(0, 1.0, 1.0), (1, 3.0, 1.0)], &mut rng).unwrap()] += 1;
        }
        assert!((800..1200).contains(&counts[0]), "{counts:?}");
    }

    #[test]
    fn a_transfer_expression_combines_its_conditions() {
        let c = |n: &str| Transfer::Condition { name: n.into(), params: Vec::new() };
        let t = Transfer::And(vec![c("A"), Transfer::Not(vec![c("B")]), Transfer::Or(vec![c("C"), c("D")])]);
        let truth = |a, b, c2, d| move |n: &str, _: &[(String, String)]| match n {
            "A" => a,
            "B" => b,
            "C" => c2,
            _ => d,
        };
        assert!(t.evaluate(&mut truth(true, false, false, true)));
        assert!(!t.evaluate(&mut truth(true, true, true, true)));
        assert!(!t.evaluate(&mut truth(true, false, false, false)));
        let mut names = Vec::new();
        t.names(&mut names);
        assert_eq!(names, ["A", "B", "C", "D"]);
    }

    #[test]
    fn the_offer_scan_attaches_a_qualifying_ped_and_skips_the_rest() {
        let s = PluginSettings::default();
        let mut rng = Rng::new(2);
        let mut b = bench();
        let yes = vec![("waypoint_sit".to_string(), 1.0)];
        let (mut m1, mut m2, mut m3, mut m4) = (RefusalMemory::default(), RefusalMemory::default(), RefusalMemory::default(), RefusalMemory::default());
        let mut peds = vec![
            OfferPed { id: 1, position: [0.0, 2.0, 0.0], has_plugin: false, odds: &yes, memory: &mut m1 },
            OfferPed { id: 2, position: [20.0, 0.0, 0.0], has_plugin: false, odds: &yes, memory: &mut m2 },
            OfferPed { id: 3, position: [0.5, 0.0, 1.0], has_plugin: true, odds: &yes, memory: &mut m3 },
            OfferPed { id: 4, position: [1.2, 0.0, 1.0], has_plugin: false, odds: &yes, memory: &mut m4 },
        ];
        let got = offer(&mut b, 10.0, 2, &mut peds, &mut |_, _| true, &s, &mut rng);
        // 1: 2 m above (over 1.8); 2: 20 m away; 3: already in a plugin; 4 takes the nearest seat (x = 1).
        assert_eq!(got, vec![Attach { ped: 4, prop: 7, waypoint: 1 }]);
        assert_eq!(b.waypoints[1].occupant, Some(4));
        assert!(peds[3].has_plugin && peds[3].memory.contains(7));
    }
}
