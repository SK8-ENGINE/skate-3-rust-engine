//! Integration tests for the legacy articulated SDK example at sdk/examples/skyline.
//! This is NOT validation of the newer one-body Skyline DRIVE mod.
//!
//! This executes the real Lua callbacks, resolves the actual GLB named-node
//! hulls, applies every command to DynamicsWorld, and steps a real Rapier
//! trimesh ground. It intentionally has no fake "grounded" host.
use serde_json::{json, Value};
use skate_dynamics::{
    BodyDesc, DynamicsWorld, GROUND_BODY_ID, JointMotorDesc, PrismaticJointDesc,
    RevoluteJointDesc, Shape,
};
use skate_mods::{
    convex_points_file, with_host, Command, DynamicsHost, Manager, RaycastFilter,
    RaycastOptions,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MOD: &str = "examples.skyline";
const CHASSIS: &str = "chassis";
const WHEELS: [&str; 4] = ["wheel_rl", "wheel_rr", "wheel_fl", "wheel_fr"];
const CARRIERS: [&str; 4] = [
    "carrier_wheel_rl",
    "carrier_wheel_rr",
    "carrier_wheel_fl",
    "carrier_wheel_fr",
];
const KNUCKLES: [&str; 2] = ["knuckle_wheel_fl", "knuckle_wheel_fr"];

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples/skyline")
}

fn flat_ground(world: &mut DynamicsWorld) {
    let triangles = [
        [[-250., 0., -250.], [250., 0., -250.], [250., 0., 250.]],
        [[-250., 0., -250.], [-250., 0., 250.], [250., 0., 250.]],
    ];
    world.set_ground(triangles.into_iter()).unwrap();
}

fn speed(v: [f32; 3]) -> f32 {
    (v[0].powi(2) + v[1].powi(2) + v[2].powi(2)).sqrt()
}

fn horizontal_speed(v: [f32; 3]) -> f32 {
    (v[0].powi(2) + v[2].powi(2)).sqrt()
}

fn yaw(q: [f32; 4]) -> f32 {
    (2. * (q[3] * q[1] + q[0] * q[2]))
        .atan2(1. - 2. * (q[1] * q[1] + q[2] * q[2]))
}

struct Host<'a> {
    world: &'a mut DynamicsWorld,
    bodies: &'a BTreeMap<(String, String), u64>,
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
                            .get(&(MOD.to_owned(), key.clone()))
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
        let body = if hit.body == GROUND_BODY_ID {
            Some("ground".to_owned())
        } else {
            self.bodies
                .iter()
                .find(|((owner, _), id)| owner == MOD && **id == hit.body)
                .map(|((_, key), _)| key.clone())
        };
        Some(json!({
            "body": body,
            "point": hit.point,
            "normal": hit.normal,
            "toi": hit.toi,
        }))
    }

    fn velocity_at(&self, key: &str, point: [f32; 3]) -> Option<[f32; 3]> {
        let id = *self.bodies.get(&(MOD.to_owned(), key.to_owned()))?;
        self.world.velocity_at(id, point)
    }

    fn effective_inv_mass(
        &self,
        key: &str,
        point: [f32; 3],
        direction: [f32; 3],
    ) -> Option<f32> {
        let id = *self.bodies.get(&(MOD.to_owned(), key.to_owned()))?;
        self.world.effective_inv_mass(id, point, direction)
    }

    fn spring_ray(
        &mut self,
        key: &str,
        desc: skate_dynamics::SpringRayDesc,
    ) -> Option<skate_dynamics::SpringRayHit> {
        let id = *self.bodies.get(&(MOD.to_owned(), key.to_owned()))?;
        self.world.spring_ray(id, desc)
    }

    fn local_ang_accel_impulse(
        &self,
        key: &str,
        acceleration: [f32; 3],
        dt: f32,
    ) -> Option<[f32; 3]> {
        let id = *self.bodies.get(&(MOD.to_owned(), key.to_owned()))?;
        self.world.local_ang_accel_impulse(id, acceleration, dt)
    }
}

struct Sim {
    world: DynamicsWorld,
    bodies: BTreeMap<(String, String), u64>,
    joints: BTreeMap<(String, String), u64>,
    package: PathBuf,
    attached: bool,
    tick: u64,
}

impl Sim {
    fn new() -> Self {
        let mut world = DynamicsWorld::default();
        flat_ground(&mut world);
        Self {
            world,
            bodies: BTreeMap::new(),
            joints: BTreeMap::new(),
            package: package_root(),
            attached: false,
            tick: 0,
        }
    }

    fn resolve_body(&self, mut body: BodyDesc) -> Result<BodyDesc, String> {
        if let Shape::Mesh { path, object } = &body.shape {
            body.shape = Shape::Convex {
                points: convex_points_file(&self.package.join(path), object)?,
            };
        } else if let Shape::Model { path, object, options } = &body.shape {
            body.shape = skate_mods::model_shape_file(&self.package.join(path), object, options)?;
        }
        Ok(body)
    }

    fn body_id(&self, key: &str) -> u64 {
        self.bodies[&(MOD.to_owned(), key.to_owned())]
    }

    fn apply(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::PhysicsSpawn { key, body } => {
                let slot = (MOD.to_owned(), key);
                if let Some(old) = self.bodies.remove(&slot) {
                    self.world.remove(old);
                }
                let body = self.resolve_body(body)?;
                let id = self.world.spawn(body)?;
                self.bodies.insert(slot, id);
            }
            Command::PhysicsRemove { key } => {
                if let Some(id) = self.bodies.remove(&(MOD.to_owned(), key)) {
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
                let slot = (MOD.to_owned(), key);
                if let Some(old) = self.joints.remove(&slot) {
                    self.world.remove_joint(old);
                }
                let id = self.world.add_revolute_joint(RevoluteJointDesc {
                    body_a: self.body_id(&body_a),
                    body_b: self.body_id(&body_b),
                    anchor_a,
                    anchor_b,
                    axis,
                    limits,
                    contacts_enabled,
                })?;
                self.joints.insert(slot, id);
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
                let slot = (MOD.to_owned(), key);
                if let Some(old) = self.joints.remove(&slot) {
                    self.world.remove_joint(old);
                }
                let id = self.world.add_prismatic_joint(PrismaticJointDesc {
                    body_a: self.body_id(&body_a),
                    body_b: self.body_id(&body_b),
                    anchor_a,
                    anchor_b,
                    axis,
                    limits,
                    contacts_enabled,
                })?;
                self.joints.insert(slot, id);
            }
            Command::PhysicsJointMotor { key, motor } => {
                let joint = self.joints[&(MOD.to_owned(), key)];
                if !self.world.set_joint_motor(joint, motor) {
                    return Err("joint motor failed".into());
                }
            }
            Command::PhysicsJointSpring { key, spring } => {
                let joint = self.joints[&(MOD.to_owned(), key)];
                if !self.world.set_joint_spring(joint, spring) {
                    return Err("joint spring failed".into());
                }
            }
            Command::PhysicsRemoveJoint { key } => {
                if let Some(id) = self.joints.remove(&(MOD.to_owned(), key)) {
                    self.world.remove_joint(id);
                }
            }
            Command::PlayerAttach { .. } => self.attached = true,
            Command::PlayerDetach { .. } => self.attached = false,
            Command::PhysicsForce { .. }
            | Command::PhysicsImpulse { .. }
            | Command::PhysicsTorque { .. }
            | Command::PhysicsTorqueImpulse { .. }
            | Command::PhysicsSetLinvel { .. }
            | Command::PhysicsSetAngvel { .. }
            | Command::PhysicsSetPose { .. } => {
                return Err(format!("Skyline emitted prohibited drive command: {command:?}"));
            }
            _ => {}
        }
        Ok(())
    }

    fn physics_snapshot(&self) -> Value {
        let mut bodies = serde_json::Map::new();
        let mut reverse = BTreeMap::new();
        for ((owner, key), id) in &self.bodies {
            if owner != MOD {
                continue;
            }
            reverse.insert(*id, key.clone());
            let body = self.world.read(*id).unwrap();
            bodies.insert(
                key.clone(),
                json!({
                    "position": body.position,
                    "rotation": body.rotation,
                    "linvel": body.linvel,
                    "angvel": body.angvel,
                    "force": body.force,
                    "torque": body.torque,
                    "mass": body.mass,
                    "speed": speed(body.linvel),
                }),
            );
        }
        let label = |id: u64| {
            if id == GROUND_BODY_ID {
                Some("ground".to_owned())
            } else {
                reverse.get(&id).cloned()
            }
        };
        let touching: Vec<_> = self
            .world
            .active_contact_pairs()
            .into_iter()
            .filter(|(a, b)| reverse.contains_key(a) || reverse.contains_key(b))
            .map(|(a, b)| json!({"a": label(a), "b": label(b)}))
            .collect();
        json!({"bodies": bodies, "contacts": [], "touching": touching})
    }

    fn tick(
        &mut self,
        manager: &mut Manager,
        keys: Value,
        pad: Value,
        dt: f32,
    ) -> Vec<Command> {
        self.tick += 1;
        manager.snapshot = json!({
            "player": {
                "position": [0., 0.35, 0.],
                "velocity": [0., 0., 0.],
                "heading": 0.,
                "on_board": !self.attached,
                "state": 0,
                "category": 0,
                "bailing": false,
            },
            "map": {"name": "test", "generation": 1},
            "tick": self.tick,
            "keys": keys,
            "actions": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "pad": pad,
            "paused": false,
            "replay": false,
            "attach": self.attached.then(|| json!({"body": CHASSIS, "owner": MOD})),
            "physics": self.physics_snapshot(),
            "network": {
                "active": false,
                "local_id": "0",
                "is_host": true,
                "states": {},
                "status": "",
            },
        });
        manager.commands.clear();
        let mut host = Host {
            world: &mut self.world,
            bodies: &self.bodies,
        };
        with_host(&mut host, || {
            manager.call(MOD, "on_fixed_update", json!({"dt": dt}));
        });
        let mut commands: Vec<_> = manager.commands.drain(..).map(|(_, command)| command).collect();
        commands.sort_by_key(|command| match command {
            Command::PhysicsRemove { .. } | Command::PhysicsRemoveJoint { .. } => 0,
            Command::PhysicsSpawn { .. } => 1,
            _ => 2,
        });
        self.world.begin_force_frame();
        for command in &commands {
            self.apply(command.clone()).expect("apply command");
        }
        self.world.step(dt);
        commands
    }

    fn touches_ground(&self, key: &str) -> bool {
        let body = self.body_id(key);
        self.world.active_contact_pairs().iter().any(|(a, b)| {
            (*a == body && *b == GROUND_BODY_ID) || (*b == body && *a == GROUND_BODY_ID)
        })
    }
}

fn manager() -> Manager {
    let prefs = std::env::temp_dir().join(format!("skyline-physics-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&prefs);
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples");
    let mut manager = Manager::new(examples, prefs);
    manager.snapshot = json!({
        "player": {
            "position": [0., 0.35, 0.],
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
    assert!(
        manager.packages.get(MOD).is_some_and(|package| package.running()),
        "Skyline package must load: {:?}",
        manager.packages.get(MOD).and_then(|package| package.error.clone())
    );
    manager
}

fn neutral_pad() -> Value {
    json!({"buttons": 0, "triggers": [0., 0.], "left": [0., 0.], "right": [0., 0.]})
}

fn launch_pad(sim: &Sim, gear: &mut usize, rb_held: &mut bool) -> Value {
    let ratios = [3.827, 2.360, 1.685, 1.312, 1.000, 0.793];
    let wheel_omega = WHEELS
        .iter()
        .map(|key| {
            let body = sim.world.read(sim.body_id(key)).unwrap();
            let [x, y, z, w] = body.rotation;
            let axle = [
                1. - 2. * (y * y + z * z),
                2. * (x * y + w * z),
                2. * (x * z - w * y),
            ];
            (body.angvel[0] * axle[0]
                + body.angvel[1] * axle[1]
                + body.angvel[2] * axle[2])
                .abs()
        })
        .sum::<f32>()
        / WHEELS.len() as f32;
    let rpm = wheel_omega * 60. / std::f32::consts::TAU * ratios[*gear - 1] * 3.545;
    let mut buttons = 0;
    if *rb_held {
        *rb_held = false;
    } else if *gear < 5 && rpm >= 7_400. {
        buttons = 0x0200;
        *gear += 1;
        *rb_held = true;
    }
    json!({
        "buttons": buttons,
        "triggers": [0., 1.],
        "left": [0., 0.],
        "right": [0., 0.]
    })
}

fn spawn(sim: &mut Sim, manager: &mut Manager, dt: f32) -> Vec<Command> {
    let first = sim.tick(manager, json!({"F10": true}), neutral_pad(), dt);
    assert!(
        !first.iter().any(|command| matches!(command, Command::PhysicsSpawn { .. })),
        "respawn must teardown before next-tick placement queries"
    );
    sim.tick(manager, json!({}), neutral_pad(), dt)
}

fn assert_real_glb_hulls(path: &Path) {
    for wheel in WHEELS {
        let points = convex_points_file(path, wheel).expect("wheel GLB hull");
        let min = std::array::from_fn::<_, 3, _>(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MAX, f32::min)
        });
        let max = std::array::from_fn::<_, 3, _>(|axis| {
            points
                .iter()
                .map(|point| point[axis])
                .fold(f32::MIN, f32::max)
        });
        let center = std::array::from_fn::<_, 3, _>(|axis| (min[axis] + max[axis]) * 0.5);
        let extent = std::array::from_fn::<_, 3, _>(|axis| max[axis] - min[axis]);
        assert!(
            center.iter().all(|value| value.abs() < 0.03),
            "{wheel} collider must be body-local, center={center:?}"
        );
        assert!(
            extent[0] > 0.25 && extent[1] > 0.6 && extent[2] > 0.6,
            "{wheel} must use authored tire hull, extent={extent:?}"
        );
    }
}

#[test]
fn skyline_every_component_is_real_and_drives_through_ground_contact() {
    let package = package_root();
    assert!(package.join("skyline.glb").is_file(), "Skyline GLB missing");
    assert_real_glb_hulls(&package.join("skyline.glb"));

    let mut manager = manager();
    let mut sim = Sim::new();
    let dt = 1. / 120.;
    let spawn_commands = spawn(&mut sim, &mut manager, dt);

    let body_spawns: Vec<_> = spawn_commands
        .iter()
        .filter_map(|command| match command {
            Command::PhysicsSpawn { key, body } => Some((key.as_str(), &body.shape, body.position)),
            _ => None,
        })
        .collect();
    assert_eq!(body_spawns.len(), 11, "chassis + carriers + knuckles + wheels");
    assert_eq!(
        spawn_commands
            .iter()
            .filter(|command| matches!(command, Command::PhysicsPrismatic { .. }))
            .count(),
        4,
        "one physical suspension slider per wheel"
    );
    assert_eq!(
        spawn_commands
            .iter()
            .filter(|command| matches!(command, Command::PhysicsJointSpring { .. }))
            .count(),
        4,
        "one physical spring per suspension"
    );
    assert_eq!(
        spawn_commands
            .iter()
            .filter(|command| matches!(command, Command::PhysicsRevolute { .. }))
            .count(),
        6,
        "four axle hinges + two steering hinges"
    );
    for wheel in WHEELS {
        assert!(
            body_spawns.iter().any(|(key, shape, position)| {
                let smooth_tire = match shape {
                    Shape::Convex { points } => {
                        points.len() == 128
                            && points.iter().all(|point| {
                                (point[0].abs() - 0.146).abs() < 0.001
                                    && ((point[1] * point[1] + point[2] * point[2]).sqrt() - 0.34)
                                        .abs()
                                        < 0.001
                            })
                    }
                    _ => false,
                };
                *key == wheel && smooth_tire && (position[1] - 0.34).abs() < 0.02
            }),
            "{wheel} must use a smooth GLB-dimensioned tire proxy directly on queried y=0 terrain"
        );
    }
    assert!(spawn_commands.iter().any(|command| {
        matches!(command, Command::GraphicsMesh { path, body, .. }
            if path == "skyline.glb" && body.as_deref() == Some(CHASSIS))
    }));
    assert_eq!(sim.bodies.len(), 11);
    assert_eq!(sim.joints.len(), 10);
    for key in KNUCKLES {
        assert!(sim.bodies.contains_key(&(MOD.to_owned(), key.to_owned())));
    }

    for _ in 0..240 {
        sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    }
    for wheel in WHEELS {
        assert!(
            sim.touches_ground(wheel),
            "{wheel} must touch the actual Rapier ground trimesh after settling"
        );
    }
    assert!(
        !sim.touches_ground(CHASSIS),
        "chassis has real collision but must have wheel-supported clearance"
    );
    for (wheel, carrier) in WHEELS.into_iter().zip(CARRIERS) {
        let wheel_position = sim.world.read(sim.body_id(wheel)).unwrap().position;
        let carrier_position = sim.world.read(sim.body_id(carrier)).unwrap().position;
        let separation = speed([
            wheel_position[0] - carrier_position[0],
            wheel_position[1] - carrier_position[1],
            wheel_position[2] - carrier_position[2],
        ]);
        assert!(
            separation < 0.05,
            "{wheel} axle must remain attached to {carrier}, separation={separation}"
        );
    }

    for _ in 0..70 {
        sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    }
    let enter = sim.tick(&mut manager, json!({"KeyE": true}), neutral_pad(), dt);
    assert!(
        enter.iter().any(|command| matches!(
            command,
            Command::PlayerAttach { body, .. } if body == CHASSIS
        )),
        "E must attach to the physical chassis"
    );
    sim.tick(&mut manager, json!({}), neutral_pad(), dt);

    let mut peak_speed: f32 = 0.;
    let mut peak_rear_spin: f32 = 0.;
    let mut peak_center_steer: f32 = 0.;
    let mut grounded_frames = 0;
    let drive_start = sim.world.read(sim.body_id(CHASSIS)).unwrap();
    let mut sixty_time = None;
    let mut launch_gear = 1;
    let mut rb_held = false;
    for frame in 0..360 {
        let launch = launch_pad(&sim, &mut launch_gear, &mut rb_held);
        let commands = sim.tick(&mut manager, json!({}), launch, dt);
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                Command::PhysicsForce { .. }
                    | Command::PhysicsImpulse { .. }
                    | Command::PhysicsTorque { .. }
                    | Command::PhysicsTorqueImpulse { .. }
                    | Command::PhysicsSetLinvel { .. }
                    | Command::PhysicsSetAngvel { .. }
                    | Command::PhysicsSetPose { .. }
            )),
            "drive may only submit joint motors: {commands:?}"
        );
        let drive_motors = commands
            .iter()
            .filter(|command| matches!(
                command,
                Command::PhysicsJointMotor {
                    key,
                    motor: JointMotorDesc::Velocity {
                        target_velocity,
                        max_force: Some(max_force),
                        ..
                    },
                } if key.starts_with("axle_wheel_")
                    && *target_velocity > 1.
                    && *max_force > 100.
            ))
            .count();
        assert!(
            drive_motors == 0 || drive_motors == 4,
            "R34 GT-R AWD must power all four axles together or open the clutch during shifts"
        );
        let chassis = sim.world.read(sim.body_id(CHASSIS)).unwrap();
        peak_speed = peak_speed.max(horizontal_speed(chassis.linvel));
        if sixty_time.is_none() && horizontal_speed(chassis.linvel) >= 26.8224 {
            sixty_time = Some((frame + 1) as f32 * dt);
        }
        let rear = sim.world.read(sim.body_id("wheel_rl")).unwrap();
        peak_rear_spin = peak_rear_spin.max(rear.angvel[0].abs());
        for key in ["steering_wheel_fl", "steering_wheel_fr"] {
            let joint = sim.joints[&(MOD.to_owned(), key.to_owned())];
            peak_center_steer =
                peak_center_steer.max(sim.world.revolute_angle(joint).unwrap().abs());
        }
        if WHEELS.iter().all(|wheel| sim.touches_ground(wheel)) {
            grounded_frames += 1;
        }
    }
    let drive_end = sim.world.read(sim.body_id(CHASSIS)).unwrap();
    assert!(
        peak_rear_spin > 5.,
        "physical axle motor must spin the GLB wheel, peak={peak_rear_spin}"
    );
    assert!(
        peak_speed > 2.,
        "wheel-ground friction must accelerate chassis, peak={peak_speed} m/s"
    );
    assert!(
        grounded_frames > 120,
        "all four tires must stay grounded for substantial drive time; frames={grounded_frames}"
    );
    let forward_distance = drive_end.position[2] - drive_start.position[2];
    let lateral_distance = (drive_end.position[0] - drive_start.position[0]).abs();
    assert!(
        forward_distance > 2. && lateral_distance < forward_distance * 0.40,
        "centered physical steering must track forward: forward={forward_distance}, lateral={lateral_distance}, peak steer={peak_center_steer}"
    );
    for frame in 360..1200 {
        if sixty_time.is_some() {
            break;
        }
        let launch = launch_pad(&sim, &mut launch_gear, &mut rb_held);
        sim.tick(&mut manager, json!({}), launch, dt);
        let chassis = sim.world.read(sim.body_id(CHASSIS)).unwrap();
        let current_speed = horizontal_speed(chassis.linvel);
        peak_speed = peak_speed.max(current_speed);
        if current_speed >= 26.8224 {
            sixty_time = Some((frame + 1) as f32 * dt);
        }
    }
    let final_speed = horizontal_speed(sim.world.read(sim.body_id(CHASSIS)).unwrap().linvel);
    let sixty_time = sixty_time.unwrap_or_else(|| {
        panic!(
            "Skyline must reach 60 mph within ten seconds; peak={peak_speed}, final={final_speed}, gear={launch_gear}, chassis={:?}",
            sim.world.read(sim.body_id(CHASSIS)).unwrap()
        )
    });
    assert_eq!(launch_gear, 2, "0-60 launch must use RB once to reach second gear");
    assert!(
        (4.5..=5.3).contains(&sixty_time),
        "stock R34 GT-R target is 4.9 s 0-60 mph; measured {sixty_time:.3} s"
    );

    let speed_before_brake =
        horizontal_speed(sim.world.read(sim.body_id(CHASSIS)).unwrap().linvel);
    let brake_pad = json!({
        "buttons": 0x2000,
        "triggers": [0., 0.],
        "left": [0., 0.],
        "right": [0., 0.]
    });
    for _ in 0..240 {
        sim.tick(&mut manager, json!({}), brake_pad.clone(), dt);
    }
    let speed_after_brake =
        horizontal_speed(sim.world.read(sim.body_id(CHASSIS)).unwrap().linvel);
    assert!(
        speed_after_brake < speed_before_brake * 0.65,
        "B rear handbrake must slow chassis: before={speed_before_brake}, after={speed_after_brake}"
    );

    let steer_pad = json!({
        "buttons": 0,
        "triggers": [0., 0.],
        "left": [0.8, 0.],
        "right": [0., 0.]
    });
    let mut peak_steer_angle: f32 = 0.;
    let mut active_steer_commands = 0;
    for _ in 0..180 {
        let commands = sim.tick(&mut manager, json!({}), steer_pad.clone(), dt);
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(
                    command,
                    Command::PhysicsJointMotor {
                        key,
                        motor: JointMotorDesc::Velocity {
                            ..
                        },
                    } if key.starts_with("steering_")
                ))
                .count(),
            2,
            "stick steering must command both physical steering servos"
        );
        active_steer_commands += commands
            .iter()
            .filter(|command| matches!(
                command,
                Command::PhysicsJointMotor {
                    key,
                    motor: JointMotorDesc::Velocity { target_velocity, .. },
                } if key.starts_with("steering_") && target_velocity.abs() > 0.2
            ))
            .count();
        let joint = sim.joints[&(MOD.to_owned(), "steering_wheel_fl".to_owned())];
        peak_steer_angle = peak_steer_angle.max(sim.world.revolute_angle(joint).unwrap().abs());
    }
    assert!(
        (0.05..=0.70).contains(&peak_steer_angle),
        "steering hinge must move within its physical limit, peak={peak_steer_angle}"
    );
    assert!(active_steer_commands > 10);

    let before_turn = sim.world.read(sim.body_id(CHASSIS)).unwrap();
    let turning_pad = json!({
        "buttons": 0,
        "triggers": [0., 0.65],
        "left": [0.8, 0.],
        "right": [0., 0.]
    });
    for _ in 0..180 {
        sim.tick(&mut manager, json!({}), turning_pad.clone(), dt);
    }
    let after_turn = sim.world.read(sim.body_id(CHASSIS)).unwrap();
    let chassis_yaw_change = (yaw(after_turn.rotation) - yaw(before_turn.rotation)).abs();
    assert!(
        chassis_yaw_change > 0.02,
        "steered front tire contacts must turn chassis, yaw change={chassis_yaw_change}"
    );

    for _ in 0..6 {
        sim.tick(
            &mut manager,
            json!({}),
            json!({"buttons": 0x0100, "triggers": [0., 0.], "left": [0., 0.], "right": [0., 0.]}),
            dt,
        );
        sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    }
    for _ in 0..24 {
        sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    }
    let reverse = sim.tick(
        &mut manager,
        json!({}),
        json!({"buttons": 0, "triggers": [0., 1.], "left": [0., 0.], "right": [0., 0.]}),
        dt,
    );
    assert_eq!(
        reverse
            .iter()
            .filter(|command| matches!(
                command,
                Command::PhysicsJointMotor {
                    key,
                    motor: JointMotorDesc::Velocity { target_velocity, .. },
                } if key.starts_with("axle_wheel_") && *target_velocity < 0.
            ))
            .count(),
        4,
        "six LB downshifts from fifth must select reverse on all AWD axles"
    );

    let coast = sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    assert_eq!(
        coast
            .iter()
            .filter(|command| matches!(
                command,
                Command::PhysicsJointMotor {
                    key,
                    motor: JointMotorDesc::Velocity {
                        factor,
                        max_force: Some(max_force),
                        ..
                    },
                } if key.starts_with("axle_wheel_") && *factor == 0. && *max_force == 0.
            ))
            .count(),
        4,
        "coast must release all four axle motors"
    );

    let teardown = sim.tick(
        &mut manager,
        json!({}),
        json!({"buttons": 0x0080, "triggers": [0., 0.], "left": [0., 0.], "right": [0., 0.]}),
        dt,
    );
    assert_eq!(
        teardown
            .iter()
            .filter(|command| matches!(command, Command::PhysicsRemove { .. }))
            .count(),
        11
    );
    assert_eq!(
        teardown
            .iter()
            .filter(|command| matches!(command, Command::PhysicsRemoveJoint { .. }))
            .count(),
        10
    );
    assert!(sim.bodies.is_empty());
    assert!(sim.joints.is_empty());
    let replacement = sim.tick(&mut manager, json!({}), neutral_pad(), dt);
    assert_eq!(
        replacement
            .iter()
            .filter(|command| matches!(command, Command::PhysicsSpawn { .. }))
            .count(),
        11
    );
}
