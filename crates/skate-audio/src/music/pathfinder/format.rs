//! EA PathFinder v5.3 music graphs (`.mpf`, "PFDx"; research b1016 §1): big endian, bounds checked. A node plays a
//! segment or routes to the next one by a 0..127 value; events are lists of 12-byte actions the game posts by id.
//! Docs: docs/hails-additions/11-audio.md#music-banks.

/// A node's role (+0x00, signed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Plays bank segment `n - 1` (the file's sample `n - 1`, vgmstream subsong `n`).
    Audio(u16),
    /// A section entry: notifies the game; random-flagged entries pick their branch at random.
    Entry,
    /// An exit / loop node: its loop count decides whether it stops or loops back.
    Exit,
    /// A random router: always picks with a random 0..127 value.
    Router,
    /// Posts its event (id24) when reached.
    Event(u32),
    Other(i16),
}

/// One branch: a target for values `lo..=hi` (signed, inclusive); `None` = stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Branch {
    pub lo: i8,
    pub hi: i8,
    pub target: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    /// Section id (+0x02 bits 5..10).
    pub section: u8,
    /// Exit nodes: the loop count (+0x02 bits 0..4, 5-bit signed: -1 loops for ever).
    pub loops: i8,
    /// +0x04 bits 12..14 == 1: pick with a random value (entry nodes).
    pub random: bool,
    /// Audio / router nodes: the entry node they belong to; entry / exit nodes: an ordinal (+0x08).
    pub owner: u16,
    pub branches: Vec<Branch>,
}

/// An action's operand: a constant, a variable (index) or a stream property (1 control value, 2 node, 3 entry of the
/// node, 4 section of the node, 18 random, 19 timer, 20 playback position).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Const(i32),
    Var(u8),
    Prop(u8),
}

fn operand(mode: u32, value: i32) -> Operand {
    match mode {
        2 => Operand::Var(value as u8),
        1 => Operand::Prop(value as u8),
        _ => Operand::Const(value),
    }
}

/// Comparisons of an if / elif (b1016 §1: 0 always, 1 ==, 2 !=, 3..6 ordered).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Always,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    If { elif: bool, compare: Compare, a: Operand, b: Operand },
    Else,
    EndIf,
    /// Wait a number of ms (NOT RETAIL YET in our stream: no timeline, run on).
    Wait(Operand),
    /// Wait for a beat / bar boundary (parameters open).
    WaitSync(u32),
    /// Go to `node`, or (with a section) to the `node`-th (1-based) entry of that section; `cut` switches now, else at
    /// the next segment end.
    Goto { node: Operand, section: Option<Operand>, cut: bool },
    /// Set a variable (or stream property 1, the control value).
    Set { target: Operand, value: Operand },
    Post(u32),
    Callback { a: i16, b: i16 },
    /// `target = op(target, value)` (op 1 add, 2 sub (value - target, research inferred), 3 mul, 4 div, 5 mod).
    Arith { target: u8, op: u8, value: Operand },
    Unknown { kind: u8, b: u32, c: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: u32,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    /// Header 0x0C: the event / action bank bit is `0x0100_0000 << slot`.
    pub slot: u8,
    pub nodes: Vec<Node>,
    pub events: Vec<Event>,
    /// (name, initial value).
    pub vars: Vec<(String, i32)>,
    /// Per sample: duration in ms.
    pub durations: Vec<u32>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&self, at: usize) -> Result<u8, String> {
        self.0.get(at).copied().ok_or_else(|| format!("PathFinder: read past the end at {at:#x}"))
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        Ok(u16::from_be_bytes([self.u8(at)?, self.u8(at + 1)?]))
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        Ok(u32::from_be_bytes([self.u8(at)?, self.u8(at + 1)?, self.u8(at + 2)?, self.u8(at + 3)?]))
    }
}

fn action(b: u32, c: u32) -> Action {
    let kind = ((b >> 17) & 0x7F) as u8;
    let (m1, m2) = ((b >> 14) & 3, (b >> 12) & 3);
    let hi = (c >> 16) as u16 as i16 as i32;
    let lo = c as u16 as i16 as i32;
    match kind {
        1 => match (b >> 9) & 7 {
            3 => Action::Else,
            4 => Action::EndIf,
            sub => Action::If {
                elif: sub == 2,
                compare: match (b >> 6) & 7 {
                    1 => Compare::Eq,
                    2 => Compare::Ne,
                    3 => Compare::Lt,
                    4 => Compare::Le,
                    5 => Compare::Gt,
                    6 => Compare::Ge,
                    _ => Compare::Always,
                },
                a: operand(m1, hi),
                b: operand(m2, lo),
            },
        },
        2 => Action::Wait(operand(m2, hi)),
        3 => Action::WaitSync(c),
        4 => {
            let section = ((c >> 8) & 0xFF) as u8 as i8 as i32;
            // A constant section below 0 means "no section": the node operand is a node index.
            let section = match operand(m2, section) {
                Operand::Const(s) if s < 0 => None,
                s => Some(s),
            };
            Action::Goto { node: operand(m1, hi), section, cut: c & 0x80 != 0 }
        }
        8 => Action::Set { target: operand(m1, ((c >> 8) & 0xFF) as i32), value: operand(m2, hi) },
        9 => Action::Post(c >> 8),
        13 => Action::Callback { a: lo as i16, b: hi as i16 },
        14 => Action::Arith { target: (c >> 24) as u8, op: ((c >> 16) & 0xFF) as u8, value: operand(m2, lo) },
        _ => Action::Unknown { kind, b, c },
    }
}

impl Graph {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let r = Reader(data);
        if data.get(0..4) != Some(b"PFDx") || r.u8(4)? != 5 || r.u8(5)? != 3 {
            return Err("PathFinder: not a version 5.3 PFDx graph".into());
        }
        let slot = r.u8(0x0C)?;
        let events_n = r.u8(0x0F)? as usize;
        let vars_n = r.u8(0x11)? as usize;
        let nodes_n = r.u16(0x12)? as usize;
        let table = |i: usize| r.u32(0x14 + 4 * i).map(|v| v as usize);
        let (node_index, event_index, vars_at, samples_at, end) = (table(0)?, table(2)?, table(4)?, table(8)?, table(9)?);
        let mut nodes = Vec::with_capacity(nodes_n);
        for i in 0..nodes_n {
            let at = r.u16(node_index + 2 * i)? as usize * 4;
            let seg = r.u16(at)? as i16;
            let f2 = r.u16(at + 2)?;
            let f4 = r.u32(at + 4)?;
            let count = ((f4 >> 15) & 0x1F) as usize;
            let kind = match seg {
                s if s > 0 => Kind::Audio(s as u16),
                0 => Kind::Entry,
                -1 => Kind::Exit,
                -2 => Kind::Router,
                -3 => Kind::Event(r.u32(at + 0x0C)? >> 8),
                other => Kind::Other(other),
            };
            let loops = ((((f2 & 0x1F) as i8) << 3) >> 3) as i8;
            let mut branches = Vec::with_capacity(count);
            for b in 0..count {
                let p = at + 0x10 + 4 * b;
                let target = r.u16(p + 2)?;
                if target != 0xFFFF && target as usize >= nodes_n {
                    return Err(format!("PathFinder: node {i} branch to {target} of {nodes_n}"));
                }
                branches.push(Branch { lo: r.u8(p)? as i8, hi: r.u8(p + 1)? as i8, target: (target != 0xFFFF).then_some(target) });
            }
            nodes.push(Node { kind, section: ((f2 >> 5) & 0x3F) as u8, loops, random: (f4 >> 12) & 7 == 1, owner: r.u16(at + 8)?, branches });
        }
        let mut events = Vec::with_capacity(events_n);
        for i in 0..events_n {
            let at = r.u16(event_index + 2 * i)? as usize * 4;
            let head = r.u32(at + 0x0C)?;
            let n = (head & 0xFF) as usize;
            let actions = (0..n).map(|k| {
                let p = at + 0x14 + 12 * k;
                Ok(action(r.u32(p + 4)?, r.u32(p + 8)?))
            }).collect::<Result<Vec<_>, String>>()?;
            events.push(Event { id: head >> 8, actions });
        }
        let vars = (0..vars_n).map(|i| {
            let at = vars_at + 0x14 * i;
            let name = data.get(at..at + 0x10).ok_or("PathFinder: variable past the end")?;
            let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
            Ok((String::from_utf8_lossy(name).into_owned(), r.u32(at + 0x10)? as i32))
        }).collect::<Result<Vec<_>, String>>()?;
        let durations = (samples_at..end.min(data.len())).step_by(8).take_while(|p| p + 8 <= end).map(|p| r.u32(p + 4)).collect::<Result<Vec<_>, String>>()?;
        for (i, n) in nodes.iter().enumerate() {
            if let Kind::Audio(s) = n.kind {
                if s as usize > durations.len() {
                    return Err(format!("PathFinder: node {i} plays sample {s} of {}", durations.len()));
                }
            }
        }
        Ok(Self { slot, nodes, events, vars, durations })
    }

    /// The bank bit events are posted with (`0x0100_0000 << slot`).
    pub fn bank_bit(&self) -> u32 {
        0x0100_0000u32.checked_shl(u32::from(self.slot)).unwrap_or(0)
    }

    /// The entry nodes of `section`, in node order (goto by ordinal).
    pub fn entries(&self, section: u8) -> Vec<u16> {
        self.nodes.iter().enumerate().filter(|(_, n)| n.kind == Kind::Entry && n.section == section).map(|(i, _)| i as u16).collect()
    }

    pub fn var(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|(n, _)| n.eq_ignore_ascii_case(name))
    }
}
