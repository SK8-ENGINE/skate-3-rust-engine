//! Original82D2CFE8 nearest distance and82D2D2B0 arc-length evaluation.
use super::{Record,math::*};
fn frame(r:&Record)->[Vector;4] {[r.vector(0),r.vector(16),r.vector(32),r.vector(48)]}
fn indices(r:&Record)->impl Iterator<Item=usize> {
    let count=r.points().len();
    let reverse=r.byte(200)&0x20!=0;
    (0..count).map(move |i| if reverse {count-1-i} else {i})
}
pub fn nearest_distance(r:&Record,world:Vector)->f32 {
    if r.points().len()<=1 {return 0.;}
    let local=inverse_point(frame(r),world);
    let mut indices=indices(r);
    let mut a=r.points()[indices.next().expect("nonempty source spline")];
    let mut accumulated=0.;let mut best=f32::MAX;let mut distance=0.;
    for index in indices {
        let b=r.points()[index];
        let near=closest_point(local,[a,b]);
        let error=sub(local,near);let squared=dot(error,error);
        if best>squared {
            best=squared;
            let partial=sub(near,a);
            //82D2D204 zero mask applies AFTER adding accumulated distance.
            distance=if dot(partial,partial)==0. {0.} else {accumulated+length(partial)};
        }
        let segment=sub(b,a);
        //82D2D260 has the same post-add zero selection.
        accumulated=if dot(segment,segment)==0. {0.} else {accumulated+length(segment)};
        a=b;
    }
    distance
}
/// 82D2D550: spline point at signed position `t` from the middle (b148); projected lengths, never reversed: docs/hails-additions/27-dynamic-props.md#spline-point-at-along
pub fn at_along(r:&Record,t:f32)->Vector {
    let half=r.scalar(180);
    let [first_end,last_end]=r.endpoints();
    let points=r.points();
    if t<= -half || points.len()<=1 {return first_end;}
    if t>=half {return last_end;}
    let u=if r.byte(200)&0x20!=0 {half-t} else {t+half};
    let axis=normalize(sub(points[points.len()-1],points[0]));
    let mut total=0.;
    for pair in points.windows(2) {
        let (a,b)=(pair[0],pair[1]);
        let segment=sub(b,a);
        let projected=dot(segment,axis).max(1e-4);
        total+=projected;
        if total>u {
            let back=(total-u)/projected;
            let local=std::array::from_fn(|i| b[i]-segment[i]*back);
            return transform(frame(r),local);
        }
    }
    last_end
}
pub fn at_distance(r:&Record,distance:f32)->Vector {
    let endpoints=r.endpoints();
    if r.points().len()<=1 || 0.>=distance {return endpoints[0];}
    let mut indices=indices(r);
    let mut a=r.points()[indices.next().expect("nonempty source spline")];
    let mut accumulated=0.;
    for index in indices {
        let b=r.points()[index];let direction=sub(b,a);let len=length(direction);
        accumulated+=len;
        if accumulated>distance {
            if 0.>=len {return transform(frame(r),b);}
            let remaining=reciprocal(len)*(accumulated-distance);
            let local=std::array::from_fn(|i| b[i]-direction[i]*remaining);
            return transform(frame(r),local);
        }
        a=b;
    }
    endpoints[1]
}
