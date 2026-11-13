//! The HUD minimap's frame (`hud2/hudminimap`, template 53 MiniMap, research b568) on its own APT VM: the round
//! backdrop, glass and north needle; the screen's Show toggles it like retail's component (82665278).
//! Details: docs/hails-additions/31-retail-menus.md#hud-minimap
use crate::{
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
};

pub struct Bindings {
    pub movie: Movie,
}

pub struct Runtime {
    pub vm: Vm,
    pub bindings: Bindings,
    controller: usize,
    shown: bool,
}

impl Host for Bindings {
    fn property_changed(&mut self, vm: &mut Vm, object: usize, key: &str) -> Result<(), String> {
        if ["_xscale", "_yscale", "_rotation", "_width", "_height"].iter().any(|k| key.eq_ignore_ascii_case(k)) {
            self.movie.transform_changed(vm, object, key)?;
        }
        Ok(())
    }
    fn call(&mut self, vm: &mut Vm, object: usize, method: &str, args: Vec<Value>) -> Result<Value, String> {
        if self.movie.method(vm, object, method, &args)? {
            return Ok(Value::Undefined);
        }
        let native = match vm.objects.get(object).map(|o| &o.kind) {
            Some(ObjectKind::Native(name)) => name.as_str(),
            _ => "global",
        };
        match (native, method) {
            ("global", "ASSetPropFlags") => Ok(Value::Undefined),
            // The screen registers with the front end's screen manager; the HUD has none to answer.
            ("ScreenManager", _) => Ok(Value::Undefined),
            _ => Err(format!("Unimplemented minimap HUD binding {native}.{method}")),
        }
    }
}

impl Runtime {
    pub fn load(json: &serde_json::Value) -> Result<Self, String> {
        let mut vm = Vm::new();
        for name in ["MovieClip", "ScreenManager"] {
            let object = vm.object(ObjectKind::Native(name.into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(object))?;
        }
        vm.set(vm.global, "Screen_EdgeOffset", Value::Number(0.0))?;
        let mut bindings = Bindings { movie: Movie::load(json)? };
        let initial: Vec<_> = bindings
            .movie
            .characters
            .values()
            .flat_map(|c| &c.frames)
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in initial {
            let code = bindings.movie.actions[&offset.to_string()].clone();
            vm.run(&code, &mut bindings)?;
        }
        vm.begin_update();
        bindings.movie.initialize(&mut vm)?;
        let mut runtime = Self { vm, bindings, controller: usize::MAX, shown: false };
        runtime.drain()?;
        let Value::Object(controller) = runtime.vm.get(runtime.bindings.movie.root, "screen") else {
            return Err("Minimap HUD controller was not constructed".into());
        };
        runtime.controller = controller;
        Ok(runtime)
    }

    fn drain(&mut self) -> Result<(), String> {
        let mut calls = 0;
        while let Some(crate::apt_movie::Pending { object, offset, .. }) = self.bindings.movie.next_action() {
            calls += 1;
            if calls > 4096 {
                return Err("Minimap HUD frame action limit".into());
            }
            let code = self.bindings.movie.actions.get(&offset.to_string()).ok_or("Missing minimap HUD action block")?.clone();
            self.vm.run_on(object, &code, &mut self.bindings)?;
        }
        Ok(())
    }

    /// The screen's Show on a change only (the component shows / hides it as its rule changes).
    pub fn set_shown(&mut self, shown: bool) -> Result<(), String> {
        if shown == self.shown {
            return Ok(());
        }
        self.shown = shown;
        self.vm.begin_update();
        self.vm.call_method(self.controller, "Show", vec![Value::Bool(shown)], &mut self.bindings)?;
        self.drain()
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Turn the north needle (`mScreen.mNorth`): retail's pointer (82665848) turns with the smoothed heading.
    pub fn set_north(&mut self, degrees: f32) -> Result<(), String> {
        let Value::Object(screen) = self.vm.get(self.controller, "mScreen") else { return Ok(()) };
        let Value::Object(north) = self.vm.get(screen, "mNorth") else { return Ok(()) };
        if self.vm.get(north, "_rotation") == Value::Number(degrees as f64) {
            return Ok(());
        }
        self.vm.set(north, "_rotation", Value::Number(degrees as f64))?;
        self.bindings.property_changed(&mut self.vm, north, "_rotation")
    }

    /// One frame of the movie's timelines.
    pub fn update(&mut self) -> Result<(), String> {
        self.vm.begin_update();
        self.bindings.movie.advance(&mut self.vm)?;
        self.drain()?;
        self.vm.collect(self.bindings.movie.instances.keys().copied())?;
        Ok(())
    }
}

/// The minimap's icon groups (retail MiniMapManager groups), by group id; engine systems and mods fill them. Drawn in
/// the order the groups were first added, as retail's group list (82644BB0 appends, b571).
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct MinimapIcons {
    groups: Vec<(String, Vec<skate_core::minimap::Icon>)>,
    /// Who hides the minimap element (retail's show / hide messages, group E of b574; mods: `sdk.minimap.show`).
    hidden_by: std::collections::BTreeSet<String>,
}

impl MinimapIcons {
    /// Replaces a group's icons; a new group goes after the existing ones.
    pub(crate) fn set(&mut self, group: &str, icons: Vec<skate_core::minimap::Icon>) {
        match self.groups.iter_mut().find(|(g, _)| g == group) {
            Some((_, old)) => *old = icons,
            None => self.groups.push((group.to_owned(), icons)),
        }
    }

    pub(crate) fn remove(&mut self, group: &str) {
        self.groups.retain(|(g, _)| g != group);
    }

    pub(crate) fn set_hidden(&mut self, by: &str, hidden: bool) {
        if hidden {
            self.hidden_by.insert(by.to_owned());
        } else {
            self.hidden_by.remove(by);
        }
    }

    pub(crate) fn hidden_by(&self) -> impl Iterator<Item = &str> {
        self.hidden_by.iter().map(String::as_str)
    }

    /// The minimap element is not hidden by anyone (it still follows `skate_core::minimap::visible`).
    pub(crate) fn element_shown(&self) -> bool {
        self.hidden_by.is_empty()
    }

    /// Every icon, groups in draw order.
    pub(crate) fn icons(&self) -> impl Iterator<Item = &skate_core::minimap::Icon> {
        self.groups.iter().flat_map(|(_, icons)| icons)
    }
}

/// The called skaters' group (FE 62 Call Skater; retail adds AI slots with +0x7A set, 8254E540, b565): `mini_skater` at
/// each called skater, every frame.
pub(crate) fn called_skater_icons(
    population: Option<bevy::prelude::Res<crate::living_world::PopulationState>>,
    npcs: bevy::prelude::Query<(&crate::living_world::npc_skaters::NpcSkater, &bevy::prelude::GlobalTransform)>,
    mut icons: bevy::prelude::ResMut<MinimapIcons>,
) {
    let serials: Vec<u32> = population.as_ref().map(|p| p.world.companions().iter().filter_map(|c| c.serial).collect()).unwrap_or_default();
    let group = npcs
        .iter()
        .filter(|(n, _)| serials.contains(&n.id.serial))
        .map(|(n, at)| skate_core::minimap::Icon {
            id: format!("called_skaters:{}", n.id.serial),
            texture: "mini_skater".into(),
            position: [at.translation().x, at.translation().z],
            angle: 0.0,
            scale: 1.0,
            fade: true,
            max_distance: 0.0,
            colour: [1.0; 4],
        })
        .collect();
    icons.set("called_skaters", group);
}

/// The map's heading (skate_core::minimap::Heading) stepped at the console's HUD rate from the player's motion.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct MinimapHeading {
    pub heading: skate_core::minimap::Heading,
    /// The needle's own eased heading (82665848).
    pub needle: skate_core::minimap::Heading,
    /// The player's ground position (x, z), the map's centre.
    pub player: [f32; 2],
    last: Option<bevy::math::Vec3>,
    carry: f32,
    since: f32,
}

/// Retail's HUD update runs at the console's 30 a second; the smoothing is per update, so it is stepped at that rate.
const HUD_STEP: f32 = 1.0 / 30.0;

pub(crate) fn track_heading(
    time: bevy::prelude::Res<bevy::prelude::Time>,
    player: bevy::prelude::Query<&bevy::prelude::GlobalTransform, bevy::prelude::With<crate::world::PlayerRoot>>,
    camera: bevy::prelude::Query<&bevy::prelude::GlobalTransform, bevy::prelude::With<crate::camera::GameplayCamera>>,
    mut state: bevy::prelude::ResMut<MinimapHeading>,
) {
    let Ok(at) = player.single() else { return };
    let p = at.translation();
    state.player = [p.x, p.z];
    let rules = skate_core::minimap::HeadingRules::default();
    let dt = time.delta_secs();
    state.carry += dt;
    state.since += dt;
    if state.carry < HUD_STEP {
        return;
    }
    let velocity = match state.last {
        Some(l) if state.since > 0.0 => (p - l) / state.since,
        _ => bevy::math::Vec3::ZERO,
    };
    // Slow: the camera's forward (82665388, b571); the skater's facing when there is no gameplay camera.
    let facing = camera.iter().next().unwrap_or(at).forward().as_vec3();
    state.last = Some(p);
    state.since = 0.0;
    while state.carry >= HUD_STEP {
        state.carry -= HUD_STEP;
        state.heading.step([velocity.x, velocity.z], [facing.x, facing.z], &rules);
        let heading = state.heading;
        state.needle.follow(&heading, &rules);
    }
}
