//! The AEMS patch-program evaluator (spec: `.claude/notes/aems-evaluator-spec.md`).
//!
//! - Csis projects are installed first ([`Evaluator::install_project`]), then banks
//!   ([`Evaluator::load_bank`]): every export is resolved and each module registers as a constructor
//!   of its class.
//! - A post to a class ([`Evaluator::post`]) creates one instance per bound module (newest bank
//!   first, capacity permitting) and copies the payload into each instance's ClassData. The game
//!   keeps the returned node, redelivers new payloads and releases it.
//! - [`Evaluator::block`] is called once per 256-frame block; every 6th call walks the instance list
//!   (newest first) and runs each program over its own copy of the module's template.
//! - Instance memory is a byte copy of the template, read and written as big-endian words at the
//!   same offsets as retail, so programs run untranslated.
//! - Voices are driven through [`VoiceHost`] (the mixer in the game, a mock in tests).
pub mod fmath;
pub mod ops;
pub mod rng;
pub mod symbols;
#[doc(hidden)]
pub mod synthetic;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::be::*;
use crate::formats::abk::{InterfaceKind, Op};
use crate::formats::{Bank, FormatError};
use rng::Rng;
use symbols::{Client, Registry, SymRef};

/// What the evaluator hands the voice layer when a Player opens a voice (spec §5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRequest<'a> {
    pub bank: usize,
    /// S10A slot (the sample group entry's sample index).
    pub slot: u16,
    /// Sample group entry byte 2 (%). Meaning UNCERTAIN (voice-graph spec §3.4): not applied as gain.
    pub level: u8,
    /// Sample group entry bytes 3..8 as retail hands them over, each `<< 8` into 65536 = 360°.
    /// Bytes 3..7 are per-channel azimuths (stereo banks hold 224/32 = −45°/+45°, quad 224/32/160/96);
    /// byte 8 is the first byte of the stream-offset word at +8 (hence the `…, FF` seen on in-memory
    /// samples), so only the first five are angles.
    pub azimuth: [u8; 6],
    /// Sample group entry +8: stream offset (`FFFFFFFF` = in-memory sample).
    pub stream_offset: u32,
    /// The Player's input records (type id, value) in record order; ids ≥ 9 carry routing codes.
    pub inputs: &'a [(u8, i32)],
}

/// A voice's state as the Player op queries it (spec §5.3). Times are ms of source time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceStatus {
    pub alive: bool,
    pub remaining_ms: i32,
    pub elapsed_ms: i32,
}

/// The voice device the Player op drives. Voice handles are non-zero.
pub trait VoiceHost {
    fn open(&mut self, request: &OpenRequest) -> Option<u32>;
    fn release(&mut self, voice: u32);
    fn pause(&mut self, voice: u32);
    fn resume(&mut self, voice: u32);
    /// A voice property (PLAYER_INPUT id, spec §1.6), already clamped by the setter. Never id 3.
    fn set(&mut self, voice: u32, id: u8, value: i32);
    /// Property 3 (AZIMUTH), through the alternate setter: 65536 = 360°.
    fn set_azimuth(&mut self, voice: u32, value: i32);
    fn query(&mut self, voice: u32) -> VoiceStatus;
}

/// A post (Csis class instance) the game or a ControlClass op holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// Evaluator errors at load time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Format(FormatError),
}

impl From<FormatError> for LoadError {
    fn from(e: FormatError) -> Self {
        Self::Format(e)
    }
}

/// One executed op, for golden comparisons ([`Evaluator::trace`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpTrace {
    pub walk: u64,
    pub instance: u32,
    pub bank: usize,
    pub module: usize,
    pub opcode: u8,
    pub block: u32,
    pub result: i32,
}

struct LoadedModule {
    program: Arc<[Op]>,
    class: Option<usize>,
    /// Handles inside the template: instance offset → symbol.
    handles: HashMap<u32, SymRef>,
    live: i16,
}

struct LoadedBank {
    bank: Arc<Bank>,
    modules: Vec<LoadedModule>,
}

struct Instance {
    bank: usize,
    module: usize,
    node: u32,
    mem: Vec<u8>,
}

struct Node {
    class: usize,
    refcount: u32,
    class_data: Vec<Client>,
    destructors: Vec<Client>,
}

/// Blocks per walk with δ = f32(256/48000) and D = 30 (spec §2.6).
pub const BLOCKS_PER_WALK: u32 = 6;

/// The tick scale retail computes for that period: f32(f32(f32(6)·δ)·1000) = 31.999998 ms.
pub fn tick_scale() -> f32 {
    let delta = 256.0f32 / 48000.0f32;
    (BLOCKS_PER_WALK as f32 * delta) * 1000.0
}

pub struct Evaluator {
    pub registry: Registry,
    banks: Vec<Option<LoadedBank>>,
    instances: Vec<Option<Instance>>,
    free: Vec<u32>,
    /// Instance ids, newest first (the timer client list).
    order: VecDeque<u32>,
    nodes: HashMap<u32, Node>,
    next_node: u32,
    pub rng: Rng,
    tick: f32,
    countdown: u32,
    /// Completed walks.
    pub walks: u64,
    /// When Some, every executed op is appended (tests and golden runs).
    pub trace: Option<Vec<OpTrace>>,
    /// The walk's snapshot of `order` (kept between walks: no allocation per walk).
    walk_order: Vec<u32>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn word(m: &[u8], at: u32) -> i32 {
    i32_at(m, at as usize)
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            registry: Registry::default(),
            banks: Vec::new(),
            instances: Vec::new(),
            free: Vec::new(),
            order: VecDeque::new(),
            nodes: HashMap::new(),
            next_node: 1,
            rng: Rng::default(),
            tick: tick_scale(),
            countdown: BLOCKS_PER_WALK,
            walks: 0,
            trace: None,
            walk_order: Vec::new(),
        }
    }

    pub fn tick_scale(&self) -> f32 {
        self.tick
    }

    pub fn install_project(&mut self, project: &crate::formats::Project) {
        self.registry.install(project);
    }

    /// Install a bank: resolve its exports and register its modules on their classes. Projects
    /// must be installed first. Returns the bank id.
    pub fn load_bank(&mut self, bank: Bank) -> usize {
        let bank = Arc::new(bank);
        // The first slot an unloaded bank left (map changes would otherwise grow the list for
        // ever). Safe to reuse: unloading destroyed the bank's instances and dropped its
        // constructors; ids are only keys (posts reach banks in constructor order, not id order).
        let id = self.banks.iter().position(Option::is_none).unwrap_or(self.banks.len());
        let mut modules: Vec<LoadedModule> = bank
            .modules
            .iter()
            .map(|m| LoadedModule { program: m.program.clone().into(), class: None, handles: HashMap::new(), live: 0 })
            .collect();
        for e in &bank.exports {
            let Some(sym) = self.registry.lookup(e.kind, e.project, e.name_id, &e.name) else { continue };
            for (m, module) in bank.modules.iter().enumerate() {
                if e.handle_offset == module.offset + 4 && e.kind == InterfaceKind::Class {
                    if let SymRef::Class(c) = sym {
                        modules[m].class = Some(c);
                    }
                } else if (module.template_offset..module.template_offset + module.data_size).contains(&e.handle_offset) {
                    modules[m].handles.insert(e.handle_offset - module.template_offset, sym);
                }
            }
        }
        for (m, module) in modules.iter().enumerate() {
            if let Some(c) = module.class {
                self.registry.classes[c].constructors.push((id, m));
            }
        }
        let loaded = Some(LoadedBank { bank, modules });
        match self.banks.get_mut(id) {
            Some(slot) => *slot = loaded,
            None => self.banks.push(loaded),
        }
        id
    }

    /// Remove a bank: its live instances are destroyed (voices released) and its modules stop
    /// answering posts.
    pub fn unload_bank(&mut self, id: usize, host: &mut dyn VoiceHost) {
        let doomed: Vec<u32> = self.order.iter().copied().filter(|&i| self.instance(i).is_some_and(|x| x.bank == id)).collect();
        for i in doomed {
            self.destroy(i, host);
        }
        for class in &mut self.registry.classes {
            class.constructors.retain(|&(b, _)| b != id);
        }
        if let Some(slot) = self.banks.get_mut(id) {
            *slot = None;
        }
    }

    pub fn bank(&self, id: usize) -> Option<&Arc<Bank>> {
        self.banks.get(id)?.as_ref().map(|b| &b.bank)
    }

    pub fn class_id(&self, name: &str) -> Option<usize> {
        self.registry.by_name(InterfaceKind::Class, name)
    }

    pub fn function_id(&self, name: &str) -> Option<usize> {
        self.registry.by_name(InterfaceKind::Function, name)
    }

    pub fn global_id(&self, name: &str) -> Option<usize> {
        self.registry.by_name(InterfaceKind::GlobalVariable, name)
    }

    /// Live instances, newest first: (instance id, bank, module).
    pub fn instances(&self) -> Vec<(u32, usize, usize)> {
        self.order.iter().filter_map(|&i| self.instance(i).map(|x| (i, x.bank, x.module))).collect()
    }

    /// An instance's memory (tests, debugging).
    pub fn instance_memory(&self, id: u32) -> Option<&[u8]> {
        self.instance(id).map(|i| &i.mem[..])
    }

    /// The class a live post was made to.
    pub fn node_class(&self, node: NodeId) -> Option<usize> {
        self.nodes.get(&node.0).map(|n| n.class)
    }

    /// Live posts (class instances), including ControlClass children.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_refcount(&self, node: NodeId) -> Option<u32> {
        self.nodes.get(&node.0).map(|n| n.refcount)
    }

    fn instance(&self, id: u32) -> Option<&Instance> {
        self.instances.get(id as usize)?.as_ref()
    }

    fn instance_mut(&mut self, id: u32) -> Option<&mut Instance> {
        self.instances.get_mut(id as usize)?.as_mut()
    }

    // ---- posting (spec §2.2–2.5) -------------------------------------------------------------

    /// Post to a class: create the bound modules' instances (newest bank first) and deliver the
    /// payload. Succeeds even when nothing was created, like retail.
    pub fn post(&mut self, class: usize, payload: &[i32]) -> NodeId {
        let id = self.next_node;
        self.next_node = self.next_node.wrapping_add(1).max(1);
        self.nodes.insert(id, Node { class, refcount: 1, class_data: Vec::new(), destructors: Vec::new() });
        let constructors: Vec<(usize, usize)> =
            self.registry.classes.get(class).map(|c| c.constructors.iter().rev().copied().collect()).unwrap_or_default();
        for (bank, module) in constructors {
            self.create_instance(id, bank, module);
        }
        self.redeliver(NodeId(id), payload);
        NodeId(id)
    }

    /// Rewrite the payload of a held post (SetMemberData): only its ClassData clients rerun.
    pub fn redeliver(&mut self, node: NodeId, payload: &[i32]) {
        let Some(n) = self.nodes.get(&node.0) else { return };
        for &(inst, off) in &n.class_data {
            if let Some(Some(i)) = self.instances.get_mut(inst as usize) {
                let count = u8_at(&i.mem, off as usize + 16) as usize;
                for k in 0..count {
                    put_i32(&mut i.mem, off as usize + 20 + 4 * k, payload.get(k).copied().unwrap_or(0));
                }
            }
        }
    }

    /// Release a post: its instances see the ClassDestructor pulse; the poster's reference drops.
    pub fn release(&mut self, node: NodeId) {
        let Some(n) = self.nodes.get(&node.0) else { return };
        for &(inst, off) in &n.destructors {
            if let Some(Some(i)) = self.instances.get_mut(inst as usize) {
                put_i32(&mut i.mem, off as usize + 16, 1);
            }
        }
        self.unref(node.0);
    }

    fn unref(&mut self, node: u32) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.refcount = n.refcount.saturating_sub(1);
            if n.refcount == 0 {
                self.nodes.remove(&node);
            }
        }
    }

    fn create_instance(&mut self, node: u32, bank: usize, module: usize) {
        let Some(Some(lb)) = self.banks.get(bank) else { return };
        let m = &lb.bank.modules[module];
        if lb.modules[module].live >= m.max_instances {
            return;
        }
        let mem = m.template(&lb.bank.data).to_vec();
        let destructor = m.destructor_state;
        let class_data = m.class_data_state;
        let globals: Vec<(u32, Option<usize>)> = m
            .global_states
            .iter()
            .map(|&o| (o, match lb.modules[module].handles.get(&o) { Some(SymRef::Global(g)) => Some(*g), _ => None }))
            .collect();
        let functions: Vec<(u32, Option<usize>)> = m
            .function_states
            .iter()
            .map(|&o| (o, match lb.modules[module].handles.get(&o) { Some(SymRef::Function(f)) => Some(*f), _ => None }))
            .collect();
        let id = match self.free.pop() {
            Some(id) => id,
            None => {
                self.instances.push(None);
                (self.instances.len() - 1) as u32
            }
        };
        self.instances[id as usize] = Some(Instance { bank, module, node, mem });
        if let Some(Some(lb)) = self.banks.get_mut(bank) {
            lb.modules[module].live += 1;
        }
        let n = self.nodes.get_mut(&node).expect("post node");
        if let Some(off) = destructor {
            n.destructors.push((id, off));
            n.refcount += 1;
        }
        if let Some(off) = class_data {
            n.class_data.push((id, off));
            n.refcount += 1;
        }
        for (off, global) in globals {
            if let Some(g) = global {
                self.registry.globals[g].subscribers.push((id, off));
                let value = self.registry.globals[g].value;
                put_i32(&mut self.instances[id as usize].as_mut().unwrap().mem, off as usize + 24, value);
            }
        }
        for (off, function) in functions {
            if let Some(f) = function {
                self.registry.functions[f].subscribers.push((id, off));
            }
        }
        self.order.push_front(id);
    }

    fn destroy(&mut self, id: u32, host: &mut dyn VoiceHost) {
        let Some(inst) = self.instances.get_mut(id as usize).and_then(Option::take) else { return };
        self.free.push(id);
        self.order.retain(|&i| i != id);
        if let Some(n) = self.nodes.get_mut(&inst.node) {
            let before = n.destructors.len() + n.class_data.len();
            n.destructors.retain(|c| c.0 != id);
            n.class_data.retain(|c| c.0 != id);
            let dropped = before - n.destructors.len() - n.class_data.len();
            for _ in 0..dropped {
                self.unref(inst.node);
            }
        }
        for g in &mut self.registry.globals {
            g.subscribers.retain(|c| c.0 != id);
        }
        for f in &mut self.registry.functions {
            f.subscribers.retain(|c| c.0 != id);
        }
        let Some(Some(lb)) = self.banks.get_mut(inst.bank) else { return };
        lb.modules[inst.module].live -= 1;
        // The bank's handle (a reference count, no copy of the lists): `self.release` below needs
        // `self` while the module's object lists are read.
        let bank = lb.bank.clone();
        let module = &bank.modules[inst.module];
        for &p in module.players() {
            let voice = u32_at(&inst.mem, p as usize + 8);
            if voice != 0 {
                host.release(voice);
            }
        }
        for &c in module.controllers() {
            let child = u32_at(&inst.mem, c as usize + 8);
            if child != 0 {
                self.release(NodeId(child));
            }
        }
    }

    /// CallFunction (op 5, or game code): every subscriber copies its own number of parameters and
    /// sees the trigger at its next op 37. Returns −4 when nobody subscribes.
    pub fn call_function(&mut self, function: usize, params: &[i32]) -> i32 {
        let Some(f) = self.registry.functions.get(function) else { return -6 };
        if f.subscribers.is_empty() {
            return -4;
        }
        for (inst, off) in f.subscribers.clone() {
            if let Some(i) = self.instance_mut(inst) {
                let off = off as usize;
                let count = u8_at(&i.mem, off + 24) as usize;
                for k in 0..count {
                    put_i32(&mut i.mem, off + 28 + 4 * k, params.get(k).copied().unwrap_or(0));
                }
                put_u8(&mut i.mem, off + 25, 1);
            }
        }
        0
    }

    /// SetGlobalVariable (op 39 after its clamp, or game code): subscribers are told only when the
    /// stored value changes.
    pub fn set_global(&mut self, global: usize, value: i32) {
        let Some(g) = self.registry.globals.get_mut(global) else { return };
        if g.value == value {
            return;
        }
        g.value = value;
        for (inst, off) in g.subscribers.clone() {
            if let Some(i) = self.instance_mut(inst) {
                put_i32(&mut i.mem, off as usize + 24, value);
            }
        }
    }

    pub fn global(&self, global: usize) -> Option<i32> {
        self.registry.globals.get(global).map(|g| g.value)
    }

    // ---- the tick (spec §2.6, §3) --------------------------------------------------------------

    /// One 256-frame audio block. Every 6th call walks the instances. Returns true on a walk.
    pub fn block(&mut self, host: &mut dyn VoiceHost) -> bool {
        self.countdown -= 1;
        if self.countdown > 0 {
            return false;
        }
        self.countdown = BLOCKS_PER_WALK;
        self.walk(host);
        true
    }

    /// Run every instance's program once, newest first.
    pub fn walk(&mut self, host: &mut dyn VoiceHost) {
        // The order as the walk starts (instances created or destroyed meanwhile don't change it).
        let mut order = std::mem::take(&mut self.walk_order);
        order.clear();
        order.extend(self.order.iter().copied());
        for &id in &order {
            if self.instance(id).is_some() {
                self.run(id, host);
            }
        }
        self.walk_order = order;
        self.walks += 1;
    }

    fn run(&mut self, id: u32, host: &mut dyn VoiceHost) {
        let (bank, module) = match self.instance(id) {
            Some(i) => (i.bank, i.module),
            None => return,
        };
        let program = match &self.banks[bank] {
            Some(lb) => lb.modules[module].program.clone(),
            None => return,
        };
        for op in program.iter() {
            let result = self.exec(id, bank, module, op, host);
            if let Some(trace) = &mut self.trace {
                trace.push(OpTrace { walk: self.walks, instance: id, bank, module, opcode: op.opcode, block: op.block, result });
            }
            let Some(inst) = self.instance_mut(id) else { return }; // destroyed by op 4
            let m = &mut inst.mem;
            for &(src, dst) in &op.pairs {
                let to = op.block as i64 + i64::from(dst);
                let value = if src == -1 { result } else { i32_at(m, (op.block as i64 + i64::from(src)) as usize) };
                if to >= 0 {
                    put_i32(m, to as usize, value);
                }
            }
        }
    }

    fn exec(&mut self, id: u32, bank: usize, module: usize, op: &Op, host: &mut dyn VoiceHost) -> i32 {
        let b = op.block;
        match op.opcode {
            // ClassDestructor: the release pulse, read once.
            0 => self.take_word(id, b + 16),
            // ClassData: values[0]; programs fan out the other words with copy pairs.
            1 => self.instance(id).map_or(0, |i| word(&i.mem, b + 20)),
            // GlobalVariable: the subscribed value.
            2 => self.instance(id).map_or(0, |i| word(&i.mem, b + 24)),
            // Create: 1 on the first walk only.
            3 => self.take_word(id, b),
            // Destroy: the last op; ends the instance in this walk when triggered.
            4 => {
                if self.instance(id).is_some_and(|i| word(&i.mem, b + 12) != 0) {
                    self.destroy(id, host);
                }
                0
            }
            5 => self.call_function_op(id, bank, module, b),
            27 => self.player(id, bank, b, host),
            // Function: the call pulse, read once; programs copy the parameters out.
            37 => {
                let Some(i) = self.instance_mut(id) else { return 0 };
                let t = u8_at(&i.mem, b as usize + 25);
                put_u8(&mut i.mem, b as usize + 25, 0);
                i32::from(t)
            }
            38 => self.control_class(id, bank, module, b),
            39 => {
                let Some(i) = self.instance_mut(id) else { return 0 };
                let (min, max, prev, value) = (word(&i.mem, b + 8), word(&i.mem, b + 12), word(&i.mem, b + 16), word(&i.mem, b + 20));
                if value != prev {
                    put_i32(&mut i.mem, b as usize + 16, value);
                    if let Some(SymRef::Global(g)) = self.handle(bank, module, b) {
                        self.set_global(g, value.max(min).min(max));
                    }
                }
                0
            }
            opcode => {
                let Evaluator { banks, instances, rng, tick, .. } = self;
                let (Some(Some(lb)), Some(Some(inst))) = (banks.get(bank), instances.get_mut(id as usize)) else { return 0 };
                let mut ctx = ops::Ctx { bank: &lb.bank.data, rng, tick: *tick };
                ops::run(opcode, &mut inst.mem, b as usize, &mut ctx).unwrap_or(0)
            }
        }
    }

    fn handle(&self, bank: usize, module: usize, at: u32) -> Option<SymRef> {
        self.banks.get(bank)?.as_ref()?.modules[module].handles.get(&at).copied()
    }

    /// Read a word and clear it.
    fn take_word(&mut self, id: u32, at: u32) -> i32 {
        let Some(i) = self.instance_mut(id) else { return 0 };
        let v = word(&i.mem, at);
        put_i32(&mut i.mem, at as usize, 0);
        v
    }

    /// Clamp `n` parameters at `params` in place to the {min, max} pairs at `ranges`.
    fn clamp_params(mem: &mut [u8], ranges: u32, params: u32, n: u32) {
        for k in 0..n {
            let (min, max) = (word(mem, ranges + 8 * k), word(mem, ranges + 8 * k + 4));
            let v = word(mem, params + 4 * k);
            put_i32(mem, (params + 4 * k) as usize, v.max(min).min(max));
        }
    }

    /// The words from `at` to the end of the instance (a callee may read more parameters than the
    /// caller declares; retail then reads on into the caller's block).
    fn words_from(mem: &[u8], at: u32) -> Vec<i32> {
        (at as usize..mem.len()).step_by(4).map(|o| i32_at(mem, o)).collect()
    }

    /// Op 5 CallFunction: +0 function handle, +8 u8 clamp flag, +9 u8 n, +12 ranges (if flag),
    /// then {trigger, params[n]}. The trigger is not cleared.
    fn call_function_op(&mut self, id: u32, bank: usize, module: usize, b: u32) -> i32 {
        let target = self.handle(bank, module, b);
        let Some(i) = self.instance_mut(id) else { return 0 };
        let flag = u8_at(&i.mem, b as usize + 8) != 0;
        let n = u32::from(u8_at(&i.mem, b as usize + 9));
        let inputs = b + 12 + if flag { 8 * n } else { 0 };
        if flag {
            Self::clamp_params(&mut i.mem, b + 12, inputs + 4, n);
        }
        if word(&i.mem, inputs) != 0 {
            let params = Self::words_from(&i.mem, inputs + 4);
            if let Some(SymRef::Function(f)) = target {
                self.call_function(f, &params);
            }
        }
        0
    }

    /// Op 38 ControlClass: owns a child post (spec §4.6). +0 class handle, +8 node, +12 u8 clamp
    /// flag, +13 u8 n, +16 ranges (if flag), then {construct, destruct, params[n]}.
    fn control_class(&mut self, id: u32, bank: usize, module: usize, b: u32) -> i32 {
        let class = self.handle(bank, module, b);
        let Some(i) = self.instance_mut(id) else { return 0 };
        let mut node = u32_at(&i.mem, b as usize + 8);
        let flag = u8_at(&i.mem, b as usize + 12) != 0;
        let n = u32::from(u8_at(&i.mem, b as usize + 13));
        let inputs = b + 16 + if flag { 8 * n } else { 0 };
        let (construct, destruct) = (word(&i.mem, inputs), word(&i.mem, inputs + 4));
        if destruct != 0 {
            if node != 0 {
                self.release(NodeId(node));
                node = 0;
            }
        } else if construct != 0 {
            if node == 0 {
                if flag {
                    Self::clamp_params(&mut i.mem, b + 16, inputs + 8, n);
                }
                let params = Self::words_from(&i.mem, inputs + 8);
                if let Some(SymRef::Class(c)) = class {
                    node = self.post(c, &params).0;
                }
            }
        } else if node != 0 {
            if flag {
                Self::clamp_params(&mut i.mem, b + 16, inputs + 8, n);
            }
            let params = Self::words_from(&i.mem, inputs + 8);
            self.redeliver(NodeId(node), &params);
        }
        if let Some(i) = self.instance_mut(id) {
            put_u32(&mut i.mem, b as usize + 8, node);
        }
        if node == 0 { 0 } else { self.nodes.get(&node).map_or(0, |n| n.refcount as i32) }
    }

    /// Op 27 Player (spec §5.1). Layout: +4 sample group, +8 voice, +12/+13 previous play
    /// controls, +14 u8 input count, +15 u8 update outputs, +20 sample select, +24 play control,
    /// +28 inputs {u8 id, …, +4 applied, +8 value}, then {time left, time current}.
    fn player(&mut self, id: u32, bank: usize, b: u32, host: &mut dyn VoiceHost) -> i32 {
        let Evaluator { banks, instances, .. } = self;
        let (Some(Some(lb)), Some(Some(inst))) = (banks.get(bank), instances.get_mut(id as usize)) else { return 0 };
        let data = &lb.bank.data[..];
        let m = &mut inst.mem;
        let b = b as usize;
        let control = i32_at(m, b + 24).clamp(0, 2);
        let (prev0, prev1) = (i32::from(i8_at(m, b + 12)), i32::from(i8_at(m, b + 13)));
        let mut voice = u32_at(m, b + 8);
        let n = u8_at(m, b + 14) as usize;
        let update = u8_at(m, b + 15) != 0;
        let extra = u8_at(m, b + 17) != 0;
        let outputs = b + 28 + 12 * n;
        let clear = |m: &mut [u8]| {
            if update {
                for k in 0..(2 + if extra { 8 } else { 0 }) {
                    put_i32(m, outputs + 4 * k, 0);
                }
            }
        };
        let push = |m: &mut [u8], voice: u32, k: usize, host: &mut dyn VoiceHost| {
            let at = b + 28 + 12 * k;
            let (kind, value) = (u8_at(m, at), i32_at(m, at + 8));
            set_property(host, voice, kind, value);
            put_i32(m, at + 4, value);
        };
        if control != prev0 {
            match control {
                0 => {
                    if voice != 0 {
                        host.release(voice);
                        voice = 0;
                        clear(m);
                    }
                }
                1 => {
                    if voice != 0 {
                        host.resume(voice);
                    } else if !(prev0 == 2 && prev1 == 1) {
                        let group = u32_at(m, b + 4) as usize;
                        let count = u32_at(data, group) as i32;
                        let k = i32_at(m, b + 20).clamp(0, (count - 1).max(0)) as usize;
                        let entry = group + 4 + 12 * k;
                        let slot = u16_at(data, entry);
                        if count <= 0 || slot == 0xFFFF {
                            clear(m);
                        } else {
                            let inputs: Vec<(u8, i32)> =
                                (0..n).map(|k| (u8_at(m, b + 28 + 12 * k), i32_at(m, b + 36 + 12 * k))).collect();
                            let mut azimuth = [0u8; 6];
                            azimuth.copy_from_slice(&data[entry + 3..entry + 9]);
                            let request = OpenRequest {
                                bank,
                                slot,
                                level: u8_at(data, entry + 2),
                                azimuth,
                                stream_offset: u32_at(data, entry + 8),
                                inputs: &inputs,
                            };
                            match host.open(&request) {
                                Some(v) => {
                                    voice = v;
                                    // Open applies the full input set.
                                    for k in 0..n {
                                        push(m, voice, k, host);
                                    }
                                }
                                None => clear(m),
                            }
                        }
                    }
                }
                _ => {
                    if voice != 0 {
                        host.pause(voice);
                    }
                }
            }
            put_u8(m, b + 13, prev0 as u8);
            put_u8(m, b + 12, control as u8);
        }
        let mut result = match (voice != 0, control) {
            (true, 1) => 1,
            (true, 2) => 2,
            _ => 0,
        };
        if control == 1 && voice != 0 {
            for k in 0..n {
                let at = b + 28 + 12 * k;
                if i32_at(m, at + 8) != i32_at(m, at + 4) {
                    push(m, voice, k, host);
                }
            }
            let status = host.query(voice);
            if !status.alive {
                host.release(voice);
                voice = 0;
                clear(m);
                result = 0;
            } else if update {
                put_i32(m, outputs, status.remaining_ms);
                put_i32(m, outputs + 4, status.elapsed_ms);
            }
        }
        put_u32(m, b + 8, voice);
        result
    }
}

/// The property setter (`sub_82B1BE30`): clamp by id, then hand to the voice. Id 3 goes to the
/// alternate (azimuth) setter.
pub fn set_property(host: &mut dyn VoiceHost, voice: u32, id: u8, value: i32) {
    match id {
        3 => host.set_azimuth(voice, value),
        0 | 6 | 7 => host.set(voice, id, value.clamp(0, 65535)),
        2 | 5 | 8 => host.set(voice, id, value.clamp(0, 32767)),
        _ => host.set(voice, id, value),
    }
}

#[cfg(test)]
mod tests;
