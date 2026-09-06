//! BipedGround PreUpdate82D30D30 job production. The scene/host completes
//! contact queries before this call and retains the resulting contact packet.
use super::{contact_packet::Packet,controller::{GroundJob,Frame,Vector},
    ground_entry::State,ground_input::{self,GroundInput},ground_query};
use crate::{math::Vector3,point_graph::PointGraph};

pub struct Input {
    pub previous_state: u32,
    pub frames_since_teleport: u32,
    pub fallback_line_hit: Option<Vector>,
    pub controls: GroundInput,
    pub flags_2476: u32,
    pub flags_2480: u32,
    pub flags_2484: u32,
    pub flags_2488: u32,
    pub animation_motion_2864: Vector,
    pub duration_2896: f32,
    pub phase_2900: f32,
    pub override_duration_2904: f32,
    pub position_592: Vector,
    pub velocity_608: Vector,
    pub skeleton_motion_16320: Vector,
    pub skeleton_displacements_16288_16304: [Vector;2],
}
pub struct Prepared {
    pub job: GroundJob,
    /// Retained until FillPhysOut82D32D38, including754 which is not a controller input.
    pub geometry_flags_752_to_754: [bool;3],
}
fn xyz(v: Vector) -> Vector3 {Vector3::new(v[0],v[1],v[2])}
fn vector(v: Vector3) -> Vector {[v.x,v.y,v.z,0.]}
fn frame(f: Frame) -> ground_query::Frame {
    ground_query::Frame {right:xyz(f[0]),up:xyz(f[1]),forward:xyz(f[2]),position:xyz(f[3])}
}

/// A completed toolkit packet exists only when the native contact age320>0.
/// Missing completion preserves retained contact fields, except for the original
/// preceding-riding-state line fallback. No synthetic floor hit is introduced.
pub fn prepare(state: &mut State,retained: &mut Packet,completed: Option<Packet>,
    geometry: Option<ground_query::GroundGeometry>,input: &Input,
    movement_curve: &PointGraph<8>,turn_curve: &PointGraph<8>) -> Prepared
{
    let timestep=f32::from_bits(0x3c88_8889);
    state.distance_164+=timestep;
    let mut both_feet=true;
    if let Some(packet)=completed {
        *retained=packet;
        both_feet=retained.flags & 0x30 == 0x30;
    } else if input.previous_state!=501 {
        if let Some(position)=input.fallback_line_hit {
            retained.flags=1;
            retained.position=position;
            retained.normal=state.frame_80[1];
        }
    }
    let suppress_minimum=input.frames_since_teleport<20;
    if suppress_minimum {retained.flags &= !8;}
    let mut controls=input.controls;
    controls.frame_forward_112=[state.frame_80[2][0],state.frame_80[2][1],state.frame_80[2][2]];
    let control=ground_input::calculate(&controls,movement_curve,turn_curve);
    let adjustment=ground_query::consume_geometry(ground_query::ConsumeInput {
        frame_80:frame(state.frame_80),contact_position_192:xyz(retained.position),
        contact_flags_368:retained.flags,reach_364:retained.distance_172,
        previous_input_up_416:xyz(retained.normal),
    },geometry);
    if !both_feet && !adjustment.state_753 && retained.flags & 8 == 0 {state.distance_164=0.;}
    let target=adjustment.frame_768;
    let job=GroundJob {
        contact_position:retained.position,
        contact_normal:[adjustment.input_up_416.x,adjustment.input_up_416.y,adjustment.input_up_416.z,retained.normal[3]],
        support_frame:retained.support_frame,target_position:retained.target_position,
        target_normal:retained.target_normal,edge_position:retained.edge_position,edge_normal:retained.edge_normal,
        flags:retained.flags,support_id:retained.support_id,
        collision_displacements:input.skeleton_displacements_16288_16304,
        animation_motion:input.animation_motion_2864,animation_velocity:input.skeleton_motion_16320,
        desired_direction:control.state_656,
        animation_position:std::array::from_fn(|i|input.velocity_608[i].mul_add(timestep,input.position_592[i])),
        requested_duration:input.duration_2896,mirrored:input.flags_2476 & 4 != 0,
        requested_phase:input.phase_2900,override_duration:input.override_duration_2904,
        animation_directed:input.flags_2488 & 0x0800_0000 != 0,
        movement:control.state_708,steering:control.state_712,sprint_pressed:input.flags_2484 & 0x20000 != 0,
        suppress_lean:input.flags_2480 & 128 != 0,suppress_minimum,
        target_frame_present:adjustment.state_752,edge_active:adjustment.state_753,
        target_frame:[vector(target.right),vector(target.up),vector(target.forward),vector(target.position)],
        ignore_obstacle:input.controls.processed_flags_2472 & 0x1000_0000 != 0,
    };
    Prepared {job,geometry_flags_752_to_754:[adjustment.state_752,adjustment.state_753,adjustment.state_754]}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Input {
        Input {previous_state:500,frames_since_teleport:20,fallback_line_hit:None,
            controls:GroundInput {processed_flags_2472:0x1000_0000,processed_direct_2684:0.75,
                processed_direct_2680:-0.5,processed_stick_2692:0.,processed_stick_2688:0.,
                processed_scale_2912:1.,processed_scale_2908:1.,frame_forward_112:[0.,0.,1.]},
            flags_2476:4,flags_2480:128,flags_2484:0x20000,flags_2488:0x0800_0000,
            animation_motion_2864:[1.,2.,3.,4.],duration_2896:0.25,phase_2900:0.75,override_duration_2904:1.5,
            position_592:[10.,20.,30.,0.],velocity_608:[60.,120.,180.,0.],skeleton_motion_16320:[4.,3.,2.,1.],
            skeleton_displacements_16288_16304:[[1.;4],[2.;4]]}
    }
    fn curves() -> PointGraph<8> {PointGraph {x:std::array::from_fn(|i|i as f32),y:[1.;8]}}
    #[test]
    fn delayed_contact_has_priority_and_suppression_expires_at_twenty() {
        let curves=curves();
        for age in [19,20] {
            let mut state=State::default();
            let mut retained=Packet::default();
            let mut input=input(); input.frames_since_teleport=age;input.fallback_line_hit=Some([99.;4]);
            let mut completed=Packet::default();completed.flags=57;completed.position=[0.,-1.,0.,0.];
            let prepared=prepare(&mut state,&mut retained,Some(completed),None,&input,&curves,&curves);
            let job=prepared.job;
            assert_eq!(job.contact_position,completed.position);
            assert_eq!(job.suppress_minimum,age==19);
            assert_eq!(job.flags,if age==19 {49} else {57});
            assert_eq!(job.animation_position,[11.,22.,33.,0.]);
            assert_eq!(job.collision_displacements,[[1.;4],[2.;4]]);
            assert_eq!(job.movement,0.75); assert_eq!(job.steering,-0.5);
            assert!(job.mirrored && job.sprint_pressed && job.suppress_lean && job.animation_directed && job.ignore_obstacle);
        }
    }
    #[test]
    fn fallback_is_only_used_when_no_completed_contact_and_previous_state_is_not_biped_air() {
        let curves=curves();
        for previous in [500,501] {
            let mut state=State::default();
            let mut retained=Packet::default();
            let mut input=input();input.previous_state=previous;input.fallback_line_hit=Some([0.,-1.,0.,0.]);
            let result=prepare(&mut state,&mut retained,None,None,&input,&curves,&curves);
            assert_eq!(result.job.flags,if previous==501 {0} else {1});
            assert_eq!(retained.position,if previous==501 {[0.;4]} else {[0.,-1.,0.,0.]});
        }
    }
}
