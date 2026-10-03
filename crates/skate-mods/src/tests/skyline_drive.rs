//! Current installed DRIVE mod, real Lua, cooked render-model colliders and Rapier.
//! This is a physics/API regression check, not an FPS or WAN latency benchmark.
use crate::{Command, DynamicsHost, RaycastFilter, RaycastOptions, vm::Vm};
use serde_json::{Value, json};
use skate_dynamics::{DynamicsWorld, Shape, definition_codec, solid::sweep_sphere};
use std::{collections::BTreeMap, path::PathBuf};

const MOD: &str = "examples.skyline";
const DT: f32 = 1. / 120.;

struct Host<'a> { world: &'a mut DynamicsWorld, car: Option<u64> }
impl DynamicsHost for Host<'_> {
    fn raycast(&mut self, origin:[f32;3], direction:[f32;3], options:&RaycastOptions)->Option<Value> {
        let hit=match options.filter {
            RaycastFilter::Ground=>self.world.raycast_ground(origin,direction,options.max_distance),
            RaycastFilter::All=>self.world.raycast_excluding_bodies(origin,direction,options.max_distance,
                &self.car.filter(|_|options.exclude.iter().any(|k|k=="chassis")).into_iter().collect::<Vec<_>>()),
        }?;
        Some(json!({"body":if Some(hit.body)==self.car {"chassis"} else {"ground"},
            "point":hit.point,"normal":hit.normal,"toi":hit.toi}))
    }
    fn velocity_at(&self,key:&str,point:[f32;3])->Option<[f32;3]> {
        if key=="ground" {Some([0.;3])} else {self.world.velocity_at(self.car?,point)}
    }
    fn effective_inv_mass(&self,_:&str,p:[f32;3],d:[f32;3])->Option<f32> {
        self.world.effective_inv_mass(self.car?,p,d)
    }
    fn spring_ray(&mut self,_:&str,d:skate_dynamics::SpringRayDesc)->Option<skate_dynamics::SpringRayHit> {
        self.world.spring_ray(self.car?,d)
    }
    fn local_ang_accel_impulse(&self,_:&str,a:[f32;3],dt:f32)->Option<[f32;3]> {
        self.world.local_ang_accel_impulse(self.car?,a,dt)
    }
}

struct Drive {
    vm:Vm, world:DynamicsWorld, root:PathBuf, car:Option<u64>,
    receipt:Value, tick:u64, forces:usize,
}
impl Drive {
    fn new() -> Self {
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods/Skyline_Drive_Mod");
        let manifest=crate::validate_package(&root).expect("installed Skyline package");
        let settings=manifest.settings.iter().map(|(k,v)|(k.clone(),v.default.clone())).collect::<BTreeMap<_,_>>();
        let vm=Vm::new(&root,&manifest,&settings,&Value::Null).unwrap();
        let mut world=DynamicsWorld::default();
        world.set_ground([
            [[-500.,0.,-500.],[500.,0.,-500.],[500.,0.,500.]],
            [[-500.,0.,-500.],[500.,0.,500.],[-500.,0.,500.]],
        ].into_iter()).unwrap();
        let mut drive=Self {vm,world,root,car:None,receipt:Value::Null,tick:0,forces:0};
        drive.call("on_load",json!({}),json!({}),false);
        drive
    }
    fn snapshot(&self,keys:Value,occupied:bool)->Value {
        let mut bodies=json!({});
        if let Some(id)=self.car {
            let b=self.world.read(id).unwrap();
            bodies["chassis"]=json!({"position":b.position,"rotation":b.rotation,
                "linvel":b.linvel,"angvel":b.angvel,"mass":b.mass,"force":b.force,"torque":b.torque});
        }
        json!({"tick":self.tick,"keys":keys,"paused":false,"replay":false,
            "player":{"position":[0.,0.35,0.],"heading":0.,"velocity":[0.,0.,0.]},
            "map":{"generation":1},"pad":{"buttons":0,"triggers":[0.,if occupied {1.} else {0.}],"left":[0.,0.],"right":[0.,0.]},
            "physics":{"bodies":bodies,"contacts":[],"touching":[]},
            "attach":if occupied {json!({"owner":MOD,"body":"chassis"})} else {Value::Null},
            "command_results":{MOD:{"spawn_chassis":self.receipt}},
            "network":{"active":true,"is_host":true,"local_id":"1","players":["1","2"],"states":{},"status":"test"}})
    }
    fn call(&mut self,name:&str,payload:Value,keys:Value,occupied:bool) {
        let snapshot=self.snapshot(keys,occupied);
        let mut host=Host {world:&mut self.world,car:self.car};
        let commands=crate::with_host(&mut host,||self.vm.call(name,payload,&snapshot)).unwrap();
        for command in commands {assert!(command.validate());self.apply(command);}
    }
    fn apply(&mut self,command:Command) {
        match command {
            Command::Request{key,token,command}=>{
                assert_eq!(key,"spawn_chassis");self.apply(*command);
                self.receipt=json!({"ok":true,"token":token,"tick":self.tick});
            }
            Command::PhysicsSpawn{key,mut body}=>{
                assert_eq!(key,"chassis");
                if let Shape::Model{path,object,options}=&body.shape {
                    body.shape=crate::model_shape_file(&self.root.join(path),object,options).unwrap();
                }
                self.car=Some(self.world.spawn(body).unwrap());
            }
            Command::PhysicsRemove{..}=>{if let Some(id)=self.car.take(){self.world.remove(id);}},
            Command::PhysicsForce{force,point,..}=>{
                assert!(self.world.apply_force(self.car.unwrap(),force,point));self.forces+=1;
            }
            Command::PhysicsTorque{torque,..}=>{assert!(self.world.apply_torque(self.car.unwrap(),torque));},
            Command::PhysicsImpulse{impulse,point,..}=>{assert!(self.world.apply_impulse(self.car.unwrap(),impulse,point));},
            Command::PhysicsTorqueImpulse{torque,..}=>{assert!(self.world.apply_torque_impulse(self.car.unwrap(),torque));},
            Command::PhysicsDebug{..}=>{},
            // Rendering/audio/UI commands do not participate in this physics test.
            other=>assert!(!format!("{other:?}").starts_with("Physics"),"unhandled physics command: {other:?}"),
        }
    }
    fn step(&mut self,keys:Value,occupied:bool,renders:usize) {
        self.tick+=1;
        for _ in 0..renders {
            self.call("on_ui_update",json!({"dt":DT/renders as f32}),keys.clone(),occupied);
            self.call("on_update",json!({"dt":DT/renders as f32}),keys.clone(),occupied);
        }
        self.world.begin_force_frame();
        self.call("on_fixed_update",json!({"dt":DT}),keys,occupied);
        self.world.step(DT);
    }
}

#[test]
fn installed_drive_preserves_physics_across_render_rates_and_replica_geometry() {
    let mut slow=Drive::new();let mut fast=Drive::new();
    for drive in [&mut slow,&mut fast] {
        drive.step(json!({"F10":true}),false,1);
        drive.step(json!({}),false,1);
        assert!(drive.car.is_some(),"real Skyline spawn command must succeed");
    }
    for tick in 0..360 {
        // Settle first; afterwards the same driver state at 120 and 480 render FPS.
        let occupied=tick>=120;
        slow.step(json!({}),occupied,1);fast.step(json!({}),occupied,4);
        let a=slow.world.read(slow.car.unwrap()).unwrap();
        let b=fast.world.read(fast.car.unwrap()).unwrap();
        for (x,y) in a.position.iter().zip(b.position.iter()).chain(a.linvel.iter().zip(b.linvel.iter())) {
            assert!(x.is_finite() && y.is_finite() && (x-y).abs()<1e-4,"render-dependent physics at tick {tick}: {a:?} vs {b:?}");
        }
    }
    let id=fast.car.unwrap();let car=fast.world.read(id).unwrap();
    assert!(fast.forces>300 && car.linvel[2]>1.,"current car must actually drive: {car:?}");
    assert!(car.position[1]>0.2,"car must stay supported by real ground");
    let definition=fast.world.body_definition(id).unwrap();
    assert!(definition.body.ccd && definition.body.contact_group==8);
    assert!(matches!(definition.body.shape,Shape::Compound{..}));
    let wire=definition_codec::encode(&definition).unwrap();
    let decoded=definition_codec::decode(&wire).unwrap();
    let mut remote=DynamicsWorld::default();let replica=remote.spawn_replica(&decoded).unwrap();
    remote.set_pose(replica,car.position,car.rotation);
    remote.set_kinematic_motion(replica,car.linvel,car.angvel);
    let local=fast.world.solid_bodies();let mirrored=remote.solid_bodies();
    assert_eq!(local[0].colliders.len(),mirrored[0].colliders.len());
    assert!((local[0].inverse_mass-mirrored[0].inverse_mass).abs()<1e-7);
    // A rapid sweep through the actual car must hit the same surface on each peer.
    let [x,y,z]=car.position;
    let a=sweep_sphere(&local,[x-5.,y,z],[x+5.,y,z],0.3).expect("local car collision").1;
    let b=sweep_sphere(&mirrored,[x-5.,y,z],[x+5.,y,z],0.3).expect("replica collision").1;
    assert!((a.time_of_impact-b.time_of_impact).abs()<1e-5);
    assert!(!remote.add_velocity_delta(replica,[10.,0.,0.],[0.;3]),"remote owner must not receive a duplicate reaction");
}
