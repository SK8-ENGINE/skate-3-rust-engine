//! The front-end stage: several retail menu movies in ONE script VM sharing `_global`, like retail's APT player.
//! Details: docs/hails-additions/31-retail-menus.md#front-end-stage
use crate::{
    apt_imports::library_key,
    apt_movie::{Movie, Pending, CLASS_CONSTRUCTOR},
    apt_vm::{Host, ObjectKind, Value, Vm},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Host objects the retail C++ registers for the AS side (sub_825A2190 creates ~40: Game,
/// Audio, CoreMenu, ...). Calls on them go to `call`.
pub trait StageNatives {
    fn objects(&self) -> Vec<String>;
    fn call(
        &mut self,
        vm: &mut Vm,
        object: &str,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String>;
}

/// Linked movies by retail key (`apt_imports::library_key`).
pub trait MovieSource {
    fn movie(&self, key: &str) -> Option<Movie>;
}
impl MovieSource for BTreeMap<String, Movie> {
    fn movie(&self, key: &str) -> Option<Movie> {
        self.get(key).cloned()
    }
}

/// One movie playing on the stage.
pub struct Level {
    pub key: String,
    pub movie: Movie,
    /// Milliseconds not yet turned into frames.
    elapsed: f32,
}

/// Timeline rate when a movie header gives none (the retail HUD and most menus use 16 ms;
/// doc 31 "Milestone 5a").
const DEFAULT_MS_PER_FRAME: f32 = 16.0;
/// Frames a single update may advance one movie (a long hitch does not replay seconds of
/// animation at once; NOT RETAIL: retail's catch-up rule is not decoded).
const MAX_FRAMES_PER_UPDATE: usize = 4;
const ACTION_LIMIT: usize = 8192;

struct Load {
    key: String,
    target: usize,
    loader: usize,
}

struct State {
    levels: Vec<Level>,
    loads: VecDeque<Load>,
    /// Clips made by createEmptyMovieClip: id -> (parent, depth).
    dynamic: BTreeMap<usize, (usize, i32)>,
    loader_prototype: usize,
    /// The global `Object` (registerClass lives on it).
    object_class: usize,
    /// registerClass bindings: linkage name -> class function.
    classes: BTreeMap<String, usize>,
    /// Library movies whose root init actions already ran (once per stage, like a library
    /// that stays loaded).
    libraries: BTreeSet<String>,
    native_names: BTreeSet<String>,
    /// Calls nothing handled (method on object kind), for diagnostics and tests.
    pub unhandled: BTreeSet<String>,
    traces: Vec<String>,
}

pub struct Stage {
    pub vm: Vm,
    state: State,
    /// Object count at which the next update ends with a collection (twice what survived the last one).
    collect_at: usize,
}

/// Fewest objects before the stage collects at all (the booted front end alone holds about this many).
const MIN_COLLECT_AT: usize = 1024;

struct Bindings<'a> {
    state: &'a mut State,
    natives: &'a mut dyn StageNatives,
}

fn new_clip(vm: &mut Vm, parent: usize, name: &str) -> Result<usize, String> {
    let id = vm.object(ObjectKind::Native("movie:empty".into()));
    let root = vm.get(parent, "_root");
    vm.set(id, "_root", if root == Value::Undefined { Value::Object(parent) } else { root })?;
    vm.set(id, "_parent", Value::Object(parent))?;
    vm.set(id, "_name", Value::Text(name.into()))?;
    vm.set(id, "_x", Value::Number(0.0))?;
    vm.set(id, "_y", Value::Number(0.0))?;
    vm.set(id, "_visible", Value::Bool(true))?;
    vm.set(id, "_alpha", Value::Number(100.0))?;
    if !name.is_empty() {
        vm.set(parent, name, Value::Object(id))?;
    }
    Ok(id)
}

/// Movie key of a loadClip url. Retail ScreenManagerBase::OpenScreen (sub_82D09140) passes
/// "source/screens/" + the template's path, which names the authoring file (`main/core_menu.swf`,
/// template table 0x8302F1A8); the loader opens the packed `.apt` of the same name.
fn screen_key(url: &str) -> String {
    let url = url.strip_suffix(".swf").unwrap_or(url);
    library_key(url)
}

impl Bindings<'_> {
    fn level_of(&self, id: usize) -> Option<usize> {
        self.state.levels.iter().position(|l| l.movie.instances.contains_key(&id))
    }
    fn clip_call(
        &mut self,
        vm: &mut Vm,
        object: usize,
        method: &str,
        args: &[Value],
    ) -> Result<Option<Value>, String> {
        match method {
            "createEmptyMovieClip" => {
                let name = args.first().map(Value::text).unwrap_or_default();
                let depth = args.get(1).map_or(0, crate::apt_vm::to_integer);
                let id = new_clip(vm, object, &name)?;
                self.state.dynamic.insert(id, (object, depth));
                return Ok(Some(Value::Object(id)));
            }
            // An attachMovie clip: the movie removes it (apt_movie `method`).
            "removeMovieClip" if self.level_of(object).is_some_and(|l| self.state.levels[l].movie.attached.contains(&object)) => {}
            "removeMovieClip" | "unloadMovie" => {
                self.unload_into(vm, object);
                if method == "removeMovieClip" {
                    if let Some((parent, _)) = self.state.dynamic.remove(&object) {
                        let name = vm.get(object, "_name").text();
                        if vm.get(parent, &name) == Value::Object(object) {
                            vm.objects[parent].fields.remove(&name);
                        }
                        vm.set(object, "_visible", Value::Bool(false))?;
                    }
                }
                return Ok(Some(Value::Undefined));
            }
            "setTextFormat" | "setNewTextFormat" => {
                // TextField.setTextFormat: the format colour becomes the field's text colour (NOT RETAIL YET: AS2 semantics).
                if let (Some(Value::Object(format)), Some(level)) = (args.last(), self.level_of(object)) {
                    self.state.levels[level].movie.set_text_format(vm, object, *format)?;
                }
                return Ok(Some(Value::Undefined));
            }
            "loadMovie" => {
                // MovieClip.loadMovie(url): the movie replaces this clip's content on a later update (main.apt
                // fills gScreenManager / gChyron / gSubtitle / gFadeBackground this way).
                if let Some(url) = args.first() {
                    self.state.loads.push_back(Load { key: screen_key(&url.text()), target: object, loader: usize::MAX });
                }
                return Ok(Some(Value::Undefined));
            }
            "getDepth" => {
                if let Some((_, depth)) = self.state.dynamic.get(&object) {
                    return Ok(Some(Value::Number(*depth as f64)));
                }
                if let Some(level) = self.level_of(object) {
                    let movie = &self.state.levels[level].movie;
                    if let Value::Object(parent) = vm.get(object, "_parent") {
                        if let Some((key, _)) = movie.instances.get(&parent).and_then(|p| p.children.iter().find(|(_, c)| **c == object)) {
                            return Ok(Some(Value::Number((*key - crate::apt_movie::SCRIPT_DEPTH) as f64)));
                        }
                    }
                }
            }
            "attachMovie" => {
                // (linkage, new name, depth[, init object])
                let linkage = args.first().map(Value::text).unwrap_or_default();
                let name = args.get(1).map(Value::text).unwrap_or_default();
                let depth = args.get(2).map_or(0, crate::apt_vm::to_integer);
                let init = match args.get(3) {
                    Some(Value::Object(o)) => Some(*o),
                    _ => None,
                };
                if let Some(level) = self.level_of(object) {
                    let movie = &mut self.state.levels[level].movie;
                    let before = movie.immediate.len();
                    let Some(child) = movie.attach(vm, object, &linkage, &name, depth, init)? else {
                        return Ok(Some(Value::Undefined));
                    };
                    // The class constructors of the new subtree run inside attachMovie, before it returns, nested
                    // clips first (standard AS2; the caller sets titles right after, which later constructors
                    // would overwrite with their defaults). NOT RETAIL YET: retail's order is not decoded.
                    let queued: Vec<Pending> = movie.immediate.drain(before.min(movie.immediate.len())..).collect();
                    let (mut ctors, rest): (Vec<Pending>, Vec<Pending>) =
                        queued.into_iter().partition(|p| p.event == CLASS_CONSTRUCTOR);
                    movie.immediate.extend(rest);
                    ctors.sort_by_key(|p| p.object == child);
                    let ctors: Vec<(usize, usize)> = ctors
                        .iter()
                        .filter_map(|p| Some((p.object, *movie.classes.get(&movie.instances.get(&p.object)?.character)?)))
                        .collect();
                    for (clip, class) in ctors {
                        vm.call_function(class, clip, vec![], self)?;
                    }
                    return Ok(Some(Value::Object(child)));
                }
            }
            "getNextHighestDepth" => {
                if let Some(level) = self.level_of(object) {
                    let next = self.state.levels[level]
                        .movie
                        .instances
                        .get(&object)
                        .and_then(|i| i.children.keys().next_back().copied())
                        .map_or(0, |k| (k - crate::apt_movie::SCRIPT_DEPTH + 1).max(0));
                    return Ok(Some(Value::Number(next as f64)));
                }
            }
            _ => {}
        }
        if let Some(level) = self.level_of(object) {
            if self.state.levels[level].movie.method(vm, object, method, args)? {
                return Ok(Some(Value::Undefined));
            }
        }
        Ok(None)
    }
    /// Removes a movie loaded into `target`, if any (unload queued like a removal).
    fn unload_into(&mut self, vm: &mut Vm, target: usize) {
        // A load still queued for this clip is cancelled: a screen opened and closed in one frame (the Challenge Map
        // teleport reveals the pause menu and closes it at once) otherwise loaded into the dead holder on the next
        // update and stayed rooted forever (laptop session 2026-10-18: four teleports, then the object budget).
        self.state.loads.retain(|l| l.target != target);
        if let Some(i) = self.state.levels.iter().position(|l| l.movie.root == target) {
            let mut level = self.state.levels.remove(i);
            level.movie.unload(vm);
            // Its queued unload handlers still run (retail sub_82E55DA8 queues them).
            level.movie.instances.clear();
            let pending: Vec<_> = std::iter::from_fn(|| level.movie.next_action()).collect();
            if !pending.is_empty() {
                level.movie.pending = pending.into();
                self.state.levels.push(Level { key: format!("{}#unloading", level.key), ..level });
            }
        }
    }
}

impl Host for Bindings<'_> {
    fn property_changed(&mut self, vm: &mut Vm, object: usize, key: &str) -> Result<(), String> {
        // Same as the HUD host: a new text or autosize re-localises and re-measures the field.
        if key.eq_ignore_ascii_case("_x") || key.eq_ignore_ascii_case("_y") {
            if let Some(level) = self.level_of(object) {
                self.state.levels[level].movie.script_moved.insert(object);
            }
        }
        if ["_xscale", "_yscale", "_rotation", "_width", "_height"].iter().any(|k| key.eq_ignore_ascii_case(k)) {
            if let Some(level) = self.level_of(object) {
                self.state.levels[level].movie.transform_changed(vm, object, key)?;
            }
        }
        if key.eq_ignore_ascii_case("text") || key.eq_ignore_ascii_case("autoSize") || key.eq_ignore_ascii_case("wordWrap") {
            if let Some(level) = self.level_of(object) {
                self.state.levels[level].movie.text_changed(vm, object)?;
            }
        }
        Ok(())
    }
    fn trace(&mut self, message: &str) {
        self.state.traces.push(message.into());
    }
    fn get_url(&mut self, vm: &mut Vm, _this: usize, url: &str, target: &str) -> Result<(), String> {
        if let Some(command) = url.strip_prefix("FSCommand:") {
            self.state.unhandled.insert(format!("fscommand {command} {target}"));
            return Ok(());
        }
        // main.apt loads main3dhud / autosave / fmv_popup into _level1.._level3. A level is a new root clip
        // (`_levelN` in _global); like retail's load queue it appears on a later update.
        if let Some(n) = target.strip_prefix("_level").and_then(|n| n.parse::<u32>().ok()) {
            let name = format!("_level{n}");
            let clip = match vm.get(vm.global, &name) {
                Value::Object(id) => id,
                _ => {
                    let id = vm.object(ObjectKind::Native("movie:empty".into()));
                    vm.set(id, "_name", Value::Text(name.clone()))?;
                    vm.set(id, "_visible", Value::Bool(true))?;
                    vm.set(id, "_alpha", Value::Number(100.0))?;
                    vm.set(id, "_x", Value::Number(0.0))?;
                    vm.set(id, "_y", Value::Number(0.0))?;
                    vm.set(vm.global, &name, Value::Object(id))?;
                    id
                }
            };
            self.state.loads.push_back(Load { key: screen_key(url), target: clip, loader: usize::MAX });
            return Ok(());
        }
        self.state.unhandled.insert(format!("getURL {url} {target}"));
        Ok(())
    }
    fn call(
        &mut self,
        vm: &mut Vm,
        object: usize,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let kind = vm.objects.get(object).map(|o| o.kind.clone());
        if let Some(ObjectKind::Native(name)) = &kind {
            if self.state.native_names.contains(name) {
                return self.natives.call(vm, name, method, args);
            }
            if name.starts_with("movie:") {
                if let Some(v) = self.clip_call(vm, object, method, &args)? {
                    return Ok(v);
                }
            }
        }
        if vm.objects.get(object).and_then(|o| o.prototype) == Some(self.state.loader_prototype) {
            match method {
                "addListener" | "removeListener" => {
                    let listeners = match vm.get(object, "_listeners") {
                        Value::Object(id) => id,
                        _ => {
                            let id = vm.array(vec![])?;
                            vm.set(object, "_listeners", Value::Object(id))?;
                            id
                        }
                    };
                    let n = vm.get(listeners, "length").number() as usize;
                    let mut items: Vec<Value> =
                        (0..n).map(|i| vm.get(listeners, &i.to_string())).collect();
                    let listener = args.first().cloned().unwrap_or_default();
                    items.retain(|v| *v != listener);
                    if method == "addListener" {
                        items.push(listener);
                    }
                    let id = vm.array(items)?;
                    vm.set(object, "_listeners", Value::Object(id))?;
                    return Ok(Value::Bool(true));
                }
                "loadClip" => {
                    let (Some(url), Some(Value::Object(target))) = (args.first(), args.get(1))
                    else {
                        return Ok(Value::Bool(false));
                    };
                    // Retail loads asynchronously (load queue sub_82E7E9F8); the movie appears
                    // on a later update, after the calling script finished.
                    self.state.loads.push_back(Load {
                        key: screen_key(&url.text()),
                        target: *target,
                        loader: object,
                    });
                    return Ok(Value::Bool(true));
                }
                "unloadClip" => {
                    if let Some(Value::Object(target)) = args.first() {
                        self.unload_into(vm, *target);
                    }
                    return Ok(Value::Bool(true));
                }
                _ => {}
            }
        }
        if object == self.state.object_class && method == "registerClass" {
            // Object.registerClass(linkage, class): clips created from that linkage name
            // later get the class (menu_picker, tabs, highlight_* register this way).
            if let (Some(name), Some(Value::Object(class))) = (args.first(), args.get(1)) {
                self.state.classes.insert(name.text(), *class);
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Bool(false));
        }
        if object == vm.global && method == "ASSetPropFlags" {
            return Ok(Value::Undefined);
        }
        let owner = match &kind {
            Some(ObjectKind::Native(name)) => name.clone(),
            _ if object == vm.global => "_global".into(),
            _ => "object".into(),
        };
        self.state
            .unhandled
            .insert(format!("{owner}.{method} (at {:x})", vm.last_instruction.1));
        Ok(Value::Undefined)
    }
}

impl Stage {
    pub fn new(natives: &dyn StageNatives) -> Result<Self, String> {
        let mut vm = Vm::new();
        let mut native_names = BTreeSet::new();
        let mut loader_prototype = 0;
        let mut object_class = 0;
        // Math is the VM's own object (apt_vm `math`).
        for name in ["MovieClip", "MovieClipLoader", "Object", "Array", "String"] {
            let object = vm.object(ObjectKind::Plain);
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(object))?;
            if name == "MovieClipLoader" {
                loader_prototype = prototype;
            }
            if name == "Object" {
                object_class = object;
            }
        }
        for name in natives.objects() {
            let object = vm.object(ObjectKind::Native(name.clone()));
            vm.set(vm.global, &name, Value::Object(object))?;
            native_names.insert(name);
        }
        Ok(Self {
            vm,
            state: State {
                levels: Vec::new(),
                loads: VecDeque::new(),
                dynamic: BTreeMap::new(),
                loader_prototype,
                object_class,
                classes: BTreeMap::new(),
                libraries: BTreeSet::new(),
                native_names,
                unhandled: BTreeSet::new(),
                traces: Vec::new(),
            },
            collect_at: MIN_COLLECT_AT,
        })
    }
    /// A script-less empty clip under `parent` (what retail C++ makes to hold the screen manager movie).
    pub fn create_clip(&mut self, parent: usize, name: &str) -> Result<usize, String> {
        let id = new_clip(&mut self.vm, parent, name)?;
        self.state.dynamic.insert(id, (parent, 0));
        Ok(id)
    }
    pub fn live_objects(&self) -> usize {
        self.vm.live_objects()
    }
    pub fn levels(&self) -> &[Level] {
        &self.state.levels
    }
    pub fn unhandled(&self) -> &BTreeSet<String> {
        &self.state.unhandled
    }
    pub fn take_traces(&mut self) -> Vec<String> {
        std::mem::take(&mut self.state.traces)
    }
    /// The root movie (retail FE_root): loaded at the stage root, `_level0`.
    pub fn load_root(
        &mut self,
        key: &str,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        let movie = source.movie(key).ok_or_else(|| format!("APT stage: no movie {key}"))?;
        let root = self.start(key, movie, None, source, natives)?;
        self.vm.set(self.vm.global, "_level0", Value::Object(root))?;
        Ok(root)
    }
    /// Loads `key` into the clip `target` now (what a queued loadClip does on the next update).
    pub fn load_into(
        &mut self,
        key: &str,
        target: usize,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        let movie = source.movie(key).ok_or_else(|| format!("APT stage: no movie {key}"))?;
        self.start(key, movie, Some(target), source, natives)
    }
    fn start(
        &mut self,
        key: &str,
        mut movie: Movie,
        into: Option<usize>,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<usize, String> {
        self.vm.begin_update();
        // Retail loads every imported library movie before linking (sub_82E7E8C0); a library's
        // root init actions (its class definitions and registerClass calls) run then.
        for library in movie.import_files.clone() {
            self.init_library(&library, source, natives, 0)?;
        }
        // Init actions (class definitions such as _global.screenmanager) run once, before the
        // first frame, like the HUD runtime does for its movie.
        let initial: Vec<_> = movie
            .characters
            .values()
            .flat_map(|c| &c.frames)
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in initial {
            let code = movie
                .actions
                .get(&offset.to_string())
                .ok_or("APT stage: missing init action block")?
                .clone();
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run(&code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("{key} init action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        if let Some(target) = into {
            self.unload_target(target, natives);
        }
        movie.classes = movie
            .linkage
            .iter()
            .map(|(name, character)| (*character, name))
            .chain(movie.class_links.iter().map(|(character, name)| (*character, name)))
            .filter_map(|(character, name)| Some((character, *self.state.classes.get(name)?)))
            .collect();
        movie.initialize_into(&mut self.vm, into)?;
        let root = movie.root;
        // Retail load completion (82E7EF28) flags a root 0x80, not playing 0x40, so it holds frame 0 until play(): docs/hails-additions/31-retail-menus.md#loaded-root-clips-hold
        if let Some(instance) = movie.instances.get_mut(&root) {
            instance.playing = false;
        }
        self.state.levels.push(Level { key: key.into(), movie, elapsed: 0.0 });
        self.drain(natives)?;
        Ok(root)
    }
    fn init_library(
        &mut self,
        key: &str,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 16 || !self.state.libraries.insert(key.to_string()) {
            return Ok(());
        }
        let Some(library) = source.movie(key) else {
            // Retail leaves the import slot null for a missing source (apt_imports).
            return Ok(());
        };
        for inner in &library.import_files {
            self.init_library(inner, source, natives, depth + 1)?;
        }
        let Some(root) = library.characters.get(&0) else {
            return Ok(());
        };
        let offsets: Vec<_> = root
            .frames
            .iter()
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in offsets {
            let Some(code) = library.actions.get(&offset.to_string()) else {
                continue;
            };
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run(code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("{key} library init action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        Ok(())
    }
    fn unload_target(&mut self, target: usize, natives: &mut dyn StageNatives) {
        let mut host = Bindings { state: &mut self.state, natives };
        host.unload_into(&mut self.vm, target);
    }
    /// Runs queued actions of every movie until all queues are empty.
    pub fn drain(&mut self, natives: &mut dyn StageNatives) -> Result<(), String> {
        for _ in 0..ACTION_LIMIT {
            let next = self.state.levels.iter_mut().find_map(|l| {
                let p = l.movie.next_action()?;
                Some((p, l.movie.actions.get(&p.offset.to_string()).cloned()))
            });
            let Some((Pending { object, offset, event }, code)) = next else {
                self.state.levels.retain(|l| !l.key.ends_with("#unloading"));
                return Ok(());
            };
            if event == CLASS_CONSTRUCTOR {
                let class = self
                    .state
                    .levels
                    .iter()
                    .find_map(|l| l.movie.classes.get(&l.movie.instances.get(&object)?.character))
                    .copied();
                if let Some(class) = class {
                    let mut host = Bindings { state: &mut self.state, natives };
                    self.vm.call_function(class, object, vec![], &mut host)?;
                }
                continue;
            }
            let code = code.ok_or("APT stage: missing action block")?;
            let mut host = Bindings { state: &mut self.state, natives };
            self.vm.run_on(object, &code, &mut host).map_err(|e| {
                let (op, at) = self.vm.last_instruction;
                format!("action {offset}: {e} (op {op:02x} at {at:x})")
            })?;
        }
        Err("APT stage action limit".into())
    }
    /// Script call into the stage (what retail C++ does with `_global.ScreenManager.OpenScreen`
    /// and friends): resolves the dotted path from `_global`, calls it, then runs what it queued.
    pub fn call_path(
        &mut self,
        path: &str,
        args: Vec<Value>,
        natives: &mut dyn StageNatives,
    ) -> Result<Value, String> {
        self.vm.begin_update();
        let path = path.strip_prefix("_global.").unwrap_or(path);
        let (owner, method) = match path.rsplit_once('.') {
            Some((owner, method)) => (owner, method),
            None => ("", path),
        };
        let mut object = self.vm.global;
        for part in owner.split('.').filter(|p| !p.is_empty()) {
            match self.vm.get(object, part) {
                Value::Object(id) => object = id,
                _ => return Err(format!("APT stage: no {part} in {path}")),
            }
        }
        let mut host = Bindings { state: &mut self.state, natives };
        let value = self.vm.call_method(object, method, args, &mut host)?;
        self.drain(natives)?;
        Ok(value)
    }
    /// One engine update of `dt_ms`: queued loads complete, then every movie advances by the
    /// frames its own rate gives.
    pub fn update(
        &mut self,
        dt_ms: f32,
        source: &dyn MovieSource,
        natives: &mut dyn StageNatives,
    ) -> Result<(), String> {
        while let Some(load) = self.state.loads.pop_front() {
            if !self.vm.objects.get(load.target).is_some_and(|o| {
                matches!(&o.kind, ObjectKind::Native(n) if n.starts_with("movie:"))
            }) {
                continue;
            }
            let Some(movie) = source.movie(&load.key) else {
                bevy::log::warn!("APT stage: no movie {} for a loadClip (keys are case sensitive)", load.key);
                self.notify(load.loader, "onLoadError", vec![Value::Object(load.target)], natives)?;
                continue;
            };
            self.notify(load.loader, "onLoadStart", vec![Value::Object(load.target)], natives)?;
            self.start(&load.key, movie, Some(load.target), source, natives)?;
            self.notify(load.loader, "onLoadComplete", vec![Value::Object(load.target)], natives)?;
            self.notify(load.loader, "onLoadInit", vec![Value::Object(load.target)], natives)?;
        }
        self.vm.begin_update();
        let count = self.state.levels.len();
        for i in 0..count {
            let Some(level) = self.state.levels.get_mut(i) else { break };
            let rate = level.movie.milliseconds_per_frame.unwrap_or(DEFAULT_MS_PER_FRAME);
            level.elapsed += dt_ms.max(0.0);
            let mut frames = 0;
            while level.elapsed >= rate && frames < MAX_FRAMES_PER_UPDATE {
                level.elapsed -= rate;
                frames += 1;
            }
            if frames == MAX_FRAMES_PER_UPDATE {
                level.elapsed = level.elapsed.min(rate);
            }
            for _ in 0..frames {
                let Some(level) = self.state.levels.get_mut(i) else { break };
                level.movie.advance(&mut self.vm)?;
                self.drain(natives)?;
            }
        }
        if self.vm.live_objects() >= self.collect_at {
            self.collect()?;
        }
        Ok(())
    }
    /// Objects in use, slots, the next collection point and each level's key and instance count (for logs).
    pub fn object_summary(&self) -> String {
        let levels: Vec<String> = self.state.levels.iter().map(|l| format!("{}:{}", l.key, l.movie.instances.len())).collect();
        format!(
            "in use {} slots {} collect_at {} levels {} [{}]",
            self.vm.live_objects(),
            self.vm.objects.len(),
            self.collect_at,
            levels.len(),
            levels.join(", ")
        )
    }
    /// Frees unreachable objects between updates (NOT RETAIL mechanism: retail refcounts): docs/hails-additions/31-retail-menus.md#stage-object-collection
    pub fn collect(&mut self) -> Result<(), String> {
        let state = &self.state;
        let mut roots = vec![state.loader_prototype, state.object_class];
        roots.extend(state.classes.values());
        // `loadMovie` loads have no loader (usize::MAX).
        roots.extend(state.loads.iter().flat_map(|l| [l.target, l.loader]).filter(|&id| id != usize::MAX));
        for level in &state.levels {
            let m = &level.movie;
            if m.root != usize::MAX {
                roots.push(m.root);
            }
            roots.extend(m.instances.iter().flat_map(|(id, i)| std::iter::once(*id).chain(i.children.values().copied())));
            roots.extend(m.pending.iter().chain(&m.immediate).map(|p| p.object));
            roots.extend(m.classes.values());
            roots.extend(m.script_moved.iter().chain(&m.attached).chain(m.scaled.keys()));
        }
        let free: BTreeSet<usize> = self.vm.collect(roots)?.iter().copied().collect();
        self.state.dynamic.retain(|id, _| !free.contains(id));
        // Twice the survivors, but never past three quarters of the VM's guard: garbage must not fill the slots to the
        // guard before a collection runs.
        self.collect_at = (2 * self.vm.live_objects()).clamp(MIN_COLLECT_AT, crate::apt_vm::MAX_OBJECTS * 3 / 4);
        Ok(())
    }
    /// Placement of the clip a level's root is loaded into, from its ancestors (each an instance of another
    /// level or a script-made clip): matrix, alpha factor, and whether every ancestor is visible.
    fn parent_of(&self, root: usize) -> ([f32; 6], f32, bool) {
        let mut matrix = crate::apt_scene::IDENTITY;
        let mut alpha = 1.0;
        let mut current = self.vm.get(root, "_parent");
        for _ in 0..64 {
            let Value::Object(id) = current else { break };
            if !self.vm.get(id, "_visible").truth() {
                return (matrix, alpha, false);
            }
            let m = self
                .state
                .levels
                .iter()
                .find(|l| l.movie.instances.contains_key(&id))
                .map_or(crate::apt_scene::IDENTITY, |l| crate::apt_movie::clip_matrix(&l.movie, &self.vm, id));
            matrix = crate::apt_scene::compose(m, matrix);
            alpha *= (self.vm.get(id, "_alpha").number() as f32 / 100.).clamp(0., 1.);
            current = self.vm.get(id, "_parent");
        }
        (matrix, alpha, true)
    }
    /// A clip's place in the display tree: the level its top ancestor roots, then the depth of each link from the
    /// top down (timeline depths, script depths from SCRIPT_DEPTH, createEmptyMovieClip depths as script depths).
    fn display_path(&self, object: usize) -> (usize, Vec<i32>) {
        let mut path = Vec::new();
        let mut current = object;
        for _ in 0..64 {
            let Value::Object(parent) = self.vm.get(current, "_parent") else { break };
            let depth = match self.state.dynamic.get(&current) {
                Some((_, depth)) => depth.saturating_add(crate::apt_movie::SCRIPT_DEPTH),
                None => self
                    .state
                    .levels
                    .iter()
                    .find_map(|l| l.movie.instances.get(&parent))
                    .and_then(|p| p.children.iter().find(|(_, c)| **c == current).map(|(k, _)| *k))
                    .unwrap_or(0),
            };
            path.push(depth);
            current = parent;
        }
        path.reverse();
        let top = self.state.levels.iter().position(|l| l.movie.root == current).unwrap_or(usize::MAX);
        (top, path)
    }
    /// Draws every level under its holder clip in display-tree order (NOT RETAIL YET: per-level blocks): docs/hails-additions/31-retail-menus.md#stage-draw-order
    pub fn draw<'s, S: crate::apt_scene::ShapeSource>(
        &'s self,
        scene: impl Fn(&'s str, &'s Movie) -> S,
    ) -> Result<Vec<crate::apt_scene::Draw>, String> {
        let mut out = Vec::new();
        let mut order: Vec<(usize, Vec<i32>, usize)> =
            self.state.levels.iter().enumerate().map(|(i, l)| {
                let (top, path) = self.display_path(l.movie.root);
                (top, path, i)
            }).collect();
        order.sort();
        for level in order.into_iter().map(|(_, _, i)| &self.state.levels[i]) {
            if level.key.ends_with("#unloading") {
                continue;
            }
            let (matrix, alpha, visible) = self.parent_of(level.movie.root);
            if !visible || alpha <= 0.0 {
                continue;
            }
            let shapes = scene(&level.key, &level.movie);
            out.extend(crate::apt_scene::draw_from(
                &level.movie,
                &self.vm,
                &shapes,
                matrix,
                [1., 1., 1., alpha],
                [0.; 4],
            )?);
        }
        Ok(out)
    }
    fn notify(
        &mut self,
        loader: usize,
        event: &str,
        args: Vec<Value>,
        natives: &mut dyn StageNatives,
    ) -> Result<(), String> {
        if loader == usize::MAX {
            return self.drain(natives);
        }
        let Value::Object(list) = self.vm.get(loader, "_listeners") else {
            return Ok(());
        };
        let n = self.vm.get(list, "length").number().max(0.0) as usize;
        for i in 0..n {
            if let Value::Object(listener) = self.vm.get(list, &i.to_string()) {
                if let Value::Object(_) = self.vm.get(listener, event) {
                    let mut host = Bindings { state: &mut self.state, natives };
                    self.vm.call_method(listener, event, args.clone(), &mut host)?;
                }
            }
        }
        self.drain(natives)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_string_and_array_builtins() {
        let mut vm = Vm::new();
        let a = vm.array(vec![Value::Number(1.0), Value::Number(2.0)]).unwrap();
        struct Stub;
        impl Host for Stub {
            fn call(&mut self, _: &mut Vm, _: usize, m: &str, _: Vec<Value>) -> Result<Value, String> {
                Err(format!("host {m}"))
            }
        }
        assert_eq!(vm.call_method(a, "push", vec![Value::Number(3.0)], &mut Stub), Ok(Value::Number(3.0)));
        assert_eq!(vm.call_method(a, "join", vec![Value::Text("-".into())], &mut Stub), Ok(Value::Text("1-2-3".into())));
        let Ok(Value::Object(removed)) = vm.call_method(a, "splice", vec![Value::Number(0.0), Value::Number(1.0)], &mut Stub) else {
            panic!()
        };
        assert_eq!(vm.get(removed, "0"), Value::Number(1.0));
        assert_eq!(vm.get(a, "length"), Value::Number(2.0));
        assert_eq!(vm.call_method(a, "pop", vec![], &mut Stub), Ok(Value::Number(3.0)));
        assert!(vm.call_method(a, "unknown", vec![], &mut Stub).is_err());
    }

    /// The real retail screen manager + core_menu: OpenScreen with CrossBar arguments loads and reports like retail.
    #[test]
    fn retail_screen_manager_opens_a_screen() {
        retail_screen_manager(false);
    }

    /// CloseScreen after many updates: the screen manager's 2-frame root must not loop (loaded roots do not
    /// play, doc 31 "Milestone 5a"), or `new screenmanager(this)` would reset mMovieClipList.
    #[test]
    fn retail_screen_manager_closes_a_screen() {
        retail_screen_manager(true);
    }

    /// Video Calibration (template 91, FE 51): the retail movie loads through the screen manager, reports loaded and
    /// intro complete, and takes SetState for its three pages (settings, contrast, brightness) with no native left
    /// unanswered (it calls none; doc 31 "Video Calibration"). Data-gated like the above.
    #[test]
    fn retail_video_calibration_steps_its_pages() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/options/videocal") {
            eprintln!("skipped: menu set without videocal");
            return;
        }
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("VideoCalibration".into())], &mut game).unwrap();
        let open = vec![
            Value::Text("source/screens/options/videocal.swf".into()),
            Value::Text("screen".into()),
            Value::Text("0".into()),
            Value::Text("VideoCalibration".into()),
        ];
        stage.call_path("_global.ScreenManager.OpenScreen", open, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenLoaded(600)"), "{:?}", game.calls);
        for page in 0..3 {
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text(page.to_string())], &mut game).unwrap();
            for _ in 0..90 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
        }
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenIntroComplete(600)"), "{:?}", game.calls);
        let unanswered: Vec<&String> = stage.unhandled().iter().filter(|c| c.starts_with("Game.")).collect();
        assert!(unanswered.is_empty(), "{unanswered:?}");
        if std::env::var_os("SKATE3_DUMP_DRAWS").is_some() {
            eprintln!("calls: {:?}\nunhandled: {:?}", game.calls, stage.unhandled());
        }
    }

    /// The instant replay screen (template Replay2, FE 12, `replay2/replay_editor2`; b390 / b391): loads through the
    /// screen manager, reports loaded and intro complete, and once shown asks the instant mode natives. Data-gated
    /// like the above (a v11 menu set: `SKATE3_MENU_MOVIES` at a fresh export until the install is refreshed).
    #[test]
    fn retail_replay_editor_opens() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded || !movies.contains_key("source/screens/replay2/replay_editor2") {
            eprintln!("skipped: no menu tables or no replay_editor2");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        game.replay_menu.open = true;
        game.replay_menu.feed = crate::replay_menu_host::ReplayScreenFeed { active: true, instant: true, position: 1.0, playing: false, panel: true, length_seconds: 42, mode: 5, ..Default::default() };
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("Replay2".into())], &mut game).unwrap();
        let open = ["source/screens/replay2/replay_editor2.swf", "screen", "0", "Replay2"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", open, &mut game).unwrap();
        for _ in 0..90 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("calls: {:?}
unhandled: {:?}", game.calls, stage.unhandled());
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenLoaded(600)"), "{:?}", game.calls);
        stage.call_path("_global.ScreenManager.ShowScreen", vec![Value::Text("600".into()), Value::Text("true".into())], &mut game).unwrap();
        for state in [999, 18, 999] {
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text(state.to_string())], &mut game).unwrap();
            for _ in 0..30 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
        }
        // The instant mode natives (b390 table 2) are all asked and answered by the host; no FEReplayMenu call is left.
        for native in ["IsInstantReplayMode", "GetCursorPosition", "GetPlayheadPosition", "GetEncodedLength", "IsEncodedLengthValid", "CanSaveMedia"] {
            assert!(game.replay_menu.seen.iter().any(|m| m == native), "{native} not asked: {:?}", game.replay_menu.seen);
        }
        assert!(!stage.unhandled().iter().any(|c| c.starts_with("FEReplayMenu.")), "{:?}", stage.unhandled());
        // The editor's timeline (A: SetState 21, 9): the CustomTimeline strip is drawn as the host's placeholder.
        game.replay_menu.feed.mode = 6;
        game.replay_menu.feed.instant = false;
        for state in [21, 9] {
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text(state.to_string())], &mut game).unwrap();
            for _ in 0..30 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
        }
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        let strip = draws.iter().find(|d| d.texture == crate::apt_scene::REPLAY_TIMELINE);
        eprintln!("timeline placeholder: {:?}", strip.map(|d| (d.vertices.iter().map(|v| v.position).collect::<Vec<_>>(), d.multiply)));
        assert!(strip.is_some(), "no CustomTimeline placeholder in the editor");
    }

    /// The instant replay help (template ReplayHelp, FE 13, `replay2/replay_help`): loads over the replay screen and
    /// reports loaded and intro complete; no FEReplayMenu / Game native is left unanswered. Data-gated like the above.
    #[test]
    fn retail_replay_help_opens() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded || !movies.contains_key("source/screens/replay2/replay_help") {
            eprintln!("skipped: no menu tables or no replay_help");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.replay_menu.open = true;
        game.replay_menu.feed.instant = true;
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("ReplayHelp".into())], &mut game).unwrap();
        let open = ["source/screens/replay2/replay_help.swf", "screen", "0", "ReplayHelp"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", open, &mut game).unwrap();
        for _ in 0..90 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenIntroComplete(600)"), "{:?}", game.calls);
        assert_eq!(game.replay_menu.seen, vec!["GetHelpMode".to_owned()]);
        assert!(!game.calls.iter().any(|c| c.starts_with("FEReplayMenu.")), "{:?}", game.calls);
        // A turns the page (SetState 0 = NextHelpPage) without leaving anything unanswered.
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("0".into())], &mut game).unwrap();
        for _ in 0..10 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        assert!(!stage.unhandled().iter().any(|c| c.starts_with("FEReplayMenu.") || c.starts_with("Game.")), "{:?}", stage.unhandled());
    }

    /// The retail popup movie (template 17 Sk8Popup) on the "popup" layer: SetState(1) with a shown popup builds its
    /// panel from the Popup natives (doc 31 "Popups"), every layout value finite; A on the first option reaches the
    /// popup host as choice 0. Data-gated like the above; `SKATE3_DUMP_DRAWS=1` prints the draws.
    #[test]
    fn retail_popup_builds_from_the_popup_natives() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded || !movies.contains_key("source/screens/popup/sk8popup") {
            eprintln!("skipped: no menu tables or no sk8popup");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("Sk8Popup".into())], &mut game).unwrap();
        let open = ["source/screens/popup/sk8popup.swf", "popup", "0", "Sk8Popup"].map(|s| Value::Text(s.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", open, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        let rules = skate_core::video_calibration::CalibrationRules::default();
        game.popup.shown = Some(skate_core::fe_popup::PopupRequest {
            description: rules.popup_description.clone(),
            options: rules.popup_buttons.clone(),
            ..Default::default()
        });
        game.popup.input_ready = true;
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("1".into())], &mut game).unwrap();
        stage.call_path("_global.ScreenManager.ShowScreen", vec![Value::Text("600".into()), Value::Text("true".into())], &mut game).unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        let draws = stage.draw(|key, movie| set.scene(key, movie));
        if std::env::var_os("SKATE3_DUMP_DRAWS").is_some() {
            eprintln!("unhandled: {:?}\ncalls: {:?}", stage.unhandled(), game.calls);
        }
        let draws = draws.unwrap_or_else(|e| panic!("{e}; unhandled {:?}", stage.unhandled()));
        if std::env::var_os("SKATE3_DUMP_DRAWS").is_some() {
            for d in &draws {
                eprintln!("draw {:?} mul {:?} v0 {:?}", d.texture, d.multiply, d.vertices.first().map(|v| v.position));
            }
            let level = stage.levels().iter().find(|l| l.key.ends_with("sk8popup")).expect("popup loaded");
            let root = level.movie.root;
            eprintln!("root visible {} alpha {} frame {}", stage.vm.get(root, "_visible").text(), stage.vm.get(root, "_alpha").text(), stage.vm.get(root, "_currentframe").text());
            for (id, inst) in &level.movie.instances {
                let name = inst.placement.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                if ["mScreen", "mcPanel", "body"].iter().any(|n| name.eq_ignore_ascii_case(n)) {
                    eprintln!("{name} {id}: visible {} alpha {} frame {} playing {}", stage.vm.get(*id, "_visible").text(), stage.vm.get(*id, "_alpha").text(), inst.frame, inst.playing);
                }
            }
        }
        assert!(!draws.is_empty(), "the popup draws");
        if std::env::var_os("SKATE3_DUMP_DRAWS").is_some() {
            let level = stage.levels().iter().find(|l| l.key.ends_with("sk8popup")).expect("popup loaded");
            for (id, inst) in &level.movie.instances {
                let name = inst.placement.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                if name.eq_ignore_ascii_case("mMenuPicker") {
                    let f = |k: &str| stage.vm.get(*id, k).text();
                    let arr = |k: &str| match stage.vm.get(*id, k) { Value::Object(o) => stage.vm.get(o, "length").text(), v => v.text() };
                    eprintln!("picker {id}: value {} options {} texts {} types {} gui {} menu {}", f("mCurrentValue"), f("mNumOptions"), arr("mItemTextArray"), arr("mItemTypeArray"), f("mEnableGUI"), f("mMenuID"));
                    let fields: Vec<String> = stage.vm.objects[*id].fields.keys().cloned().collect();
                    eprintln!("fields {fields:?}");
                }
            }
        }
        // A (key index 2, registered like the boot does) picks the first option: Yes.
        stage.call_path("_level0._root.RegisterAptInputEnum", ["AptNext", "2"].map(|s| Value::Text(s.into())).to_vec(), &mut game).unwrap();
        stage.call_path("_global.ScreenManager.UpdateInput", ["600", "2", "0"].map(|s| Value::Text(s.into())).to_vec(), &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        assert_eq!(game.popup.choices, vec![0], "calls {:?} traces {:?}", game.calls, stage.take_traces());
    }

    /// The Game Settings screen (template 90 Options) through the screen manager: SetState(1) after it loads makes
    /// it build its rows from the GameSettings natives (doc 31 "Game Settings screen"). Data-gated like the above;
    /// `SKATE3_DUMP_DRAWS=1` prints the tree and the draws.
    #[test]
    fn retail_options_screen_builds_rows() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        // As the game builds it (retail links and rules, engine handlers).
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.settings.open = true;
        game.settings_titles = vec!["ID_GAMESETTINGS_TITLE".into(); 2];
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("Options".into())], &mut game).unwrap();
        stage
            .call_path(
                "_global.ScreenManager.OpenScreen",
                vec![
                    Value::Text("source/screens/options/options.swf".into()),
                    Value::Text("screen".into()),
                    Value::Text("0".into()),
                    Value::Text("Options".into()),
                ],
                &mut game,
            )
            .unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenLoaded(600)"), "{:?}", game.calls);
        if let Err(e) = stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("1".into())], &mut game) {
            let n = game.calls.len();
            panic!("{e}: {} calls, first {:?} last {:?}", n, &game.calls[..n.min(40)], &game.calls[n.saturating_sub(8)..]);
        }
        for _ in 0..120 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("rows: {:?}", game.settings.rows);
        eprintln!("game calls: {:?}", game.calls);
        eprintln!("unhandled: {:?}", stage.unhandled());
        let level = stage.levels().iter().find(|l| l.key.ends_with("options/options")).expect("options loaded");
        // Every named clip in the options movie (rows are attached under the screen's own clip).
        let names: Vec<String> = level
            .movie
            .instances
            .values()
            .filter_map(|i| i.placement.as_ref().map(|p| p.name.clone()))
            .filter(|n| !n.is_empty())
            .collect();
        eprintln!("options clips: {names:?}");
        // Rows carry their real titles: the subtree's constructors ran inside attachMovie (their default
        // "#TBD TITLE" must not win over CreateOptions' SetTitle).
        let title = |row: &str| -> String {
            let Some(r) = level.movie.attached.iter().find(|c| stage.vm.get(**c, "_name").text() == row) else {
                return String::new();
            };
            let mut v = Value::Object(*r);
            for part in ["mTitle", "mTitle", "TEXTLABEL", "text"] {
                v = match v { Value::Object(o) => stage.vm.get(o, part), other => other };
            }
            v.text()
        };
        assert_eq!(title("mItem0"), "ID_GAMESETTINGS_DIFFICULTY_SETTINGS");
        // A sub-screen rebuild (OnMenuSelect -> screen 2, SetState(1)): DeleteOptions removes the old rows.
        game.settings.screen = 2;
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("1".into())], &mut game).unwrap();
        stage.update(16.0, &movies, &mut game).unwrap();
        let level = stage.levels().iter().find(|l| l.key.ends_with("options/options")).unwrap();
        let rows = level.movie.attached.iter().filter(|c| stage.vm.get(**c, "_name").text().starts_with("mItem")).count();
        let types: Vec<String> = game.settings.rows.iter().map(|id| format!("{id}:{:?}", registry.data().setting(id).map(|s| s.widget))).collect();
        assert_eq!(rows, game.settings.rows.len(), "{types:?}");
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        // Text takes the colour the rows' SetState script sets (`textColor`): the row states' colours appear, not only
        // the fields' authored ones (on 0x70BBFF, off 0x385570, selected value 0x37A5FF).
        let script_colours = [0x385570u32, 0x37A5FF];
        let rgb = |d: &crate::apt_scene::Draw| ((d.multiply[0] * 255.).round() as u32) << 16 | ((d.multiply[1] * 255.).round() as u32) << 8 | (d.multiply[2] * 255.).round() as u32;
        let text_colours: std::collections::BTreeSet<u32> = draws.iter().filter(|d| d.fill == crate::apt_scene::Fill::TextureClamped).map(rgb).collect();
        assert!(script_colours.iter().any(|c| text_colours.contains(c)), "text colours {:06X?}", text_colours);
        if std::env::var("SKATE3_DUMP_DRAWS").is_ok() {
            for d in &draws {
                let xs = d.vertices.iter().map(|v| v.position[0]);
                let ys = d.vertices.iter().map(|v| v.position[1]);
                let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
                eprintln!("DRAW {:?} {:.0},{:.0}..{:.0},{:.0} mul {:?} {}", d.fill, x0, y0, x1, y1, d.multiply, d.texture.rsplit('/').next().unwrap_or(""));
            }
        }
        assert_eq!(game.settings.rows.len(), registry.screen_rows(2, &context).len());
        assert!(names.iter().any(|n| n == "mItem0"), "no row clips: {names:?}");
    }

    /// Challenge Map (template 68) and Skate Profile (FE 61) through the screen manager on real data (data-gated).
    #[test]
    fn retail_skate_profile_screen_opens_and_shows_each_tab() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/profile/offline_skateProfile") {
            eprintln!("skipped: menu set without the skate profile");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (data, loaded) = crate::retail_menus::read(&assets);
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        use skate_core::offline_profile::{StatRow, StatValue};
        game.offline_profile.data.career = skate_core::offline_profile::ProfileOverall { title: "#Career".into(), icon: String::new(), current: 3.0, total: 10.0 };
        game.offline_profile.data.stats = vec![StatRow { id: "flips".into(), label: "#Flips".into(), value: StatValue::Number(42.0) }];
        game.offline_profile.data.personal_bests = vec![StatRow { id: "air".into(), label: "#Best air".into(), value: StatValue::Text("#2.5 m".into()) }];
        game.offline_profile.open();
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("OfflineProfile".into())], &mut game).unwrap();
        for (name, i) in [("AptNext", "2"), ("AptPrevious", "3"), ("AptUp", "12"), ("AptDown", "13")] {
            stage.call_path("_level0._root.RegisterAptInputEnum", vec![Value::Text(name.into()), Value::Text(i.into())], &mut game).unwrap();
        }
        let args = ["source/screens/profile/offline_skateProfile.swf", "screen", "0", "OfflineProfile"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        let step = |stage: &mut Stage, game: &mut crate::crossbar_host::FrontEnd, frames: usize| {
            for frame in 0..frames {
                if let Err(e) = stage.update(16.0, &movies, game) {
                    let traces = stage.take_traces();
                    panic!("frame {frame}: {e}\ncalls: {:?}\ntraces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
                }
            }
        };
        step(&mut stage, &mut game, 120);
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        // Init's page, then each tab as the tab keys post it (sync 30, then the page).
        for state in [5, 30, 10, 30, 20, 30, 5] {
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text(state.to_string())], &mut game).unwrap();
            step(&mut stage, &mut game, 10);
        }
        eprintln!("game calls: {:?}", game.calls);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("Progression.") || c.starts_with("Stats.")).collect();
        assert!(unanswered.is_empty(), "Progression / Stats calls nobody answered: {unanswered:?}");
        let asked = game.offline_profile.asked.lock().unwrap().clone();
        eprintln!("profile calls: {asked:?}");
        for member in ["Progression.GetOverallCareerData", "Stats.GetStatsChunk", "Stats.GetPersonalBestChunk", "Stats.GetCareerProfileSelectedTab"] {
            assert!(asked.iter().any(|a| a == member), "the movie never asked {member}: {asked:?}");
        }
        assert!(stage.draw(|key, movie| set.scene(key, movie)).is_ok());
    }

    /// Extras (FE 18, template 15): the movie lists its rows through Game.Extras_* (SetState 10), greys the rows the
    /// host greys, and A on row 0 asks for the cheat code entry. Data-gated (a menu set with main/extras).
    #[test]
    fn retail_extras_screen_lists_its_rows_and_opens_cheat_entry() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/main/extras") {
            eprintln!("skipped: menu set without extras");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (data, loaded) = crate::retail_menus::read(&assets);
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.extras.open = true;
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("Extras".into())], &mut game).unwrap();
        for (name, i) in [("AptNext", "2"), ("AptPrevious", "3"), ("AptUp", "12"), ("AptDown", "13")] {
            stage.call_path("_level0._root.RegisterAptInputEnum", vec![Value::Text(name.into()), Value::Text(i.into())], &mut game).unwrap();
        }
        let args = ["source/screens/main/extras.swf", "screen", "0", "Extras"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        let step = |stage: &mut Stage, game: &mut crate::crossbar_host::FrontEnd, frames: usize| {
            for frame in 0..frames {
                if let Err(e) = stage.update(16.0, &movies, game) {
                    let traces = stage.take_traces();
                    panic!("frame {frame}: {e}\ncalls: {:?}\ntraces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
                }
            }
        };
        step(&mut stage, &mut game, 120);
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text("10".into())], &mut game).unwrap();
        step(&mut stage, &mut game, 30);
        eprintln!("game calls: {:?}", game.calls);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("Game.Extras_") || c.starts_with("Game.OnMenu")).collect();
        assert!(unanswered.is_empty(), "Extras calls nobody answered: {unanswered:?}");
        // A on the first row (the picker starts on the first enabled row).
        stage.call_path("_global.ScreenManager.UpdateInput", vec![Value::Text(id.clone()), Value::Text("2".into()), Value::Text("0".into())], &mut game).unwrap();
        step(&mut stage, &mut game, 10);
        assert_eq!(game.extras.selected, vec![skate_core::extras::ExtrasAction::CheatEntry], "calls: {:?}", game.calls);
        let draws = stage.draw(|key, movie| set.scene(key, movie));
        assert!(draws.is_ok());
        if std::env::var_os("SKATE3_DUMP_DRAWS").is_some() {
            for d in draws.iter().flatten() {
                let (lo, hi) = d.vertices.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), v| {
                    ([lo[0].min(v.position[0]), lo[1].min(v.position[1])], [hi[0].max(v.position[0]), hi[1].max(v.position[1])])
                });
                eprintln!("draw {:?} {:?} mul {:?} {lo:?}..{hi:?}", d.texture, d.fill, d.multiply);
                if d.texture.contains("controls/panel/") {
                    eprintln!("  uv {:?}", d.vertices.iter().map(|v| (v.position, v.uv)).collect::<Vec<_>>());
                }
            }
        }
    }

    /// SpawnSkater (FE 62, template 107) in Free Play mode: the movie reads the mode and the rows through
    /// `SkaterSpawner` (SetState 1), every call is answered, and A on the first pickable row asks to become that skater.
    /// Data-gated (a set with cas/spawnskater).
    #[test]
    fn retail_spawn_skater_screen_lists_skaters_and_picks_one() {
        use skate_core::spawn_skater::{Action, Entry, Group, Mode, Skater, SpawnSkater};
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/cas/spawnskater") {
            eprintln!("skipped: menu set without spawnskater");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (data, loaded) = crate::retail_menus::read(&assets);
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let entry = |k: &str| Entry { skater: Skater::Character(k.into()), label: format!("#{k}") };
        let groups = [
            Group { id: "you".into(), header: "ID_FREESKATE_YOU_HEADER".into(), modes: vec![Mode::Freeskate], entries: vec![Entry { skater: Skater::Own, label: "#You".into() }] },
            Group { id: "pros".into(), header: "ID_CIC_PRO_SKATERS".into(), modes: vec![Mode::Freeskate], entries: vec![entry("danny_way"), entry("jerry_hsu")] },
        ];
        game.spawn_skater.screen = SpawnSkater::open(Mode::Freeskate, &groups, Vec::new());
        game.spawn_skater.open = true;
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("SpawnSkater".into())], &mut game).unwrap();
        for (name, i) in [("AptNext", "2"), ("AptPrevious", "3"), ("AptMisc", "5"), ("AptUp", "12"), ("AptDown", "13")] {
            stage.call_path("_level0._root.RegisterAptInputEnum", vec![Value::Text(name.into()), Value::Text(i.into())], &mut game).unwrap();
        }
        let args = ["source/screens/cas/spawnskater.swf", "screen", "0", "SpawnSkater"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        let step = |stage: &mut Stage, game: &mut crate::crossbar_host::FrontEnd, frames: usize| {
            for frame in 0..frames {
                if let Err(e) = stage.update(16.0, &movies, game) {
                    let traces = stage.take_traces();
                    panic!("frame {frame}: {e}\ncalls: {:?}\ntraces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
                }
            }
        };
        step(&mut stage, &mut game, 120);
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text("1".into())], &mut game).unwrap();
        step(&mut stage, &mut game, 30);
        eprintln!("game calls: {:?}", game.calls);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("SkaterSpawner.") || c.starts_with("Game.OnMenu")).collect();
        assert!(unanswered.is_empty(), "SpawnSkater calls nobody answered: {unanswered:?}");
        stage.call_path("_global.ScreenManager.UpdateInput", vec![Value::Text(id.clone()), Value::Text("2".into()), Value::Text("0".into())], &mut game).unwrap();
        step(&mut stage, &mut game, 10);
        assert!(game.spawn_skater.actions.contains(&Action::SetPlayer(Skater::Own)), "actions {:?}, calls: {:?}", game.spawn_skater.actions, game.calls);
        assert!(stage.draw(|key, movie| set.scene(key, movie)).is_ok());
    }

    /// The loading screen (FE 52, template 131 TipsLoader): the movie loads, a picked panel's state reaches it and every
    /// `TipsLoader` call is answered. Data-gated (a set with screens/main/tiploader).
    #[test]
    fn retail_loading_screen_shows_a_tip() {
        use skate_core::tips_loader::State;
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/main/tiploader") {
            eprintln!("skipped: menu set without the loading screen");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (data, loaded) = crate::retail_menus::read(&assets);
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.tips_loader.lines = skate_data::tips_loader::TipsLines {
            coach: vec!["ID_TIPS_COACHFRANK_1".into(), "ID_TIPS_COACHFRANK_2".into()],
            hints: vec![("ID_HINTS_TITLE_1".into(), "ID_HINTS_TEXT_1".into())],
        };
        game.tips_loader.open = true;
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("TipsLoader".into())], &mut game).unwrap();
        let args = ["source/screens/main/tiploader.swf", "screen", "0", "TipsLoader"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        let step = |stage: &mut Stage, game: &mut crate::crossbar_host::FrontEnd, frames: usize| {
            for frame in 0..frames {
                if let Err(e) = stage.update(16.0, &movies, game) {
                    let traces = stage.take_traces();
                    panic!("frame {frame}: {e}
calls: {:?}
traces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
                }
            }
        };
        step(&mut stage, &mut game, 120);
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        for _ in 0..2 {
            let available = game.tips_loader.available();
            let mut seed = game.tips_loader.seed;
            let state = game.tips_loader.tips.update(10.0, available, &mut || crate::tips_loader_host::lcg(&mut seed)).unwrap();
            game.tips_loader.seed = seed;
            assert!(matches!(state, State::Populate(_) | State::Outro(_)));
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text(state.code().to_string())], &mut game).unwrap();
            step(&mut stage, &mut game, 30);
        }
        // SetState(1) asks every panel whether it is the picked one and fills that one; SetState(2) runs the outro.
        let traces = stage.take_traces();
        assert!(traces.iter().any(|t| t == "tiploader::UpdateCoachFrank( true true )"), "traces: {traces:?}");
        assert!(traces.iter().any(|t| t == "tiploader::SetState( 2 )"), "traces: {traces:?}");
        assert_eq!(game.tips_loader.asked.iter().filter(|m| *m == "GetCoachFrankText").count(), 1, "asked: {:?}", game.tips_loader.asked);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("TipsLoader.")).collect();
        assert!(unanswered.is_empty(), "TipsLoader calls nobody answered: {unanswered:?}");
        assert!(stage.draw(|key, movie| set.scene(key, movie)).is_ok());
    }

    /// The text viewer (FE 3, template 28 ToSScreen) with our licenses: the movie loads, SetState(1) reads the words
    /// through `tos` (GetNumWords, GetWordRange) and every `tos` call is answered. Data-gated (a set with nucleus_tos).
    #[test]
    fn retail_text_viewer_shows_the_licenses() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/nucleus/nucleus_tos") {
            eprintln!("skipped: menu set without the text viewer");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (data, loaded) = crate::retail_menus::read(&assets);
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.tos.viewer = Some(skate_core::text_viewer::TextViewer::open("ID_EXTRA_LICENSES_TITLE", crate::extras_host::LICENSES, &Default::default()));
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("ToSScreen".into())], &mut game).unwrap();
        let args = ["source/screens/nucleus/nucleus_tos.swf", "screen", "0", "ToSScreen"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        let step = |stage: &mut Stage, game: &mut crate::crossbar_host::FrontEnd, frames: usize| {
            for frame in 0..frames {
                if let Err(e) = stage.update(16.0, &movies, game) {
                    let traces = stage.take_traces();
                    panic!("frame {frame}: {e}\ncalls: {:?}\ntraces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
                }
            }
        };
        step(&mut stage, &mut game, 120);
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id), Value::Text("1".into())], &mut game).unwrap();
        step(&mut stage, &mut game, 30);
        eprintln!("game calls: {:?}", game.calls);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("tos.")).collect();
        assert!(unanswered.is_empty(), "tos calls nobody answered: {unanswered:?}");
        assert!(stage.draw(|key, movie| set.scene(key, movie)).is_ok());
    }

    #[test]
    fn retail_challenge_map_screen_opens_and_picks_a_location() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/map/challengemap") {
            eprintln!("skipped: menu set without the challenge map");
            return;
        }
        let assets = crate::apt_imports::main_checkout().join("assets");
        let (mut data, loaded) = crate::retail_menus::read(&assets);
        let Ok(collections) = skate_data::collections::Collections::load(&assets) else {
            eprintln!("skipped: no collections");
            return;
        };
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        data.map_nodes = skate_data::challenge_map::load(collections.entries(), "progression", &Default::default(), &Default::default(), &|_| None);
        let mut menus = crate::retail_menus::RetailMenus::new(data, true);
        assert!(menus.register_teleports(&[(skate_core::menus::EntryKind::FeLocation, "dist_downtown_aletown".to_owned(), "dist_downtown_aletown".to_owned())]) == 1);
        let registry = menus.registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.challenge_map.open(&registry, &context, "progression");
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("ChallengeMap".into())], &mut game).unwrap();
        for (name, i) in [("AptNext", "2"), ("AptPrevious", "3"), ("AptUp", "12"), ("AptDown", "13")] {
            stage.call_path("_level0._root.RegisterAptInputEnum", vec![Value::Text(name.into()), Value::Text(i.into())], &mut game).unwrap();
        }
        let args = ["source/screens/map/challengemap.swf", "screen", "0", "ChallengeMap"].map(|a| Value::Text(a.into())).to_vec();
        stage.call_path("_global.ScreenManager.OpenScreen", args, &mut game).unwrap();
        for frame in 0..120 {
            if let Err(e) = stage.update(16.0, &movies, &mut game) {
                let traces = stage.take_traces();
                panic!("frame {frame}: {e}\ncalls: {:?}\ntraces (last 40): {:?}", game.calls, &traces[traces.len().saturating_sub(40)..]);
            }
        }
        eprintln!("game calls: {:?}", game.calls);
        let unanswered: Vec<&String> = game.calls.iter().filter(|c| c.starts_with("FEChallengeMap.")).collect();
        assert!(unanswered.is_empty(), "FEChallengeMap calls nobody answered: {unanswered:?}");
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).expect("the movie loaded");
        // The open states, as the stage posts them after the update.
        for state in std::mem::take(&mut game.challenge_map.states) {
            stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text(state.to_string())], &mut game).unwrap();
        }
        game.calls.clear();
        for _ in 0..20 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        let rows = game.challenge_map.state.rows(&registry, &context);
        // The disc's seven location folders first; an install with DLC packs lists their locations after them
        // (the packs' own map_listing rows in private/stock/dlc).
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(&labels[..7.min(labels.len())], ["ID_DISTRICT_DOWNTOWN", "ID_DISTRICT_INDUSTRY", "ID_DISTRICT_UNIVERSITY",
            "ID_LOCATION_SKATESCHOOL", "ID_MAP_FILTER_SKATE_PARK", "ID_CHALLENGE_OTL_MMCP_01_TITLE", "ID_WORLD_BLACKBOXPARK"],
            "the seven location folders");
        // SetState 33 loads the default map into an image slot (b262: mDefaultMapImage.LoadRX2Texture); the
        // ImageRenderComponent quad draws that slot, which resolves to the set's map1024 picture (version 7).
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        let slots: Vec<&str> = draws.iter().filter_map(|d| game.image_slots.texture(&d.texture)).collect();
        eprintln!("image slot draws: {slots:?}");
        assert!(slots.contains(&"image/fe/source/images/map/map1024"), "the default map draws: {slots:?}");
        // The movie's MapRenderComponent (b262) is drawn as the marker placeholder the stage expands.
        assert!(draws.iter().any(|d| d.texture == crate::apt_scene::MAP_MARKERS), "no MapRenderComponent placeholder");
        if set.has_named_image("fe/source/images/map/map1024") {
            assert!(set.image("image/fe/source/images/map/map1024").is_some());
        }
        // A on the first folder (Downtown) through the movie's picker, then A on its first location.
        let mut selected = None;
        for _ in 0..2 {
            stage.call_path("_global.ScreenManager.UpdateInput", vec![Value::Text(id.clone()), Value::Text("2".into()), Value::Text("0".into())], &mut game).unwrap();
            for _ in 0..8 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            // The refresh states a push posts, as the stage flushes them after each key.
            for state in std::mem::take(&mut game.challenge_map.states) {
                stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.clone()), Value::Text(state.to_string())], &mut game).unwrap();
            }
            for _ in 0..8 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            selected = selected.or(game.challenge_map.selected.clone());
        }
        eprintln!("stack {:?} selected {selected:?} calls after keys {:?}", game.challenge_map.state.stack, game.calls);
        assert_eq!(game.challenge_map.state.stack.len(), 1, "A opened the Downtown folder");
        assert!(selected.is_some_and(|e| e.kind == skate_core::menus::EntryKind::FeLocation), "A picked a location");
    }

    /// The Trick Guide (pause menu Learn > Trick Guide, template 29 TrickTutorial, movie tricks/trickguide) through the
    /// screen manager. Data-gated like the above; `SKATE3_DUMP_DRAWS=1` prints the draws.
    #[test]
    fn retail_trick_guide_screen_opens() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/tricks/trickguide") {
            eprintln!("skipped: menu set without the trick guide ({} movies in {}, failures {:?})", movies.len(), root.display(), set.failures);
            return;
        }
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        if !loaded {
            eprintln!("skipped: no menu tables");
            return;
        }
        // The guide from the installed collections and the TU3 image's tables (skipped without them).
        let main = crate::apt_imports::main_checkout();
        let (Ok(image), Ok(collections)) = (
            std::fs::read(main.join(".local/tu3-image/default_82000000_011B0000.bin")),
            skate_data::collections::Collections::load(&main.join("assets")),
        ) else {
            eprintln!("skipped: no TU3 image or collections");
            return;
        };
        let tables = skate_data::menu_tables::extract(&skate_data::xex::XexImage { base_address: 0x8200_0000, image }).unwrap();
        let guide = std::sync::Arc::new(skate_data::trick_guide::load(&collections, &tables.trick_guide));
        let registry = crate::retail_menus::RetailMenus::new(data, true).registry;
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.trick_guide = crate::trick_guide_host::TrickGuideHost::new(guide);
        game.trick_guide.open = true;
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("TrickTutorial".into())], &mut game).unwrap();
        // The Apt keys the screen reads, by key index (boot registration).
        for (name, i) in [("AptNext", "2"), ("AptPrevious", "3"), ("AptUp", "12"), ("AptDown", "13")] {
            stage.call_path("_level0._root.RegisterAptInputEnum", vec![Value::Text(name.into()), Value::Text(i.into())], &mut game).unwrap();
        }
        stage
            .call_path(
                "_global.ScreenManager.OpenScreen",
                vec![
                    Value::Text("source/screens/tricks/trickguide.swf".into()),
                    Value::Text("screen".into()),
                    Value::Text("0".into()),
                    Value::Text("TrickTutorial".into()),
                ],
                &mut game,
            )
            .unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("game calls: {:?}", game.calls);
        eprintln!("unhandled: {:?}", stage.unhandled());
        assert!(game.calls.iter().any(|c| c.starts_with("Game.OnScreenLoaded(")), "{:?}", game.calls);
        for _ in 0..120 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("game calls after: {:?}", game.calls);
        eprintln!("unhandled after: {:?}", stage.unhandled());
        eprintln!("states: {:?}", game.trick_guide.states);
        // Down, then A (select Flip tricks), as the stage feeds keys: UpdateInput to the movie.
        let id = game.calls.iter().find_map(|c| c.strip_prefix("Game.OnScreenLoaded(")).map(|c| c.trim_end_matches(')').to_owned()).unwrap();
        for key in ["13", "12", "2"] {
            stage.call_path("_global.ScreenManager.UpdateInput", vec![Value::Text(id.clone()), Value::Text(key.into()), Value::Text("0".into())], &mut game).unwrap();
            for _ in 0..4 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            eprintln!("after key {key}: calls {:?} states {:?} level {} index {}", game.calls, game.trick_guide.states, game.trick_guide.state.level, game.trick_guide.state.index);
        }
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        eprintln!("draws: {}", draws.len());
        // The panel's button help gets its class through panel > button_menu although the screen imports
        // "Button Item" itself (Movie::class_links): Back shows its B prompt.
        assert!(draws.iter().any(|d| d.texture == "prompt/B"), "no B prompt drawn");
        assert_eq!(game.trick_guide.state.items(&game.trick_guide.guide).len(), 5, "Flip tricks' subcategories listed");
        if std::env::var("SKATE3_DUMP_DRAWS").is_ok() {
            for d in &draws {
                let xs = d.vertices.iter().map(|v| v.position[0]);
                let ys = d.vertices.iter().map(|v| v.position[1]);
                let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
                eprintln!("DRAW {:?} {:.0},{:.0}..{:.0},{:.0} mul {:?} {}", d.fill, x0, y0, x1, y1, d.multiply, d.texture.rsplit('/').next().unwrap_or(""));
            }
        }
        // A (Ollie Flips): its first trick's slots fill the gesture icon. gesture_item's SetGestureType attaches
        // "g_" + the gesture, a symbol the library exports but never places (Movie::link_imports copies it).
        stage.call_path("_global.ScreenManager.UpdateInput", vec![Value::Text(id.clone()), Value::Text("2".into()), Value::Text("0".into())], &mut game).unwrap();
        for _ in 0..8 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        let gesture = stage.levels().iter().find_map(|level| {
            let movie = &level.movie;
            let arrows: BTreeSet<i32> = movie.linkage.iter().filter(|(n, _)| n.starts_with("g_")).map(|(_, c)| *c).collect();
            movie.instances.iter().find_map(|(&id, inst)| {
                (stage.vm.get(id, "_name").text() == "gestureitem" && arrows.contains(&inst.character))
                    .then(|| movie.linkage.iter().find(|(_, c)| **c == inst.character).map(|(n, _)| n.clone()))
                    .flatten()
            })
        });
        eprintln!("gesture icon: {gesture:?}; states {:?}; unhandled {:?}", game.trick_guide.states, stage.unhandled());
        assert!(gesture.is_some(), "no gesture arrow attached to the gesture icon");
    }

    /// Opening and closing the pause menu many times stays inside the VM's object budget: closed screens'
    /// clips are collected between updates (a session froze on the third open, 2026-10-12).
    #[test]
    fn pause_menu_reopens_without_growing() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        let registry = if loaded { skate_core::menus::MenuRegistry::new(data) } else { crate::crossbar_host::test_registry() };
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("CrossBar".into())], &mut game).unwrap();
        let mut counts = Vec::new();
        for _ in 0..8 {
            stage
                .call_path(
                    "_global.ScreenManager.OpenScreen",
                    vec![
                        Value::Text("source/screens/main/core_menu.swf".into()),
                        Value::Text("screen".into()),
                        Value::Text("0".into()),
                        Value::Text("CrossBar".into()),
                    ],
                    &mut game,
                )
                .unwrap();
            for f in 0..60 {
                if let Err(e) = stage.update(16.0, &movies, &mut game) {
                    panic!("{e} at open {} frame {f}: live {} slots {} after {counts:?}", counts.len(), stage.live_objects(), stage.vm.objects.len());
                }
            }
            stage.call_path("_global.ScreenManager.CloseScreen", vec![Value::Text("600".into())], &mut game).unwrap();
            for _ in 0..60 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            stage.collect().unwrap();
            counts.push(stage.live_objects());
        }
        eprintln!("live objects after each open/close: {counts:?}");
        assert!(counts.iter().all(|&n| n == counts[0]), "objects grow with every open: {counts:?}");
    }

    /// The teleporter prompt (action overlay, Teleport state 2) draws its D-pad Up glyph: the movie's Construct sets
    /// `mButtonImage = "DPad_Up"` and button_item2's constructor applies it after its child's construct (b452).
    #[test]
    fn the_teleport_prompt_shows_its_dpad_up_glyph() {
        let Some(prompts) = overlay_prompts(2) else { return };
        assert!(prompts.iter().any(|p| p == "prompt/DPad_Up"), "prompts drawn: {prompts:?}");
    }

    /// The challenge sign-up (Challenge state 1, challenges.rs): the movie sets `DPad_Up_hud` itself.
    #[test]
    fn the_challenge_sign_up_shows_a_dpad_up_glyph() {
        let Some(prompts) = overlay_prompts(1) else { return };
        assert!(prompts.iter().any(|p| p.starts_with("prompt/DPad_Up")), "prompts drawn: {prompts:?}");
    }

    /// The own the spot sign-up (OTS state 5, "Activate Spot"): it runs (its intro and text) without a binding the
    /// stage does not answer.
    #[test]
    fn the_own_the_spot_sign_up_runs() {
        let Some(prompts) = overlay_prompts(5) else { return };
        eprintln!("OTS sign-up prompts {prompts:?}");
    }

    /// The action overlay's prompt textures after `SetScreenState(state)` (None without the movies).
    fn overlay_prompts(state: i32) -> Option<Vec<String>> {
        let root = crate::apt_imports::test_menu_set()?;
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        if !movies.contains_key("source/screens/hud2/actionoverlay2") {
            return None;
        }
        let registry = crate::crossbar_host::test_registry();
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies.get("source/screens/main/core_menu").map(|m| m.text_assets.language.clone()).unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        game.signup_title = Some("Own the spot".into());
        let mut stage = Stage::new(&game).unwrap();
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        for (value, name) in ["Off", "Challenge", "Teleport", "PressStart", "DMOGrab", "OTS", "Disabled"].iter().enumerate() {
            let global = stage.vm.global;
            stage.vm.set(global, format!("kActionOverlayState_{name}"), Value::Number(value as f64)).unwrap();
        }
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("SignUpHUD".into())], &mut game).unwrap();
        stage.call_path("_global.ScreenManager.OpenScreen", vec![Value::Text("source/screens/hud2/actionoverlay2.swf".into()),
            Value::Text("popup".into()), Value::Text("0".into()), Value::Text("SignUpHUD".into())], &mut game).unwrap();
        for _ in 0..30 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text(state.to_string())], &mut game).unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        let prompts: Vec<String> = draws.iter().filter(|d| d.texture.starts_with("prompt/")).map(|d| d.texture.clone()).collect();
        eprintln!("SIGNUP_PROMPT state {state} draws {} prompts {prompts:?} unhandled {:?}", draws.len(), stage.unhandled());
        Some(prompts)
    }

    fn retail_screen_manager(close: bool) {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let set = crate::retail_menu_movies::RetailMenuMovies::read(&root).unwrap();
        let movies = set.movies.clone();
        // The real extracted tables (setup's private/menu-tables.json) when present.
        let (data, loaded) = crate::retail_menus::read(&crate::apt_imports::main_checkout().join("assets"));
        let registry = if loaded {
            skate_core::menus::MenuRegistry::new(data)
        } else {
            crate::crossbar_host::test_registry()
        };
        let context = skate_core::menus::MenuContext::new("FreePlay");
        let language = movies
            .get("source/screens/main/core_menu")
            .map(|m| m.text_assets.language.clone())
            .unwrap_or_default();
        let mut game = crate::crossbar_host::FrontEnd::new(&registry, &context, &language);
        let mut stage = Stage::new(&game).unwrap();
        // Retail _level0 is screens/main.apt (it defines the Register*Enum functions the C++ calls and creates
        // gScreenManager); its loads land on the following updates.
        stage.load_root("source/screens/main", &movies, &mut game).unwrap();
        for _ in 0..4 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("boot levels: {:?}", stage.levels().iter().map(|l| &l.key).collect::<Vec<_>>());
        eprintln!("boot unhandled: {:?}", stage.unhandled());
        // The crossbar's enums, like sub_8261B9C8: RegisterChallengeEnum(name, "%d").
        for (i, name) in ["RefreshMenu", "RefreshOption", "InitialiseMenu", "InitialiseSubOption", "InitSubOptionOnlineTeam", "RefreshDLCBanner"]
            .iter()
            .enumerate()
        {
            stage
                .call_path(
                    "_level0._root.RegisterChallengeEnum",
                    vec![Value::Text(format!("FECrossBarScreenMode_{name}")), Value::Text(i.to_string())],
                    &mut game,
                )
                .unwrap();
        }
        assert_eq!(stage.vm.get(stage.vm.global, "FECrossBarScreenMode_InitialiseMenu"), Value::Text("2".into()));
        let Value::Object(manager) = stage.vm.get(stage.vm.global, "ScreenManager") else {
            panic!("ScreenManager not registered; unhandled: {:?}", stage.unhandled());
        };
        assert_eq!(stage.vm.get(manager, "currentDepth"), Value::Number(600.0));
        stage
            .call_path("_global.ScreenManager.RegisterTemplate", vec![Value::Text("CrossBar".into())], &mut game)
            .unwrap();
        stage
            .call_path(
                "_global.ScreenManager.OpenScreen",
                vec![
                    // Retail arguments for template 69 "CrossBar" (sub_82D09140).
                    Value::Text("source/screens/main/core_menu.swf".into()),
                    Value::Text("screen".into()),
                    Value::Text("0".into()),
                    Value::Text("CrossBar".into()),
                ],
                &mut game,
            )
            .unwrap();
        assert!(stage.levels().iter().all(|l| l.key != "source/screens/main/core_menu"));
        stage.update(16.0, &movies, &mut game).unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("levels: {:?}", stage.levels().iter().map(|l| &l.key).collect::<Vec<_>>());
        eprintln!("game calls: {:?}", game.calls);
        eprintln!("unhandled: {:?}", stage.unhandled());
        assert!(stage.levels().iter().any(|l| l.key == "source/screens/main/core_menu"));
        // The open pause menu draws, and every texture it names resolves in the exported set.
        let draws = stage.draw(|key, movie| set.scene(key, movie)).unwrap();
        let missing: Vec<_> = draws
            .iter()
            // Button prompts (`prompt/<button>`) are the engine's own glyphs (prompt_glyphs.rs), not exported textures.
            .filter(|d| d.fill.textured() && !d.texture.starts_with("prompt/") && set.image(&d.texture).is_none())
            .map(|d| d.texture.clone())
            .collect();
        eprintln!("draws: {} (textured {})", draws.len(), draws.iter().filter(|d| d.fill.textured()).count());
        // mDLC's X (button_item2 constructor, b452) draws before InitializeMenu hides the banner, but like retail it sits
        // above the stage until the animatein slide (root frame 0 places mDLC at y -112, research b467): never on screen.
        for d in draws.iter().filter(|d| d.texture.starts_with("prompt/")) {
            assert!(d.vertices.iter().all(|v| v.position[1] < 0.), "{} drawn on stage before InitializeMenu", d.texture);
        }
        if std::env::var("SKATE3_DUMP_DRAWS").is_ok() {
            // Retail crossbar IntroComplete posts SetState(InitialiseMenu = 2) (8261B9E8).
            stage
                .call_path("_global.ScreenManager.SetScreenState", vec![Value::Text("600".into()), Value::Text("2".into())], &mut game)
                .unwrap();
            for _ in 0..440 {
                stage.update(16.0, &movies, &mut game).unwrap();
            }
            for l in stage.levels() {
                let r = &l.movie.instances[&l.movie.root];
                eprintln!("LEVEL {} frame {} playing {} root _x {} _y {} vis {}", l.key, r.frame, r.playing,
                    stage.vm.get(l.movie.root, "_x").text(), stage.vm.get(l.movie.root, "_y").text(), stage.vm.get(l.movie.root, "_visible").text());
                fn walk(l: &Level, vm: &Vm, id: usize, depth: usize) {
                    let i = &l.movie.instances[&id];
                    let c = &l.movie.characters[&i.character];
                    eprintln!("{:indent$}- {} {} char {} f{}/{} col {:?} vis {} a {} x {} y {} text {:?}", "", c.type_name,
                        i.placement.as_ref().map_or("", |p| p.name.as_str()), i.character, i.frame, c.frames.len(), i.placement.as_ref().map(|p| p.color),
                        vm.get(id, "_visible").text(), vm.get(id, "_alpha").text(), vm.get(id, "_x").text(), vm.get(id, "_y").text(),
                        vm.get(id, "_displayText").text(), indent = depth * 2);
                    if depth < 4 {
                        for child in i.children.values() {
                            walk(l, vm, *child, depth + 1);
                        }
                    }
                }
                if l.key.ends_with("core_menu") {
                    for (_, child) in &r.children {
                        let name = l.movie.instances[child].placement.as_ref().map_or("", |p| p.name.as_str());
                        if ["cat0", "mTitle", "mColumnAnim"].contains(&name) {
                            walk(l, &stage.vm, *child, 1);
                        }
                    }
                }
                for (depth, child) in &r.children {
                    let i = &l.movie.instances[child];
                    eprintln!("  child d{} {} char {} frame {}/{} playing {} _x {} _y {} vis {}", depth,
                        i.placement.as_ref().map_or("", |p| p.name.as_str()), i.character, i.frame,
                        l.movie.characters[&i.character].frames.len(), i.playing,
                        stage.vm.get(*child, "_x").text(), stage.vm.get(*child, "_y").text(), stage.vm.get(*child, "_visible").text());
                }
            }
            for d in stage.draw(|key, movie| set.scene(key, movie)).unwrap() {
                let xs = d.vertices.iter().map(|v| v.position[0]);
                let ys = d.vertices.iter().map(|v| v.position[1]);
                let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
                eprintln!("DRAW {:?} {:.0},{:.0}..{:.0},{:.0} mul {:?} add {:?} {}", d.fill, x0, y0, x1, y1, d.multiply, d.add,
                    d.texture.rsplit('/').next().unwrap_or(""));
            }
        }
        assert!(!draws.is_empty());
        assert!(missing.is_empty(), "missing textures {missing:?}");
        if !close {
            return;
        }
        // Retail CloseScreen (sub_82D09298) passes the AS id as "%d"; the screen plays its outro
        // and reports OutroComplete.
        game.calls.clear();
        stage
            .call_path("_global.ScreenManager.CloseScreen", vec![Value::Text("600".into())], &mut game)
            .unwrap();
        for _ in 0..60 {
            stage.update(16.0, &movies, &mut game).unwrap();
        }
        eprintln!("after close: {:?}", game.calls);
        assert!(game.calls.iter().any(|c| c == "Game.OnScreenOutroComplete(600)"), "{:?}", game.calls);
    }
}
