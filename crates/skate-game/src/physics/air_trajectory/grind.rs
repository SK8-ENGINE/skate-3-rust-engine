//! Live TU3 trajectory-to-spline query82D69C00 and admission82D6A840.
use skate_core::{
    air::trajectory::{grind::*, Prediction},
    physics::{board_world::BoardWorld, grind_contact::Primitive},
    point_graph::PointGraph,
};
use skate_data::collections::Collections;
type V = [f32; 4];
pub(super) struct Settings {
    pub limits: GrindAssistLimits,
    height: PointGraph<8>,
    padding: f32,
    maximum_adjust: f32,
    velocity_scalar: f32,
    max_angle: f32,
    score: f32,
    truck_distance: f32,
}
impl Settings {
    pub fn load(data: &Collections) -> Result<Self, String> {
        let t = |key| data.float("physics_trajectory", "default", key);
        Ok(Self {
            limits: GrindAssistLimits {
                lock_distance: 0.0,
                max_speed_squared_ledge: t("GrindMaxSpeedSqrIntoLedge")?,
                max_speed_squared_rail: t("GrindMaxSpeedSqrIntoGrind")?,
                max_downward_speed: t("GrindMaxSpeedDownOntoGrind")?,
                ledge_scalars: [
                    t("GrindLockLedgeScalar2")?,
                    t("GrindLockLedgeScalar")?,
                    t("GrindLockLedgeLowSideScalar2")?,
                    t("GrindLockLedgeLowSideScalar")?,
                ],
                tip_scalar: t("GrindTipScalar")?,
                maximum_adjust_angle: t("GrindAdjustMaxAngle")?,
                deck_dimensions: [
                    data.float("physicsdeck", "default", "DeckMidLength")?,
                    data.float("physicsdeck", "default", "DeckFrontEndSize")?,
                ],
            },
            height: super::settings::graph8(
                data,
                "physics_trajectory",
                "RequiredAngleVsHeight",
                true,
            )?,
            padding: t("GrindOffset")?,
            maximum_adjust: t("MaxTrajectoryAdjust")?,
            velocity_scalar: t("GrindLandingVelScalar")?,
            max_angle: t("GrindLandingMaxAngle")?,
            score: t("ScoreGrind")?,
            truck_distance: data.float("physics_grinds", "default", "DeckCenterToTruck")?,
        })
    }
    pub fn evaluate(
        &self,
        prediction: &mut Prediction,
        acquire: bool,
        world: &BoardWorld,
        edges: &[Primitive],
        nearby: &mut Vec<Primitive>,
        lock_distance: f32,
        com: V,
        board_position: V,
    ) -> Result<GrindEvaluation, String> {
        let velocity = prediction.collision_velocity();
        let landing = prediction
            .request
            .trajectory
            .position_at(prediction.result.contact_time);
        if acquire {
            *nearby = edges
                .iter()
                .copied()
                .filter(|e| {
                    (0..3).all(|i| {
                        e.start[i].min(e.end[i]) <= landing[i] + if i == 1 { 4.0 } else { 2.0 }
                            && e.start[i].max(e.end[i])
                                >= landing[i] - if i == 1 { 0.5 } else { 2.0 }
                    })
                })
                .take(40)
                .collect();
        }
        let landing = prediction.result.contact_position;
        let mut distance: f32 = 1000.0;
        for e in nearby.iter() {
            let delta = sub(e.end, e.start);
            let len = dot(delta, delta);
            let from = sub(landing, e.start);
            let projected = sub(from, delta.map(|v| v * dot(from, delta) / len));
            distance = distance.min(
                dot(projected, projected)
                    .sqrt()
                    .min(dot(from, from).sqrt())
                    .min(dot(sub(landing, e.end), sub(landing, e.end)).sqrt()),
            );
        }
        if acquire {
            let mut candidates = nearby
                .iter()
                .enumerate()
                .filter_map(|(i, e)| consider_grind_primitive(*prediction, *e, i, self.padding))
                .collect();
            let mut limits = self.limits.clone();
            limits.lock_distance = lock_distance;
            while let Some(c) = take_best_grind(&mut candidates, lock_distance, &self.height) {
                let e = nearby[c.primitive];
                let Some(surface) = skate_core::air::trajectory::grind_surface::investigate(
                    world,
                    e.start,
                    e.end,
                    c.point,
                    self.truck_distance,
                )?
                else {
                    continue;
                };
                let Some(delta) = admitted_displacement(
                    *prediction,
                    c,
                    e,
                    velocity,
                    com,
                    surface.evidence,
                    &limits,
                ) else {
                    continue;
                };
                apply_admitted_target(
                    prediction,
                    c,
                    delta,
                    surface.normal,
                    velocity,
                    self.maximum_adjust,
                    self.velocity_scalar,
                    self.max_angle,
                );
                bevy::log::info!("GRIND_ASSIST lock_distance={} miss_distance={} time={} rail={} correction={:?}",lock_distance,c.distance,c.time,e.owner,delta);
                return Ok(GrindEvaluation {
                    target: Some(GrindTarget {
                        edge: e,
                        point: c.point,
                        normal: surface.normal,
                        air_limits: surface.air_limits(e.start, e.end, c.point, board_position),
                    }),
                    score: self.score,
                    distance,
                });
            }
        }
        Ok(GrindEvaluation {
            target: None,
            score: 0.0,
            distance,
        })
    }
}
fn sub(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: V, b: V) -> f32 {
    skate_core::riding::ground_correction_math::dot_product(a, b)
}
