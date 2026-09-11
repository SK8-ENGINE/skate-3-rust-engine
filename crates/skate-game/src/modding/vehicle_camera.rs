//! Render-only chase/hood camera. Reads dynamics; never applies physical forces.
use bevy::prelude::*;
use skate_dynamics::DynamicsWorld;
use skate_mods::presentation::{CameraMode, CameraRigOptions};

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
}
impl Sample {
    fn blend(self, next:Self, alpha:f32) -> Self {
        Self { position:self.position.lerp(next.position,alpha),
            rotation:self.rotation.slerp(next.rotation,alpha),
            velocity:self.velocity.lerp(next.velocity,alpha) }
    }
}
#[derive(Default)]
struct Spring { value:Vec3, velocity:Vec3 }
impl Spring {
    fn reset(&mut self, target:Vec3) {self.value=target;self.velocity=Vec3::ZERO;}
    /// Exact critically damped solution for a piecewise-constant target.
    fn step(&mut self, target:Vec3, hz:f32, dt:f32) -> Vec3 {
        let w=std::f32::consts::TAU*hz;
        let x=self.value-target;
        let j=self.velocity+w*x;
        let e=(-w*dt).exp();
        self.value=target+(x+j*dt)*e;
        self.velocity=(self.velocity-w*j*dt)*e;
        self.value
    }
}
pub(super) struct View {pub transform:Transform,pub fov:f32,pub near:f32}
pub(super) struct Rig {
    pub body:String,
    pub options:CameraRigOptions,
    previous:Option<Sample>,
    current:Option<Sample>,
    acceleration:Vec3,
    eye:Spring,
    aim:Spring,
    heading:Vec3,
    fov:f32,
    boom_fraction:f32,
    initialized:bool,
}
fn horizontal(v:Vec3)->Vec3 {Vec3::new(v.x,0.0,v.z)}
fn unit_or(v:Vec3,otherwise:Vec3)->Vec3 {v.try_normalize().unwrap_or(otherwise)}
fn capped(v:Vec3,length:f32)->Vec3 {v.clamp_length_max(length)}
fn decay(dt:f32,half_life:f32)->f32 {1.0-(-std::f32::consts::LN_2*dt/half_life).exp()}
impl Rig {
    pub fn new(body:String,options:CameraRigOptions)->Self {
        Self {body,options,previous:None,current:None,acceleration:Vec3::ZERO,
            eye:Spring::default(),aim:Spring::default(),heading:Vec3::Z,
            fov:55.0,boom_fraction:1.0,initialized:false}
    }
    pub fn configure(&mut self,options:CameraRigOptions) {
        if self.options.mode!=options.mode {self.initialized=false;}
        self.options=options;
    }
    /// Called once AFTER each native fixed physics step, not on each render.
    pub fn record(&mut self,s:Sample,dt:f32) {
        let teleport=self.current.is_none_or(|old|old.position.distance(s.position)>25.0);
        if teleport {
            self.previous=Some(s);self.current=Some(s);
            self.acceleration=Vec3::ZERO;self.initialized=false;
        } else {
            let old=self.current.unwrap();
            let a=(s.velocity-old.velocity)/dt.max(0.0001);
            self.acceleration=self.acceleration.lerp(capped(a,35.0),decay(dt,0.10));
            self.previous=Some(old);self.current=Some(s);
        }
    }
    pub fn sample(&self,alpha:f32)->Option<Sample> {
        Some(self.previous?.blend(self.current?,alpha.clamp(0.0,1.0)))
    }
    pub fn view(&mut self,s:Sample,dt:f32,dynamics:&mut DynamicsWorld)->View {
        let o=&self.options;
        let forward=s.rotation*Vec3::Z;
        let up=s.rotation*Vec3::Y;
        let speed=horizontal(s.velocity).length();
        let t=(speed/o.speed_reference).clamp(0.0,1.0);
        let blend=t*t*(3.0-2.0*t);
        let target_fov=o.fov+if o.mode==CameraMode::Chase{o.fov_gain*blend}else{0.0};
        if !self.initialized { self.fov=target_fov; }
        self.fov+=(target_fov-self.fov)*decay(dt,0.18);
        if o.mode==CameraMode::Hood {
            let eye=s.position+s.rotation*Vec3::from_array(o.hood_offset);
            self.initialized=true;
            return View{transform:Transform::from_translation(eye).looking_at(eye+forward*30.0,up),
                fov:self.fov.to_radians(),near:o.near};
        }
        let body_heading=unit_or(horizontal(forward),self.heading);
        // Never unexpectedly swing the chase camera in front of a reversing car.
        let travelling_forward=s.velocity.dot(body_heading)>1.0;
        let velocity_heading=unit_or(horizontal(s.velocity),body_heading);
        let weight=if travelling_forward{o.velocity_heading*(speed/8.0).clamp(0.0,1.0)}else{0.0};
        let desired_heading=unit_or(body_heading.lerp(velocity_heading,weight),body_heading);
        if !self.initialized{self.heading=desired_heading;}
        self.heading=unit_or(self.heading.lerp(desired_heading,decay(dt,o.heading_half_life)),desired_heading);
        // Springs operate on offsets, not absolute world position. Thus camera lag
        // does not grow by velocity*time_constant during a constant-speed drive.
        let inertial_offset=capped(-self.acceleration*o.acceleration_lag,0.65);
        let desired_eye=-self.heading*(o.distance+o.distance_gain*blend)
            +Vec3::Y*(o.height+o.height_gain*blend)+inertial_offset;
        let look_velocity=if travelling_forward{horizontal(s.velocity)}else{Vec3::ZERO};
        let desired_aim=Vec3::Y*o.target_height+body_heading*0.45+capped(look_velocity*o.look_ahead,6.0);
        if !self.initialized {
            self.eye.reset(desired_eye);self.aim.reset(desired_aim);
            self.boom_fraction=1.0;self.initialized=true;
        }
        let eye=s.position+self.eye.step(desired_eye,o.spring_hz,dt);
        let aim=s.position+self.aim.step(desired_aim,o.spring_hz+1.0,dt);
        let pivot=s.position+Vec3::Y*o.target_height;
        let delta=eye-pivot;
        let distance=delta.length().max(0.001);
        let direction=delta/distance;
        let mut allowed=1.0_f32;
        if o.collision {
            // Five parallel rays approximate a camera-sized volume. Native ground
            // only: avoids self/skater hits. This is NOT a convex sphere sweep.
            let right=unit_or(direction.cross(Vec3::Y),Vec3::X);
            for offset in [Vec3::ZERO,right*o.collision_radius,-right*o.collision_radius,
                Vec3::Y*o.collision_radius,-Vec3::Y*o.collision_radius] {
                if let Some(hit)=dynamics.raycast_ground((pivot+offset).to_array(),direction.to_array(),distance) {
                    let d=(Vec3::from_array(hit.point)-(pivot+offset)).dot(direction);
                    allowed=allowed.min(((d-o.collision_radius)/distance).clamp(0.0,1.0));
                }
            }
        }
        // Snap IN immediately, ease OUT after obstruction. Never smooth through a wall.
        if allowed<self.boom_fraction{self.boom_fraction=allowed;}
        else{self.boom_fraction+=(allowed-self.boom_fraction)*decay(dt,0.12);}
        let safe_eye=pivot+delta*self.boom_fraction;
        let safe_aim=if aim.distance_squared(safe_eye)<1e-6{safe_eye+self.heading}else{aim};
        View{transform:Transform::from_translation(safe_eye).looking_at(safe_aim,Vec3::Y),
            fov:self.fov.to_radians(),near:o.near}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spring_has_frame_partition_invariance_for_fixed_target() {
        let target=Vec3::new(3.0,2.0,-6.0);
        let mut a=Spring::default();let mut b=Spring::default();
        for _ in 0..30{a.step(target,2.8,1.0/30.0);}
        for _ in 0..240{b.step(target,2.8,1.0/240.0);}
        assert!(a.value.distance(b.value)<1e-4);
        assert!(a.velocity.distance(b.velocity)<1e-4);
    }
    #[test]
    fn hood_is_rigid_and_does_not_accumulate_velocity_lag() {
        let mut o=CameraRigOptions::default();o.mode=CameraMode::Hood;
        let offset=Vec3::from_array(o.hood_offset);
        let mut r=Rig::new("chassis".into(),o);
        let s=Sample{position:Vec3::new(100.0,2.0,200.0),rotation:Quat::from_rotation_y(0.8),velocity:Vec3::Z*70.0};
        let mut d=DynamicsWorld::default();let v=r.view(s,1.0/60.0,&mut d);
        assert!(v.transform.translation.distance(s.position+s.rotation*offset)<1e-5);
    }
    #[test]
    fn camera_reset_discards_teleport_lag() {
        let mut r=Rig::new("car".into(),CameraRigOptions::default());
        let mut s=Sample{position:Vec3::ZERO,rotation:Quat::IDENTITY,velocity:Vec3::ZERO};
        r.record(s,1.0/60.0);s.position=Vec3::X*100.0;r.record(s,1.0/60.0);
        assert_eq!(r.sample(0.0).unwrap().position,s.position);
        assert!(!r.initialized);
    }
}
