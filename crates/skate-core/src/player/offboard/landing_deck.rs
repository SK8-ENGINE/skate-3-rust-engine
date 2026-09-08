//! LandingOnDeckManager trajectory preparation,82D78B28/82D78EE8/82D78D30.
//! The board frame and velocity are observations of the canonical live board.
use super::air_launch::{V,dot,length,madd,scale,sub};
use crate::air::trajectory::Trajectory;
const ZERO:V=[0.;4];
const GRAVITY:V=[0.,-9.8,0.,0.];
#[derive(Clone,Copy,Debug)]
pub struct Settings {pub deck_min_uprightness:f32,pub approximate_com_height:f32}
#[derive(Clone,Copy,Debug)]
pub struct Input {
 pub board_position:V,pub board_velocity:V,pub board_up:V,pub up:V,
 pub board_contact_count:i32,pub flags_2480:u32,
}
#[derive(Clone,Debug)]
pub struct State {
 pub trajectory:Trajectory,pub proposed:Trajectory,pub elapsed:f32,pub trajectory_valid:bool,
 pub horizontal_correction:V,pub remaining:f32,pub proposed_remaining:f32,
 pub contact_height:f32,pub query_count:i32,pub can_land:bool,pub force:bool,
 pub blocked:bool,pub first_query:bool,pub hippy_hurdling:bool,pub collision_override:bool,
 pub collision_normal:V,pub query_pending:bool,
}
impl Default for State {
 fn default()->Self {
  let trajectory=Trajectory{position:ZERO,velocity:ZERO,acceleration:ZERO,duration:-1.};
  Self{trajectory,proposed:trajectory,elapsed:0.,trajectory_valid:false,horizontal_correction:ZERO,
   remaining:0.,proposed_remaining:0.,contact_height:0.,query_count:0,can_land:false,force:false,
   blocked:false,first_query:false,hippy_hurdling:false,collision_override:false,collision_normal:ZERO,query_pending:false}
 }
}
impl State {
 ///82D78EE8: solve the descending crossing of the deck's COM-height plane,
 ///then prepare the velocity matching trajectory. No scene hit is invented.
 pub fn probe(&mut self,input:Input,settings:Settings,position:V,velocity:V)->bool {
  let mut board_velocity=input.board_velocity;
  let acceleration=if input.board_contact_count==0 {board_velocity[1]=0.;scale(GRAVITY,0.5)} else {GRAVITY};
  let relative=sub(velocity,board_velocity);
  let plane=madd(input.up,settings.approximate_com_height,input.board_position);
  let a=dot(scale(acceleration,0.5),input.up);
  let b=dot(relative,input.up);let c=dot(sub(position,plane),input.up);
  let discriminant=b*b-4.*a*c;
  if discriminant<0. {self.can_land=false;return false;}
  //82D60B98 picks the larger root of82D60C80; the caller requires t>0.
  let root=discriminant.sqrt();
  let time=((-b+root)/(2.*a)).max((-b-root)/(2.*a));
  if !(time>0.) {self.can_land=false;return false;}
  let distance=length(sub(position,plane));
  let duration=(2.*distance/9.8).sqrt().max(time*0.75).min(time*1.3);
  self.proposed=Trajectory{position,
   velocity:madd(input.board_velocity,1.,sub(scale(sub(plane,position),1./duration),scale(GRAVITY,0.5*duration))),
   acceleration:GRAVITY,duration:-1.};
  let relative_arc=Trajectory{position,velocity:relative,acceleration,duration:-1.};
  self.horizontal_correction=sub(plane,relative_arc.position_at(time));self.horizontal_correction[1]=0.;
  self.remaining=time;self.proposed_remaining=duration;
  true
 }
 ///82D78D30, before the world's obstruction query82D79948. Some is the
 ///actual duration to submit to that query; None means no submission this tick.
 pub fn update_air(&mut self,input:Input,settings:Settings,position:V,velocity:V,maximum_velocity_difference:f32)->Option<f32> {
  self.query_count=0;self.first_query=false;
  if (!self.force&&input.flags_2480&0x8000!=0)||!(input.board_up[1]>settings.deck_min_uprightness) {return None;}
  self.probe(input,settings,position,velocity);
  if self.remaining>0.4 {
   self.trajectory=self.proposed;self.trajectory_valid=true;self.remaining=self.proposed_remaining;
   self.can_land=length(sub(velocity,self.proposed.velocity))<maximum_velocity_difference;
  }
  if self.can_land&&!self.first_query {self.first_query=true;Some(self.proposed_remaining)} else {None}
 }
 ///82D79948 uses a distinct obstruction trajectory to the moving deck +.2m.
 pub fn obstruction_query(&mut self,input:Input,position:V,duration:f32)->Trajectory {
  let mut target=madd(input.board_velocity,duration,input.board_position);target[1]+=f32::from_bits(0x3e4cccce);
  self.blocked=false;self.query_pending=true;self.contact_height=target[1]-0.1;
  Trajectory{position,velocity:sub(scale(sub(target,position),1./duration),scale(GRAVITY,duration*0.5)),acceleration:GRAVITY,duration}
 }
}
