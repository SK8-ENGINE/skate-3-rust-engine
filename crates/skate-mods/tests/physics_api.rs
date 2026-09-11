//! Host pipeline tests for API 2 physics queries and snapshots (no GLB assets).
use serde_json::{json, Value};
use skate_dynamics::{
    BodyDesc, BodyType, DynamicsWorld, GROUND_BODY_ID, JointMotorDesc, PrismaticJointDesc,
    RevoluteJointDesc, Shape,
};
use skate_mods::{
    with_host, Command, DynamicsHost, Manager, RaycastFilter, RaycastOptions,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

const MOD: &str = "examples.physics-sandbox";
const CRATE: &str = "crate";

fn flat_ground(world: &mut DynamicsWorld) {
    let tris = [
        [[-40., 0., -40.], [40., 0., -40.], [40., 0., 40.]],
        [[-40., 0., -40.], [-40., 0., 40.], [40., 0., 40.]],
    ];
    world.set_ground(tris.into_iter()).unwrap();
}

struct Host<'a> {
    world: &'a mut DynamicsWorld,
    bodies: &'a BTreeMap<(String, String), u64>,
    owner: &'a str,
}

impl DynamicsHost for Host<'_> {
    fn raycast(
        &mut self,
        origin: [f32; 3],
        direction: [f32; 3],
        options: &RaycastOptions,
    ) -> Option<Value> {
        let hit = match options.filter {
            RaycastFilter::Ground => {
                self.world
                    .raycast_ground(origin, direction, options.max_distance)?
            }
            RaycastFilter::All => {
                let excluded: Vec<_> = options
                    .exclude
                    .iter()
                    .filter_map(|key| {
                        self.bodies
                            .get(&(self.owner.to_owned(), key.clone()))
                            .copied()
                    })
                    .collect();
                self.world.raycast_excluding_bodies(
                    origin,
                    direction,
                    options.max_distance,
                    &excluded,
                )?
            }
        };
        let body_key = self
            .bodies
            .iter()
            .find(|((o, _), id)| o.as_str() == self.owner && **id == hit.body)
            .map(|((_, k), _)| k.clone())
            .or_else(|| (hit.body == GROUND_BODY_ID).then(|| "ground".into()));
        Some(json!({
            "body": body_key,
            "point": hit.point,
            "normal": hit.normal,
            "toi": hit.toi,
        }))
    }

    fn velocity_at(&self, _key: &str, _point: [f32; 3]) -> Option<[f32; 3]> {
        Some([0., 0., 0.])
    }

    fn effective_inv_mass(&self, _key: &str, _p: [f32; 3], _d: [f32; 3]) -> Option<f32> {
        Some(1. / 25.)
    }

    fn spring_ray(
        &mut self,
        _key: &str,
        _desc: skate_dynamics::SpringRayDesc,
    ) -> Option<skate_dynamics::SpringRayHit> {
        None
    }

    fn local_ang_accel_impulse(
        &self,
        _key: &str,
        a: [f32; 3],
        dt: f32,
    ) -> Option<[f32; 3]> {
        Some([a[0] * dt, a[1] * dt, a[2] * dt])
    }
}

struct Sim {
    world: DynamicsWorld,
    bodies: BTreeMap<(String, String), u64>,
    joints: BTreeMap<(String, String), u64>,
}

impl Sim {
    fn new() -> Self {
        let mut world = DynamicsWorld::default();
        flat_ground(&mut world);
        Self {
            world,
            bodies: BTreeMap::new(),
            joints: BTreeMap::new(),
        }
    }

    fn apply(&mut self, cmd: Command) -> Result<(), String> {
        match cmd {
            Command::PhysicsSpawn { key, body } => {
                if let Some(old) = self.bodies.remove(&(MOD.to_owned(), key.clone())) {
                    self.world.remove(old);
                }
                let id = self.world.spawn(body)?;
                self.bodies.insert((MOD.to_owned(), key), id);
            }
            Command::PhysicsForce { key, force, point } => {
                let id = *self.bodies.get(&(MOD.to_owned(), key.clone())).ok_or("no body")?;
                if !self.world.apply_force(id, force, point) {
                    return Err("force failed".into());
                }
            }
            Command::PhysicsRemove { key } => {
                let k = (MOD.to_owned(), key);
                if let Some(id) = self.bodies.remove(&k) {
                    self.world.remove(id);
                }
            }
            Command::PhysicsRevolute {
                key,
                body_a,
                body_b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                contacts_enabled,
            } => {
                let a = *self.bodies.get(&(MOD.to_owned(), body_a)).ok_or("no body_a")?;
                let b = *self.bodies.get(&(MOD.to_owned(), body_b)).ok_or("no body_b")?;
                let k = (MOD.to_owned(), key);
                if let Some(old) = self.joints.remove(&k) {
                    self.world.remove_joint(old);
                }
                let jid = self.world.add_revolute_joint(RevoluteJointDesc {
                    body_a: a,
                    body_b: b,
                    anchor_a,
                    anchor_b,
                    axis,
                    limits,
                    contacts_enabled,
                })?;
                self.joints.insert(k, jid);
            }
            Command::PhysicsPrismatic {
                key,
                body_a,
                body_b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                contacts_enabled,
            } => {
                let a = *self.bodies.get(&(MOD.to_owned(), body_a)).ok_or("no body_a")?;
                let b = *self.bodies.get(&(MOD.to_owned(), body_b)).ok_or("no body_b")?;
                let k = (MOD.to_owned(), key);
                if let Some(old) = self.joints.remove(&k) {
                    self.world.remove_joint(old);
                }
                let jid = self.world.add_prismatic_joint(PrismaticJointDesc {
                    body_a: a,
                    body_b: b,
                    anchor_a,
                    anchor_b,
                    axis,
                    limits,
                    contacts_enabled,
                })?;
                self.joints.insert(k, jid);
            }
            Command::PhysicsJointMotor { key, motor } => {
                let j = *self.joints.get(&(MOD.to_owned(), key)).ok_or("no joint")?;
                if !self.world.set_joint_motor(j, motor) {
                    return Err("joint motor failed".into());
                }
            }
            Command::PhysicsJointSpring { key, spring } => {
                let j = *self.joints.get(&(MOD.to_owned(), key)).ok_or("no joint")?;
                if !self.world.set_joint_spring(j, spring) {
                    return Err("joint spring failed".into());
                }
            }
            Command::PhysicsRemoveJoint { key } => {
                let k = (MOD.to_owned(), key);
                if let Some(j) = self.joints.remove(&k) {
                    self.world.remove_joint(j);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn physics_snapshot(&self) -> Value {
        let mut bodies = serde_json::Map::new();
        let mut reverse = BTreeMap::<u64, String>::new();
        for ((o, key), id) in &self.bodies {
            if o != MOD {
                continue;
            }
            reverse.insert(*id, key.clone());
            let snap = self.world.read(*id).unwrap();
            bodies.insert(
                key.clone(),
                json!({
                    "position": snap.position,
                    "rotation": snap.rotation,
                    "linvel": snap.linvel,
                    "angvel": snap.angvel,
                    "force": snap.force,
                    "torque": snap.torque,
                    "mass": snap.mass,
                    "speed": (snap.linvel[0].powi(2)
                        + snap.linvel[1].powi(2)
                        + snap.linvel[2].powi(2))
                    .sqrt(),
                }),
            );
        }
        let label = |id: u64| -> Option<String> {
            if id == GROUND_BODY_ID {
                Some("ground".into())
            } else {
                reverse.get(&id).cloned()
            }
        };
        let owned = |id: u64| reverse.contains_key(&id);
        let touching: Vec<_> = self
            .world
            .active_contact_pairs()
            .into_iter()
            .filter(|(a, b)| owned(*a) || owned(*b))
            .map(|(a, b)| json!({"a": label(a), "b": label(b)}))
            .collect();
        json!({"bodies": bodies, "contacts": [], "touching": touching})
    }

    fn snapshot(&self, physics: Value) -> Value {
        json!({
            "player": {
                "position": [0., 1., 0.],
                "velocity": [0., 0., 0.],
                "heading": 0.,
                "on_board": true,
                "state": 0,
                "category": 0,
                "bailing": false,
            },
            "map": {"name": "test", "generation": 1},
            "tick": 1,
            "keys": {},
            "actions": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "pad": null,
            "paused": false,
            "replay": false,
            "attach": null,
            "physics": physics,
            "network": null,
        })
    }

    fn fixed_tick(&mut self, manager: &mut Manager, keys: Value, dt: f32) -> Vec<Command> {
        let physics = self.physics_snapshot();
        manager.snapshot = self.snapshot(physics);
        if let Some(obj) = manager.snapshot.as_object_mut() {
            obj.insert("keys".into(), keys);
        }
        manager.commands.clear();
        let mut host = Host {
            world: &mut self.world,
            bodies: &self.bodies,
            owner: MOD,
        };
        with_host(&mut host, || {
            manager.call(MOD, "on_fixed_update", json!({"dt": dt}));
        });
        let mut cmds = manager
            .commands
            .drain(..)
            .map(|(_, c)| c)
            .collect::<Vec<_>>();
        cmds.sort_by_key(|command| match command {
            Command::PhysicsRemove { .. } | Command::PhysicsRemoveJoint { .. } => 0,
            Command::PhysicsSpawn { .. } => 1,
            _ => 2,
        });
        self.world.begin_force_frame();
        for cmd in &cmds {
            self.apply(cmd.clone()).expect("apply");
        }
        self.world.step(dt);
        cmds
    }
}

fn manager() -> Manager {
    let prefs = std::env::temp_dir().join(format!("physics-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&prefs);
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples");
    let mut manager = Manager::new(examples, prefs);
    manager.snapshot = json!({
        "player": {
            "position": [0., 1., 0.],
            "velocity": [0., 0., 0.],
            "heading": 0.,
            "on_board": true,
            "state": 0,
            "category": 0,
            "bailing": false,
        },
        "map": {"name": "test", "generation": 1},
        "tick": 0,
        "keys": {},
        "actions": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "pad": null,
        "paused": false,
        "replay": false,
        "attach": null,
        "physics": {"bodies": {}, "contacts": [], "touching": []},
        "network": null,
    });
    manager.scan(true);
    manager
}

fn touching_ground(snapshot: &Value, body_key: &str) -> bool {
    let empty: Vec<Value> = vec![];
    let touching = snapshot["physics"]["touching"].as_array().unwrap_or(&empty);
    touching.iter().any(|t| {
        let a = t["a"].as_str();
        let b = t["b"].as_str();
        (a == Some(body_key) && b == Some("ground")) || (b == Some(body_key) && a == Some("ground"))
    })
}

#[test]
fn sandbox_crate_moves_under_force_and_touches_ground() {
    let mut manager = manager();
    let mut sim = Sim::new();
    let dt = 1. / 60.;

    for (_, cmd) in manager.commands.drain(..) {
        match cmd {
            Command::PhysicsSpawn { .. }
            | Command::PhysicsRemove { .. }
            | Command::PhysicsForce { .. }
            | Command::PhysicsRevolute { .. }
            | Command::PhysicsPrismatic { .. }
            | Command::PhysicsJointMotor { .. }
            | Command::PhysicsJointSpring { .. }
            | Command::PhysicsRemoveJoint { .. } => sim.apply(cmd).expect("on_load physics"),
            _ => {}
        }
    }
    sim.world.begin_force_frame();
    sim.world.step(dt);

    // Force the mod to respawn while the old dynamic body is in the ray path.
    // Lua must use the unified raycast with filter="ground", not hit that body.
    manager.dispatch("on_event", json!({"name": "world_changed"}));
    manager.commands.clear();
    let respawn = sim.fixed_tick(&mut manager, json!({}), dt);
    let spawn_y = respawn.iter().find_map(|command| match command {
        Command::PhysicsSpawn { key, body } if key == CRATE => Some(body.position[1]),
        _ => None,
    });
    assert!(
        spawn_y.is_some_and(|y| (y - 0.55).abs() < 0.01),
        "Lua filtered raycast must place crate on y=0 ground, spawn_y={spawn_y:?}"
    );

    for _ in 0..30 {
        sim.fixed_tick(&mut manager, json!({}), dt);
    }

    let snap = sim.physics_snapshot();
    assert!(
        touching_ground(&json!({"physics": snap}), CRATE),
        "crate must rest on map ground after settle"
    );

    let mut peak = 0f32;
    for _ in 0..120 {
        sim.fixed_tick(&mut manager, json!({"KeyW": true}), dt);
        let id = sim.bodies[&(MOD.to_owned(), CRATE.to_owned())];
        let v = sim.world.read(id).unwrap().linvel;
        peak = peak.max((v[0].powi(2) + v[2].powi(2)).sqrt());
    }
    assert!(
        peak > 0.5,
        "WASD force must move crate on ground (peak horizontal speed {peak} m/s)"
    );

    // Reset submits remove then spawn in one callback. Host ordering must leave
    // exactly one live body mapped to the key.
    let reset = sim.fixed_tick(&mut manager, json!({"KeyR": true}), dt);
    assert!(matches!(reset.first(), Some(Command::PhysicsRemove { key }) if key == CRATE));
    assert!(
        reset.iter().any(|command| {
            matches!(command, Command::PhysicsSpawn { key, .. } if key == CRATE)
        }),
        "reset must respawn the crate: {reset:?}"
    );
    assert!(sim.bodies.contains_key(&(MOD.to_owned(), CRATE.to_owned())));
}

#[test]
fn raycast_all_hits_dynamic_and_ground_filter_hits_terrain() {
    let mut world = DynamicsWorld::default();
    flat_ground(&mut world);
    let crate_id = world
        .spawn(BodyDesc {
            shape: Shape::Box {
                half_extents: [0.5, 0.5, 0.5],
            },
            body_type: BodyType::Dynamic,
            mass: 25.,
            position: [0., 2., 0.],
            friction: 0.5,
            ccd: true,
            ..Default::default()
        })
        .unwrap();
    world.step(1. / 60.);
    let dynamic_hit = world
        .raycast([0., 10., 0.], [0., -1., 0.], 20.)
        .expect("world ray");
    assert_eq!(
        dynamic_hit.body, crate_id,
        "default raycast must interact with dynamic bodies"
    );
    let hit = world
        .raycast_ground([0., 10., 0.], [0., -1., 0.], 20.)
        .expect("terrain ray");
    assert_eq!(hit.body, GROUND_BODY_ID);
    assert!(hit.point[1].abs() < 0.01, "ground at y=0, got {}", hit.point[1]);
}

#[test]
fn wheel_motor_on_ground_moves_chassis() {
    let mut world = DynamicsWorld::default();
    flat_ground(&mut world);
    let chassis = world
        .spawn(BodyDesc {
            shape: Shape::Box {
                half_extents: [0.8, 0.2, 1.2],
            },
            body_type: BodyType::Dynamic,
            mass: 800.,
            position: [0., 0.55, 0.],
            friction: 0.,
            sensor: true,
            ccd: true,
            ..Default::default()
        })
        .unwrap();
    let wheel = world
        .spawn(BodyDesc {
            shape: Shape::Sphere { radius: 0.35 },
            body_type: BodyType::Dynamic,
            mass: 40.,
            position: [0., 0.35, -1.],
            friction: 1.3,
            ccd: true,
            ..Default::default()
        })
        .unwrap();
    let hub = [0., 0.2, -1.];
    let susp = world
        .add_prismatic_joint(PrismaticJointDesc {
            body_a: chassis,
            body_b: wheel,
            anchor_a: hub,
            anchor_b: [0., 0., 0.],
            axis: [0., -1., 0.],
            limits: Some([-0.5, 0.5]),
            contacts_enabled: false,
        })
        .unwrap();
    assert!(world.set_joint_spring(
        susp,
        skate_dynamics::JointSpringDesc {
            target_position: 0.35,
            stiffness: 300.,
            damping: 30.,
            max_force: Some(15_000.),
        },
    ));
    let axle = world
        .add_revolute_joint(RevoluteJointDesc {
            body_a: chassis,
            body_b: wheel,
            anchor_a: hub,
            anchor_b: [0., 0., 0.],
            axis: [-1., 0., 0.],
            limits: None,
            contacts_enabled: false,
        })
        .unwrap();
    for _ in 0..90 {
        world.begin_force_frame();
        world.step(1. / 60.);
    }
    assert!(
        world
            .active_contact_pairs()
            .iter()
            .any(|(a, b)| {
                (*a == wheel && *b == GROUND_BODY_ID) || (*b == wheel && *a == GROUND_BODY_ID)
            }),
        "wheel must touch ground after settle"
    );
    assert!(world.set_joint_motor(
        axle,
        JointMotorDesc::Velocity {
            target_velocity: 20.,
            factor: 80.,
            max_force: Some(8_000.),
        },
    ));
    let mut peak = 0f32;
    for _ in 0..180 {
        world.begin_force_frame();
        world.set_joint_motor(
            axle,
            JointMotorDesc::Velocity {
                target_velocity: 20.,
                factor: 80.,
                max_force: Some(8_000.),
            },
        );
        world.step(1. / 60.);
        let v = world.read(chassis).unwrap().linvel;
        peak = peak.max((v[0].powi(2) + v[2].powi(2)).sqrt());
    }
    assert!(
        peak > 0.5,
        "axle motor + wheel friction must move sensor chassis, peak={peak} m/s"
    );
}
