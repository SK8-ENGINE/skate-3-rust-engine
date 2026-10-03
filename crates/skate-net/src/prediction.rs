//! Short-lived collision prediction in local arrival time. Never subtract peer
//! capture timestamps from a local clock: the clocks need not be synchronized.
use crate::{Body, Pose};

/// Body snapshots normally arrive every 50 ms. Coast for one interval, then
/// smoothly stop over two more intervals. Expire instead of leaving an invisible
/// moving obstacle behind when a peer stalls. Rendering has its own buffer.
#[derive(Clone, Copy, Debug)]
pub struct CollisionPrediction {
    travel: f32,
    speed: f32,
}
impl CollisionPrediction {
    pub fn at(age: f32) -> Option<Self> {
        const COAST: f32 = 0.05;
        const EXPIRE: f32 = 0.15;
        if !age.is_finite() || age < 0. || age >= EXPIRE {
            return None;
        }
        let fading = (age - COAST).max(0.);
        let fade_duration = EXPIRE - COAST;
        Some(Self {
            // Integral of the velocity scale, so contact velocity matches the
            // motion of the predicted collider even during packet loss.
            travel: age - fading * fading / (2. * fade_duration),
            speed: 1. - fading / fade_duration,
        })
    }

    /// Input bodies have already passed the wire protocol's finite-value bounds.
    pub fn body(self, body: &Body) -> Body {
        let [wx, wy, wz] = body.angular;
        let magnitude = (wx * wx + wy * wy + wz * wz).sqrt();
        let mut q = body.pose.q;
        if magnitude > 0. {
            let half_angle = 0.5 * magnitude * self.travel;
            let scale = half_angle.sin() / magnitude;
            let [x, y, z] = [wx * scale, wy * scale, wz * scale];
            let w = half_angle.cos();
            let [qx, qy, qz, qw] = q;
            // World-space angular velocity: delta rotation multiplies on left.
            q = [
                w * qx + x * qw + y * qz - z * qy,
                w * qy - x * qz + y * qw + z * qx,
                w * qz + x * qy - y * qx + z * qw,
                w * qw - x * qx - y * qy - z * qz,
            ];
        }
        let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        Body {
            pose: Pose {
                p: std::array::from_fn(|i| body.pose.p[i] + body.velocity[i] * self.travel),
                q: q.map(|v| v / length),
            },
            velocity: body.velocity.map(|v| v * self.speed),
            angular: body.angular.map(|v| v * self.speed),
        }
    }
}

/// Distinguish a relocation from legitimate high-speed travel using source time,
/// not arrival jitter. Ten metres is slack for root animation and an impact;
/// published body speed supplies the additional distance possible between samples.
pub fn is_discontinuity(
    previous: &crate::packed::BodyState,
    current: &crate::packed::BodyState,
    elapsed: f32,
) -> bool {
    let dt = if elapsed.is_finite() {
        elapsed.max(0.)
    } else {
        0.
    };
    let speed = previous
        .bodies
        .iter()
        .chain(&current.bodies)
        .map(|b| b.velocity.iter().map(|v| v * v).sum::<f32>().sqrt())
        .fold(0., f32::max);
    let distance_squared = previous
        .root
        .p
        .iter()
        .zip(current.root.p)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>();
    distance_squared > (10. + speed * dt * 1.25).powi(2)
}
