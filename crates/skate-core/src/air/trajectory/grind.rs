//! TU3 rail-trajectory candidate leaves82D60C80/82D60B98,
//! 82D6A398 and82D6A168. Admission and physical adjustment are separate owners.
use super::{math::*, Prediction, Trajectory};
use crate::{physics::grind_contact::Primitive, point_graph::PointGraph};

#[derive(Clone, Copy, Debug)]
pub struct GrindTrajectoryCandidate {
    pub point: Vector,             //0 projected point on rail
    pub trajectory_point: Vector,  //16 centre with radius/offset removed
    pub direction: Vector,         //32 normalized rail direction
    pub approach: Vector,          //48 velocity rejected from rail normal
    pub distance: f32,             //64 perpendicular miss distance
    pub time: f32,                 //68 crossing time
    pub angle: f32,                //72 folded approach angle
    pub frame: i32,                //76 fixed60Hz time
    pub primitive: usize,          //80 query index
}

///The82C21930 result must come from the world geometry investigator. A
///spline's authored name or a nearest-distance guess cannot supply these lanes.
#[derive(Clone, Copy, Debug)]
pub struct GrindSurfaceEvidence {
    pub kind: u32,                 //0 rail,1 edge,2 ledge,3 rejected
    pub side: Vector,              //classification64
}

#[derive(Clone, Debug)]
pub struct GrindAssistLimits {
    pub lock_distance: f32,        //physics_mode80
    pub max_speed_squared_ledge: f32, //physics_trajectory520
    pub max_speed_squared_rail: f32,  //524
    pub max_downward_speed: f32,      //528
    pub ledge_scalars: [f32;4],       //532,536,540,544
    pub tip_scalar: f32,             //512
    pub maximum_adjust_angle: f32,   //560 degrees
    ///Actual deck dimensions from82762E50 (DeckMidLength) and82C00868.
    pub deck_dimensions: [f32;2],
}

///Admission and displacement portion82D6A840. The caller retries the next
///ranked candidate on None, then applies82D609E0 and82D6AF58 on success.
///This deliberately requires native surface evidence before any correction.
pub fn admitted_displacement(
    prediction: Prediction, candidate: GrindTrajectoryCandidate, edge: Primitive,
    reference_velocity: Vector, processed_592: Vector, surface: GrindSurfaceEvidence,
    limits: &GrindAssistLimits,
) -> Option<Vector> {
    let natural_landing=prediction.request.trajectory.position_at(prediction.result.contact_time);
    if candidate.point[1]-natural_landing[1]<f32::from_bits(0xbecc_cccd)
        || candidate.direction[1]>0.9 { return None; }
    let direction=normalize(sub(edge.end,edge.start));
    let perpendicular=sub(reference_velocity,scale(direction,dot(direction,reference_velocity)));
    if (perpendicular[1]*(1.0-direction[1].abs())).abs()>limits.max_downward_speed {
        return None;
    }
    if surface.kind==3 { return None; }
    let mut horizontal=perpendicular; horizontal[1]=0.0;
    let max_speed=if surface.kind==2 {limits.max_speed_squared_ledge} else {limits.max_speed_squared_rail};
    if dot(horizontal,horizontal)>max_speed { return None; }
    let delta=sub(candidate.trajectory_point,candidate.point);
    let lock_distance=if surface.kind==2 {
        let incoming=dot(delta,surface.side)>0.0;
        let body=dot(sub(processed_592,edge.start),surface.side)>0.0;
        limits.lock_distance*limits.ledge_scalars[if incoming {0} else {2}+if body {0} else {1}]
    } else {limits.lock_distance};
    if candidate.distance>=lock_distance.max(0.1) { return None; }
    let miss=sub(candidate.trajectory_point,edge.start);
    let mut correction=scale(sub(miss,scale(candidate.direction,dot(candidate.direction,miss))),-1.0);
    let distance=length(correction);
    if distance>=limits.lock_distance {
        let half_dimension=limits.deck_dimensions[1].mul_add(0.5,limits.deck_dimensions[0]*0.5);
        correction=scale(correction,(-limits.tip_scalar).mul_add(half_dimension,distance)/distance);
    }
    let original=prediction.request.trajectory.velocity;
    let adjusted=madd(correction,reciprocal(candidate.time),original);
    let angle=if dot(original,original)*dot(adjusted,adjusted)>f32::from_bits(0x38d1_b717) {
        angle_between(original,adjusted)
    } else {0.0};
    if angle>limits.maximum_adjust_angle*f32::from_bits(0x3c8e_fa35) { return None; }
    Some(correction)
}

///82D60C80's quadratic roots followed by82D60B98's later-root selection.
/// A double root must be strictly positive. The two-root branch takes max;
/// it does not add a positive-time restriction absent from the source.
pub fn descending_plane_time(t: Trajectory, point: Vector, normal: Vector) -> Option<f32> {
    let c = dot(normal,t.position)-dot(point,normal);
    let b = dot(normal,t.velocity);
    let a = dot(normal,scale(t.acceleration,reciprocal(2.0)));
    let discriminant = b*b - (4.0*a)*c;
    if discriminant < 0.0 { return None; }
    let inverse = reciprocal(2.0*a);
    if discriminant > 0.0 {
        let root = discriminant*inverse_length(discriminant);
        Some((inverse*(-b+root)).max(inverse*(-b-root)))
    } else {
        let time = inverse * -b;
        (time>0.0).then_some(time)
    }
}

///82D6A398. The padding is physics_trajectory+516, not a host snap radius.
pub fn consider_grind_primitive(
    prediction: Prediction, edge: Primitive, primitive: usize, padding: f32,
) -> Option<GrindTrajectoryCandidate> {
    let mut delta = sub(edge.end,edge.start);
    let normal_unscaled = cross(delta,cross(UP,delta));
    let normal = scale(normal_unscaled,inverse_length(dot(normal_unscaled,normal_unscaled)));
    let rail_length = length(delta);
    //830BD320 is initialized to1 by82F825D0. The original endpoints remain
    //unchanged; only the direction and endpoint overrun vector are capped.
    if rail_length>1.0 { delta=scale(delta,reciprocal(rail_length)); }
    let offset = madd(normal,prediction.request.radius,scale(normal,padding));
    let time = descending_plane_time(prediction.request.trajectory,add(edge.start,offset),normal)?;
    let trajectory_point = sub(prediction.request.trajectory.position_at(time),offset);
    let direction = normalize(delta);
    let difference = sub(trajectory_point,edge.start);
    let distance = length(cross(difference,direction));
    let point = madd(direction,dot(direction,difference),edge.start);
    let extension = scale(delta,0.1);
    //Strict endpoint test82D6A708. No closest-point clamping.
    if !(dot(sub(point,sub(edge.start,extension)),sub(point,add(edge.end,extension)))<0.0) {
        return None;
    }
    let velocity = prediction.request.trajectory.velocity;
    let approach = normalize(sub(velocity,scale(normal,dot(normal,velocity))));
    let mut horizontal = sub(point,prediction.request.trajectory.position);
    horizontal[1]=0.0;
    let angle = folded_approach_angle(horizontal,direction,normal);
    Some(GrindTrajectoryCandidate {
        point,trajectory_point,direction,approach,distance,time,angle,
        frame:(time*60.0) as i32,primitive,
    })
}

///8296EC98's oriented angle then82E09C80's actual fractional-turn fold.
///Keep the wrap arithmetic: acos(abs(dot)) is not binary32 equivalent.
fn folded_approach_angle(a: Vector,b: Vector,normal: Vector) -> f32 {
    let mut angle=angle_between(a,b);
    let aa=dot(a,a); let bb=dot(b,b);
    if aa>f32::from_bits(0x38d1_b717) && bb>f32::from_bits(0x38d1_b717) {
        let unit=|v,square| {
            let r=crate::physics::native_arithmetic::reciprocal_square_root_estimate(square);
            scale(v,(r*0.5).mul_add((-square).mul_add(r*r,1.0),r))
        };
        if dot(cross(unit(a,aa),unit(b,bb)),normal)<0.0 {
            angle=f32::from_bits(0x40c9_0fdb)-angle;
        }
    }
    let turns=angle*f32::from_bits(0x3e22_f983);
    let fraction=turns-turns.floor();
    let wrapped=(fraction-if fraction>0.5 {1.0} else {0.0})*f32::from_bits(0x40c9_0fdb);
    let sign=if wrapped>0.0 {1.0} else {-1.0};
    let magnitude=wrapped*sign;
    let folded=if magnitude>f32::from_bits(0x3fc9_0fdb) {
        magnitude-std::f32::consts::PI
    } else {magnitude};
    (sign*folded).abs()
}

///82D6A168 consumes one winner per call. Retrying after a rejected target
///must retain the remaining insertion order, not sort the list by distance.
pub fn take_best_grind(
    candidates: &mut Vec<GrindTrajectoryCandidate>,
    difficulty_distance: f32, height_penalty: &PointGraph<8>,
) -> Option<GrindTrajectoryCandidate> {
    let mut chosen=None;
    let mut fallback=1000.0;
    let mut best_angle=std::f32::consts::PI;
    let mut height=-10000.0;
    for (i,candidate) in candidates.iter().enumerate() {
        if candidate.distance<difficulty_distance {
            let benefit=best_angle-candidate.angle;
            if benefit>height_penalty.evaluate(candidate.point[1]-height) {
                height=candidate.point[1];
                fallback=-1.0;
                best_angle=candidate.angle;
                chosen=Some(i);
            }
        } else if candidate.distance<fallback {
            fallback=candidate.distance;
            chosen=Some(i);
        }
    }
    chosen.map(|i|candidates.remove(i))
}
