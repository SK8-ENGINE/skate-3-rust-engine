//! One PathFinder music stream (research b1016 §2): the node playing, the 0..127 branch control value, the
//! variables and the exit nodes' loop counters. The host asks for the next segment when one ends; events run their
//! actions (if / elif / else, goto, set, post, callback, arithmetic). Random draws come from a seeded generator, so a
//! run (and a replay) repeats. NOT RETAIL YET: waits, sync waits and the parameter ramps (actions 2, 3, 5..7, 15..18)
//! do not hold the action list; they are counted in `skipped`.
use std::collections::HashMap;

use super::format::{Action, Compare, Graph, Kind, Operand};

#[derive(Clone, Debug)]
pub struct Stream {
    /// The audio node playing.
    pub node: Option<u16>,
    /// A goto that takes effect at the next segment end.
    goto: Option<u16>,
    /// A goto with the cut flag: the host stops the playing segment and asks again.
    pub cut: bool,
    /// Property 1, the branch control value (0..127).
    pub control: i32,
    pub vars: Vec<i32>,
    loops: HashMap<u16, i8>,
    rng: u32,
    /// Callbacks to the game (action 13) since the host last took them.
    pub callbacks: Vec<(i16, i16)>,
    /// Entry nodes passed (section, node) since the host last took them (retail notifies the game).
    pub entered: Vec<(u8, u16)>,
    /// Actions not run (waits, ramps, unknown types), for logs.
    pub skipped: u32,
}

impl Stream {
    pub fn new(graph: &Graph, seed: u32) -> Self {
        Self {
            node: None,
            goto: None,
            cut: false,
            control: 0,
            vars: graph.vars.iter().map(|v| v.1).collect(),
            loops: HashMap::new(),
            rng: if seed == 0 { 0x9E37_79B9 } else { seed },
            callbacks: Vec::new(),
            entered: Vec::new(),
            skipped: 0,
        }
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    /// Sets a variable by name (case-insensitive, as retail lower-cases it); false when the graph has none.
    pub fn set_var(&mut self, graph: &Graph, name: &str, value: i32) -> bool {
        match graph.var(name) {
            Some(i) => {
                self.vars[i] = value;
                true
            }
            None => false,
        }
    }

    fn value(&mut self, graph: &Graph, op: Operand) -> i32 {
        match op {
            Operand::Const(c) => c,
            Operand::Var(i) => self.vars.get(i as usize).copied().unwrap_or(0),
            Operand::Prop(1) => self.control,
            Operand::Prop(2) => self.node.map_or(-1, i32::from),
            Operand::Prop(3) => self.node.and_then(|n| graph.nodes.get(n as usize)).map_or(-1, |n| i32::from(n.owner)),
            Operand::Prop(4) => self.node.and_then(|n| graph.nodes.get(n as usize)).map_or(-1, |n| i32::from(n.section)),
            // Property 18 is retail's rand() / 23 (rand 0..32767).
            Operand::Prop(18) => ((self.random() & 0x7FFF) / 23) as i32,
            Operand::Prop(_) => 0,
        }
    }

    /// Runs event `id` (its low 24 bits); false when the graph has no such event.
    pub fn post(&mut self, graph: &Graph, id: u32) -> bool {
        self.run(graph, id & 0x00FF_FFFF, 0)
    }

    fn run(&mut self, graph: &Graph, id: u32, depth: u32) -> bool {
        let Some(event) = graph.events.iter().rev().find(|e| e.id == id) else { return false };
        if depth > 8 {
            return true;
        }
        // (parent active, a branch of this if was taken)
        let mut stack: Vec<(bool, bool)> = Vec::new();
        let mut active = true;
        for action in event.actions.clone() {
            match action {
                Action::If { elif: false, compare, a, b } => {
                    let ok = active && self.test(graph, compare, a, b);
                    stack.push((active, ok));
                    active = ok;
                }
                Action::If { elif: true, compare, a, b } => {
                    if let Some((parent, taken)) = stack.last_mut() {
                        let ok = *parent && !*taken && self.test(graph, compare, a, b);
                        *taken |= ok;
                        active = ok;
                    }
                }
                Action::Else => {
                    if let Some((parent, taken)) = stack.last_mut() {
                        active = *parent && !*taken;
                        *taken = true;
                    }
                }
                Action::EndIf => {
                    if let Some((parent, _)) = stack.pop() {
                        active = parent;
                    }
                }
                _ if !active => {}
                Action::Goto { node, section, cut } => {
                    let n = self.value(graph, node);
                    let target = match section {
                        Some(s) => {
                            let s = self.value(graph, s);
                            u8::try_from(s).ok().and_then(|s| graph.entries(s).get((n - 1).max(0) as usize).copied())
                                .filter(|_| n >= 1)
                        }
                        None => u16::try_from(n).ok().filter(|&t| (t as usize) < graph.nodes.len()),
                    };
                    if let Some(t) = target {
                        self.goto = Some(t);
                        self.cut |= cut;
                    }
                }
                Action::Set { target, value } => {
                    let v = self.value(graph, value);
                    match target {
                        Operand::Var(i) => {
                            if let Some(slot) = self.vars.get_mut(i as usize) {
                                *slot = v;
                            }
                        }
                        Operand::Prop(1) => self.control = v.clamp(0, 127),
                        _ => self.skipped += 1,
                    }
                }
                Action::Post(e) => {
                    self.run(graph, e & 0x00FF_FFFF, depth + 1);
                }
                Action::Callback { a, b } => self.callbacks.push((a, b)),
                Action::Arith { target, op, value } => {
                    let v = self.value(graph, value);
                    if let Some(t) = self.vars.get_mut(target as usize) {
                        *t = match op {
                            1 => t.wrapping_add(v),
                            2 => v.wrapping_sub(*t),
                            3 => t.wrapping_mul(v),
                            4 if v != 0 => *t / v,
                            5 if v != 0 => t.rem_euclid(v),
                            _ => *t,
                        };
                    }
                }
                Action::Wait(_) | Action::WaitSync(_) | Action::Unknown { .. } => self.skipped += 1,
            }
        }
        true
    }

    fn test(&mut self, graph: &Graph, compare: Compare, a: Operand, b: Operand) -> bool {
        let (a, b) = (self.value(graph, a), self.value(graph, b));
        match compare {
            Compare::Always => true,
            Compare::Eq => a == b,
            Compare::Ne => a != b,
            Compare::Lt => a < b,
            Compare::Le => a <= b,
            Compare::Gt => a > b,
            Compare::Ge => a >= b,
        }
    }

    /// 82957CA0: the first branch whose range holds `v`, else the one whose edge is nearest; `None` = stop.
    fn branch(graph: &Graph, node: u16, v: i32) -> Option<u16> {
        let branches = &graph.nodes.get(node as usize)?.branches;
        let hit = branches.iter().find(|b| i32::from(b.lo) <= v && v <= i32::from(b.hi));
        let pick = hit.or_else(|| branches.iter().min_by_key(|b| {
            let (lo, hi) = (i32::from(b.lo), i32::from(b.hi));
            if v < lo { lo - v } else { v - hi }
        }))?;
        pick.target
    }

    fn pick(&mut self, graph: &Graph, node: u16) -> Option<u16> {
        let n = graph.nodes.get(node as usize)?;
        let v = match n.kind {
            Kind::Router => (self.random() & 0x7F) as i32,
            Kind::Entry if n.random => (self.random() & 0x7F) as i32,
            Kind::Exit => {
                // The loop counter: 0 takes the (0, 0) branch (stop), else (1, 127) and counts down; -1 loops for ever.
                let start = n.loops;
                let counter = self.loops.entry(node).or_insert(start);
                let v = if *counter == 0 { 0 } else { 1 };
                if *counter > 0 {
                    *counter -= 1;
                } else if *counter == 0 {
                    *counter = start;
                }
                v
            }
            _ => self.control,
        };
        Self::branch(graph, node, v)
    }

    /// The next bank segment (0-based) when the playing one ends: a pending goto first, else the playing node's
    /// branch; entry / router / exit / event nodes are passed through. `None`: the stream stopped.
    pub fn next_segment(&mut self, graph: &Graph) -> Option<u32> {
        self.cut = false;
        let mut next = match self.goto.take() {
            Some(g) => Some(g),
            None => self.node.and_then(|c| self.pick(graph, c)),
        };
        for _ in 0..512 {
            let Some(id) = next else { break };
            let Some(node) = graph.nodes.get(id as usize) else { break };
            match node.kind {
                Kind::Audio(s) => {
                    self.node = Some(id);
                    return Some(u32::from(s) - 1);
                }
                Kind::Entry => {
                    self.entered.push((node.section, id));
                    next = self.pick(graph, id);
                }
                Kind::Event(e) => {
                    self.run(graph, e, 0);
                    next = match self.goto.take() {
                        Some(g) => Some(g),
                        None => self.pick(graph, id),
                    };
                }
                _ => next = self.pick(graph, id),
            }
        }
        self.node = None;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::pathfinder::format::{Branch, Event, Node};

    fn node(kind: Kind, section: u8, branches: &[(i8, i8, Option<u16>)]) -> Node {
        Node { kind, section, loops: 0, random: false, owner: 0,
            branches: branches.iter().map(|&(lo, hi, target)| Branch { lo, hi, target }).collect() }
    }

    /// Two tracks in section 1 (entry -> 2 segments -> exit), an event that picks entry #$track.
    fn graph() -> Graph {
        Graph {
            slot: 2,
            nodes: vec![
                node(Kind::Entry, 1, &[(0, 127, Some(1))]),
                node(Kind::Audio(1), 1, &[(0, 127, Some(2))]),
                node(Kind::Audio(2), 1, &[(0, 127, Some(3))]),
                node(Kind::Exit, 1, &[(0, 0, None), (1, 127, Some(0))]),
                node(Kind::Entry, 1, &[(0, 127, Some(5))]),
                node(Kind::Audio(3), 1, &[(0, 69, Some(6)), (70, 127, Some(7))]),
                node(Kind::Audio(4), 1, &[(0, 127, None)]),
                node(Kind::Audio(5), 1, &[(0, 127, None)]),
            ],
            events: vec![Event { id: 0x16D750, actions: vec![
                Action::Goto { node: Operand::Var(0), section: Some(Operand::Const(1)), cut: false },
                Action::Callback { a: 100, b: 7 },
            ] }],
            vars: vec![("track".into(), 0)],
            durations: vec![9500; 5],
        }
    }

    #[test]
    fn an_event_starts_a_track_and_it_plays_its_segments_then_stops() {
        let g = graph();
        let mut s = Stream::new(&g, 1);
        assert!(s.set_var(&g, "TRACK", 1) && !s.set_var(&g, "none", 1));
        assert!(s.post(&g, g.bank_bit() | 0x16D750), "the bank bit is ignored");
        assert_eq!(s.callbacks, [(100, 7)]);
        assert_eq!([s.next_segment(&g), s.next_segment(&g), s.next_segment(&g)], [Some(0), Some(1), None]);
        assert_eq!(s.entered, [(1, 0)]);
    }

    #[test]
    fn the_control_value_picks_the_branch_and_out_of_range_takes_the_nearest_edge() {
        let g = graph();
        let mut s = Stream::new(&g, 1);
        s.set_var(&g, "track", 2);
        s.post(&g, 0x16D750);
        assert_eq!(s.next_segment(&g), Some(2));
        s.control = 76;
        assert_eq!(s.next_segment(&g), Some(4), "76 takes 70..127");
        assert_eq!(Stream::branch(&g, 5, 69), Some(6));
        assert_eq!(Stream::branch(&g, 5, -5), Some(6), "below every range: nearest edge");
        assert_eq!(Stream::branch(&g, 6, 3), None, "a stop branch");
    }

    #[test]
    fn if_elif_else_and_arithmetic_run_as_written() {
        let mut g = graph();
        g.vars = vec![("a".into(), 0), ("b".into(), 10)];
        g.events.push(Event { id: 1, actions: vec![
            Action::If { elif: false, compare: Compare::Eq, a: Operand::Var(0), b: Operand::Const(1) },
            Action::Set { target: Operand::Var(1), value: Operand::Const(100) },
            Action::If { elif: true, compare: Compare::Eq, a: Operand::Var(0), b: Operand::Const(0) },
            Action::Set { target: Operand::Var(1), value: Operand::Const(200) },
            Action::Else,
            Action::Set { target: Operand::Var(1), value: Operand::Const(300) },
            Action::EndIf,
            Action::Arith { target: 1, op: 5, value: Operand::Const(7) },
            Action::Set { target: Operand::Prop(1), value: Operand::Const(500) },
        ] });
        let mut s = Stream::new(&g, 1);
        s.post(&g, 1);
        assert_eq!((s.vars[1], s.control), (200 % 7, 127), "elif taken, mod, control clamped");
    }
}
