//! World tuning writes: one entry for mods (`sdk.world.set_tuning`) and engine systems ([`set`]), per-owner patches.
//! Domains and their authority resources: docs/hails-additions/26a-living-world-overview.md#world-tuning-engine-side

use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::math::Vector3;
use skate_mods::world_tuning::{parse, CarryPatch, LivingWorldPatch, Merge, Patch, PropTuningPatch, PropsPatch, DOMAINS};

use crate::living_world::LivingWorldSettings;
use crate::physics::respawn::RespawnSettings;
use crate::retail_backdrop::BackdropSettings;
use crate::retail_exposure::ExposureMeter;
use crate::video_screens::VideoScreenSettings;
use crate::skater_ghost::GhostTuning;
use crate::retail_render::{DecalSettings, WorldShadowSettings, RETAIL_WORLD_SHADOW_FLOOR};
use crate::physics::prop_carry::{CarryButtons, CarrySettings, LocomotionOverrides};
use crate::physics::prop_dynamics::{PropBox, PropTuning, PropTuningSettings, PropTuningTable};

/// Per-owner patches, in arrival order (a re-set keeps the owner's place).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorldTuning {
    pub entries: Vec<(String, String, Value)>,
}

impl WorldTuning {
    fn merged<T: Merge + Default>(&self, domain: &str, pick: impl Fn(Patch) -> Option<T>) -> T {
        let mut out: Option<T> = None;
        for (_, d, v) in &self.entries {
            let Some(p) = (d == domain).then(|| parse(d, v)).flatten().and_then(&pick) else { continue };
            match out.as_mut() {
                Some(o) => o.merge(&p),
                None => out = Some(p),
            }
        }
        out.unwrap_or_default()
    }
}

/// Set (or with `None` remove) `owner`'s patch of `domain`, then rebuild that domain.
pub(crate) fn set(world: &mut World, owner: &str, domain: &str, patch: Option<Value>) -> Result<(), String> {
    if !DOMAINS.contains(&domain) {
        return Err(format!("unknown world tuning domain {domain}"));
    }
    if let Some(p) = &patch {
        parse(domain, p).ok_or_else(|| format!("invalid {domain} tuning patch"))?;
    }
    let mut t = world.remove_resource::<WorldTuning>().unwrap_or_default();
    let at = t.entries.iter().position(|(o, d, _)| o == owner && d == domain);
    match (at, patch) {
        (Some(i), Some(p)) => t.entries[i].2 = p,
        (None, Some(p)) => t.entries.push((owner.to_owned(), domain.to_owned(), p)),
        (Some(i), None) => {
            t.entries.remove(i);
        }
        (None, None) => {}
    }
    rebuild(world, &t, domain);
    world.insert_resource(t);
    Ok(())
}

/// A mod stopped, failed or reloaded: its patches go and the domains it touched are rebuilt.
pub(crate) fn clear_owner(world: &mut World, owner: &str) {
    let Some(mut t) = world.remove_resource::<WorldTuning>() else { return };
    let touched: Vec<String> = t.entries.iter().filter(|(o, _, _)| o == owner).map(|(_, d, _)| d.clone()).collect();
    t.entries.retain(|(o, _, _)| o != owner);
    for d in &touched {
        rebuild(world, &t, d);
    }
    world.insert_resource(t);
}

/// Every mod went: every domain back to the shipped values (and the player's own choices).
pub(crate) fn clear_all(world: &mut World) {
    let had = world.remove_resource::<WorldTuning>().is_some_and(|t| !t.entries.is_empty());
    let empty = WorldTuning::default();
    if had {
        for d in DOMAINS {
            rebuild(world, &empty, d);
        }
    }
    world.insert_resource(empty);
}

fn rebuild(world: &mut World, t: &WorldTuning, domain: &str) {
    match domain {
        "living_world" => {
            let p = t.merged(domain, |p| if let Patch::LivingWorld(p) = p { Some(p) } else { None });
            if let Some(mut s) = world.get_resource_mut::<LivingWorldSettings>() {
                apply_living_world(&mut s, &p);
            }
        }
        "props" => {
            let p = t.merged(domain, |p| if let Patch::Props(p) = p { Some(p) } else { None });
            let table = props_table(&p);
            match world.get_resource_mut::<PropTuningSettings>() {
                Some(mut s) => s.0 = table,
                None => world.insert_resource(PropTuningSettings(table)),
            }
        }
        "carry" => {
            let p = t.merged(domain, |p| if let Patch::Carry(p) = p { Some(p) } else { None });
            let c = carry_settings(&p);
            match world.get_resource_mut::<CarrySettings>() {
                Some(mut s) => *s = c,
                None => world.insert_resource(c),
            }
        }
        "shadows" => {
            let p = t.merged(domain, |p| if let Patch::Shadows(p) = p { Some(p) } else { None });
            let s = WorldShadowSettings { floor: p.world_floor.map_or(RETAIL_WORLD_SHADOW_FLOOR, Vec3::from_array) };
            match world.get_resource_mut::<WorldShadowSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "backdrop" => {
            let p = t.merged(domain, |p| if let Patch::Backdrop(p) = p { Some(p) } else { None });
            let retail = BackdropSettings::default();
            let s = BackdropSettings {
                visible: p.visible.unwrap_or(retail.visible),
                proxy_terrain: p.proxy_terrain.unwrap_or(retail.proxy_terrain),
            };
            match world.get_resource_mut::<BackdropSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "respawn" => {
            let p = t.merged(domain, |p| if let Patch::Respawn(p) = p { Some(p) } else { None });
            let retail = RespawnSettings::default();
            let s = RespawnSettings {
                air_timeout_ticks: p.air_timeout_ticks.map_or(retail.air_timeout_ticks, |t| t as i32),
            };
            match world.get_resource_mut::<RespawnSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "ghost" => {
            let p = t.merged(domain, |p| if let Patch::Ghost(p) = p { Some(p) } else { None });
            let retail = skate_core::player::ghost::GhostSettings::default();
            let s = GhostTuning(skate_core::player::ghost::GhostSettings {
                enabled: p.enabled.unwrap_or(retail.enabled),
                fade_in_seconds: p.fade_in_seconds.unwrap_or(retail.fade_in_seconds),
                hold_alpha: p.hold_alpha.unwrap_or(retail.hold_alpha),
            });
            match world.get_resource_mut::<GhostTuning>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "decals" => {
            let p = t.merged(domain, |p| if let Patch::Decals(p) = p { Some(p) } else { None });
            let s = DecalSettings { opacity: p.opacity.unwrap_or(DecalSettings::default().opacity) };
            match world.get_resource_mut::<DecalSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "exposure" => {
            let p = t.merged(domain, |p| if let Patch::Exposure(p) = p { Some(p) } else { None });
            let retail = ExposureMeter::default();
            let s = ExposureMeter {
                weights: p.meter_weights.map_or(retail.weights, Vec3::from_array),
                scale: p.meter_scale.unwrap_or(retail.scale),
            };
            match world.get_resource_mut::<ExposureMeter>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "prop_sounds" => {
            let p = t.merged(domain, |p| if let Patch::PropSounds(p) = p { Some(p) } else { None });
            let s = crate::game_audio::prop_sounds::PropSoundSettings { disabled: p.enabled == Some(false), types: p.types };
            match world.get_resource_mut::<crate::game_audio::prop_sounds::PropSoundSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        "video_screens" => {
            let p = t.merged(domain, |p| if let Patch::VideoScreens(p) = p { Some(p) } else { None });
            let d = VideoScreenSettings::default();
            let s = VideoScreenSettings {
                enabled: p.enabled.unwrap_or(d.enabled),
                size: UVec2::new(p.width.unwrap_or(d.size.x), p.height.unwrap_or(d.size.y)),
                rate_hz: p.rate_hz.unwrap_or(d.rate_hz),
                max_distance: p.max_distance.unwrap_or(d.max_distance),
            };
            match world.get_resource_mut::<VideoScreenSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        _ => {}
    }
}

/// Shipped values (player's choices kept) plus the patch. Negative times / alphas clamp to 0.
pub(crate) fn apply_living_world(s: &mut LivingWorldSettings, p: &LivingWorldPatch) {
    s.reset_mod_overrides();
    if let Some(m) = p.npc_draw_distance {
        s.npc_draw_distance = skate_core::living_world::DrawDistance::new(m).multiplier();
    }
    if let Some(v) = p.npc_grind_disttocog {
        s.npc_grind_disttocog = Some(v.clamp(0.0, 1.0));
    }
    if let Some(f) = &p.skater_fade {
        let c = &mut s.skater_fade;
        c.fade_in_seconds = f.fade_in_seconds.map_or(c.fade_in_seconds, |v| v.max(0.0));
        c.fade_seconds = f.fade_seconds.map_or(c.fade_seconds, |v| v.max(0.0));
        c.despawn_alpha = f.despawn_alpha.map_or(c.despawn_alpha, |v| v.clamp(0.0, 1.0));
        c.bail_fade = f.bail_fade.unwrap_or(c.bail_fade);
        c.bail_fade_lead_seconds = f.bail_fade_lead_seconds.map_or(c.bail_fade_lead_seconds, |v| v.max(0.0));
    }
    if let Some(f) = &p.skater_line_chain {
        let c = &mut s.skater_line_chain;
        c.radius = f.radius.map_or(c.radius, |v| v.max(0.0));
        c.max_candidates = f.max_candidates.map_or(c.max_candidates, |v| v as usize);
        c.blend_seconds = f.blend_seconds.map_or(c.blend_seconds, |v| v.max(0.0));
        c.keep_facing = f.keep_facing.unwrap_or(c.keep_facing);
        c.facing_rule = f.facing_rule.as_deref().and_then(skate_core::living_world::replay::FacingRule::from_name).unwrap_or(c.facing_rule);
        c.steer_dead_zone_deg = f.steer_dead_zone_deg.map_or(c.steer_dead_zone_deg, |v| v.max(0.0));
        c.steer_full_deg = f.steer_full_deg.map_or(c.steer_full_deg, |v| v.max(0.0));
        c.fakie.high_speed = f.fakie_high_speed.map_or(c.fakie.high_speed, |v| v.max(0.0));
        c.fakie.low_speed = f.fakie_low_speed.map_or(c.fakie.low_speed, |v| v.max(0.0));
        c.fakie.slowly_backwards_seconds = f.fakie_slow_seconds.map_or(c.fakie.slowly_backwards_seconds, |v| v.max(0.0));
        c.fakie.after_teleport_seconds = f.fakie_spawn_seconds.map_or(c.fakie.after_teleport_seconds, |v| v.max(0.0));
        c.fallback_valid_d2 = f.fallback_valid_m.map_or(c.fallback_valid_d2, |v| v.max(0.0) * v.max(0.0));
    }
    if let Some(f) = &p.ped_fade {
        let c = &mut s.ped_fade;
        c.distance = f.distance.map_or(c.distance, |[a, b]| [a.max(0.0), b.max(0.0)]);
        c.fade_in_seconds = f.fade_in_seconds.map_or(c.fade_in_seconds, |v| v.max(0.0));
        c.enabled = f.enabled.unwrap_or(c.enabled);
    }
    if let Some(f) = &p.ped_obstacles {
        let c = &mut s.ped_obstacles;
        c.enabled = f.enabled.unwrap_or(c.enabled);
        c.min_half_extent = f.min_half_extent.map_or(c.min_half_extent, |v| v.max(0.0));
        c.moving_speed = f.moving_speed.map_or(c.moving_speed, |v| v.max(0.0));
        c.recut_fraction = f.recut_fraction.map_or(c.recut_fraction, |v| v.max(0.0));
        c.detour_margin = f.detour_margin.map_or(c.detour_margin, |v| v.max(0.0));
        c.target_snap = f.target_snap.map_or(c.target_snap, |v| v.max(0.0));
        c.step_height = f.step_height.map_or(c.step_height, |v| v.max(0.0));
        c.held_is_obstacle = f.held_is_obstacle.unwrap_or(c.held_is_obstacle);
        c.moving_solid = f.moving_solid.unwrap_or(c.moving_solid);
        c.skaters = f.skaters.unwrap_or(c.skaters);
        c.skater_half_extents = f.skater_half_extents.map_or(c.skater_half_extents, |v| v.map(|x| x.max(0.0)));
        c.contact_slide = f.contact_slide.map_or(c.contact_slide, |v| v.max(0.0));
        c.contact_slide_dot = f.contact_slide_dot.map_or(c.contact_slide_dot, |v| v.clamp(-1.0, 1.0));
        c.avoid_horizon = f.avoid_horizon.map_or(c.avoid_horizon, |v| v.max(0.0));
        c.avoid_ray_scale = f.avoid_ray_scale.map_or(c.avoid_ray_scale, |v| v.max(0.0));
        c.avoid_accel = f.avoid_accel.map_or(c.avoid_accel, |v| v.max(0.0));
        c.avoid_scale = f.avoid_scale.map_or(c.avoid_scale, |v| v.max(0.0));
        c.avoid_head_on_dot = f.avoid_head_on_dot.map_or(c.avoid_head_on_dot, |v| v.clamp(-1.0, 1.0));
        c.avoid_crossing_dot = f.avoid_crossing_dot.map_or(c.avoid_crossing_dot, |v| v.clamp(-1.0, 1.0));
        c.avoider_extent = f.avoider_extent.map_or(c.avoider_extent, |v| v.max(0.0));
        c.avoider_extent_tail = f.avoider_extent_tail.map_or(c.avoider_extent_tail, |v| v.max(0.0));
        for (to, from) in [(&mut c.path_turn_accel, f.path_turn_accel), (&mut c.path_turn_full_deg, f.path_turn_full_deg), (&mut c.path_turn_boost_deg, f.path_turn_boost_deg), (&mut c.path_turn_boost_rate, f.path_turn_boost_rate), (&mut c.path_turn_boost_max, f.path_turn_boost_max), (&mut c.path_turn_radius, f.path_turn_radius), (&mut c.controller_step, f.controller_step), (&mut c.contact_slow_fraction, f.contact_slow_fraction), (&mut c.contact_max_speed, f.contact_max_speed), (&mut c.brake_decel, f.brake_decel), (&mut c.brake_goal_radius, f.brake_goal_radius), (&mut c.brake_body_radius, f.brake_body_radius), (&mut c.brake_away_fraction, f.brake_away_fraction)] {
            *to = from.map_or(*to, |v| v.max(0.0));
        }
        c.path_turn_max_deg = f.path_turn_max_deg.map_or(c.path_turn_max_deg, |v| v.clamp(0.0, 180.0));
    }
    if let Some(v) = p.on_foot_vehicle_ground {
        s.on_foot_vehicle_ground = v;
    }
    if let Some(f) = &p.ped_vehicle_contact {
        let c = &mut s.ped_vehicle_contact;
        c.enabled = f.enabled.unwrap_or(c.enabled);
        c.push = f.push.unwrap_or(c.push);
    }
    if let Some(f) = &p.npc_skater_props {
        s.npc_skater_props.enabled = f.enabled.unwrap_or(s.npc_skater_props.enabled);
    }
    if let Some(f) = &p.ped_nav {
        let n = &mut s.ped_nav;
        let w = &mut n.wander;
        w.steer_rate = f.steer_rate.map_or(w.steer_rate, f32::to_radians);
        w.turn_edges = f.turn_edges.unwrap_or(w.turn_edges);
        w.stall_seconds = f.stall_seconds.unwrap_or(w.stall_seconds);
        w.stuck_distance = f.stuck_distance.unwrap_or(w.stuck_distance);
        w.stuck_seconds = f.stuck_seconds.unwrap_or(w.stuck_seconds);
        w.give_up_seconds = f.give_up_seconds.unwrap_or(w.give_up_seconds);
        w.rewrite_range = f.rewrite_range.unwrap_or(w.rewrite_range);
        w.yield_patience = f.yield_patience.unwrap_or(w.yield_patience);
        use skate_core::living_world::peds::wander::CrosswalkRule;
        n.crosswalk = match f.crosswalk.as_deref() {
            Some("walk_signal") => CrosswalkRule::WalkSignal,
            Some("off") => CrosswalkRule::Off,
            _ => n.crosswalk,
        };
    }
    if let Some(f) = &p.ped_body {
        let b = &mut s.ped_body;
        b.enabled = f.enabled.unwrap_or(b.enabled);
        if let Some(parts) = &f.parts {
            b.parts = parts.iter().map(|p| crate::living_world::ped_body::PedBodyPart {
                from: p.from.clone(), to: p.to.clone(), centre: p.centre, radius: p.radius, half_length: p.half_length, mass: p.mass, friction: p.friction.unwrap_or(crate::living_world::ped_body::DEFAULT_PART_FRICTION),
            }).collect();
        }
        b.look_ahead = f.look_ahead.unwrap_or(b.look_ahead);
        b.range = f.range.unwrap_or(b.range);
        b.reaction_part = f.reaction_part.unwrap_or(b.reaction_part);
        let mut proxy = b.proxy.unwrap_or_default();
        proxy.radius = f.proxy_radius.unwrap_or(proxy.radius);
        proxy.half_length = f.proxy_half_length.unwrap_or(proxy.half_length);
        proxy.mass = f.proxy_mass.unwrap_or(proxy.mass);
        let on = f.proxy.unwrap_or(b.proxy.is_some());
        b.proxy = on.then_some(proxy);
    }
    if let Some(m) = &p.skater_clips {
        s.skater_clips = m.clone();
    }
    if let Some(m) = &p.skater_blend_seconds {
        s.skater_blend_seconds = m.iter().map(|(k, v)| (k.clone(), v.max(0.0))).collect();
    }
    if let Some(m) = &p.skater_stance {
        use skate_core::living_world::stance::NaturalStance;
        s.skater_stance = m.iter().filter_map(|(k, v)| NaturalStance::parse(v).map(|v| (k.clone(), v))).collect();
    }
    if let Some(m) = &p.skater_stance_events {
        s.skater_stance_events = m.clone();
    }
    if let Some(f) = &p.npc_tricks {
        let t = &mut s.npc_tricks;
        t.mode = f.mode.as_deref().and_then(skate_core::living_world::npc_tricks::TrickMode::from_name).unwrap_or(t.mode);
        t.params.gate_window = f.gate_window.unwrap_or(t.params.gate_window);
        t.params.min_air_frames = f.min_air_frames.unwrap_or(t.params.min_air_frames);
    }
    for (patch, kind) in [(&p.skaters, &mut s.skaters), (&p.pedestrians, &mut s.pedestrians), (&p.vehicles, &mut s.vehicles)] {
        if let Some(f) = patch {
            kind.enabled = f.enabled.unwrap_or(kind.enabled);
            kind.density = f.density.unwrap_or(kind.density);
        }
    }
    if let Some(n) = p.ambient_skaters {
        s.ambient_skaters = n;
    }
    if let Some(z) = p.zombie {
        s.zombie = z;
    }
    if let Some(f) = &p.free_play {
        let d = skate_core::living_world::FreePlay::default();
        s.free_play = Some(skate_core::living_world::FreePlay {
            traffic: f.traffic.unwrap_or(d.traffic),
            pedestrians: f.pedestrians.unwrap_or(d.pedestrians),
            ai_skaters: f.ai_skaters.unwrap_or(d.ai_skaters),
        });
    }
    if let Some(f) = &p.npc_simulated {
        let n = &mut s.npc_simulated;
        n.enabled = f.enabled.unwrap_or(n.enabled);
        n.radius = f.radius.unwrap_or(n.radius);
        n.max = f.max.map_or(n.max, |m| m as usize);
        n.respawn_seconds = f.respawn_seconds.unwrap_or(n.respawn_seconds);
        n.respawn_min = f.respawn_min.unwrap_or(n.respawn_min);
        n.respawn_max = f.respawn_max.unwrap_or(n.respawn_max);
        n.placement_hold_seconds = f.placement_hold_seconds.unwrap_or(n.placement_hold_seconds);
        n.respawn_hold_seconds = f.respawn_hold_seconds.unwrap_or(n.respawn_hold_seconds);
        n.signals.anticipation_distance = f.anticipation_distance.unwrap_or(n.signals.anticipation_distance);
        n.signals.anticipation_frames = f.anticipation_frames.unwrap_or(n.signals.anticipation_frames);
        n.signals.max_crossed_nodes = f.max_crossed_nodes.unwrap_or(n.signals.max_crossed_nodes);
        n.walk_back = f.walk_back.unwrap_or(n.walk_back);
        n.navmesh.arrive_h = f.walk_arrive_distance.unwrap_or(n.navmesh.arrive_h);
        n.navmesh.stuck_waypoint_ticks = f.walk_stuck_ticks.unwrap_or(n.navmesh.stuck_waypoint_ticks);
        n.board_yaw_on = f.board_yaw.unwrap_or(n.board_yaw_on);
        n.board_yaw.dead_zone = f.board_yaw_dead_zone.unwrap_or(n.board_yaw.dead_zone);
        n.board_yaw.mag_scale = f.board_yaw_mag_scale.unwrap_or(n.board_yaw.mag_scale);
        n.air_steer_on = f.air_steer.unwrap_or(n.air_steer_on);
        n.air_steer.dead_zone_deg = f.air_steer_dead_zone_deg.unwrap_or(n.air_steer.dead_zone_deg);
        n.air_steer.full_deg = f.air_steer_full_deg.unwrap_or(n.air_steer.full_deg);
        n.ground_steer.avoid_anchor_ticks = f.avoid_anchor_ticks.unwrap_or(n.ground_steer.avoid_anchor_ticks);
        n.signals.manual_value = f.manual_value.unwrap_or(n.signals.manual_value);
        n.bump = f.bump.unwrap_or(n.bump);
        n.reactions.enabled = f.reactions.unwrap_or(n.reactions.enabled);
        n.reactions.radius = f.reaction_radius.unwrap_or(n.reactions.radius);
        let t = &mut n.push;
        t.enabled = f.push.unwrap_or(t.enabled);
        t.factor = f.push_factor.unwrap_or(t.factor);
        t.reduce = f.push_reduce.unwrap_or(t.reduce);
        t.fast_speed = f.push_fast_speed.unwrap_or(t.fast_speed);
        t.downhill_slope = f.push_downhill_slope.unwrap_or(t.downhill_slope);
        t.max_heading_error = f.push_max_heading_deg.map_or(t.max_heading_error, f32::to_radians);
        t.right_foot = f.push_right_foot.map(Some).unwrap_or(t.right_foot);
        t.auto_pump = f.auto_pump.unwrap_or(t.auto_pump);
        let b = &mut n.brake;
        b.enabled = f.brake.unwrap_or(b.enabled);
        b.factor = f.brake_factor.unwrap_or(b.factor);
        b.ramp = f.brake_ramp.unwrap_or(b.ramp);
    }
    // After `npc_simulated`: the nested keys win over its walk-back aliases.
    macro_rules! set {
        ($to:expr, $from:expr; $($f:ident),*) => { $( if let Some(v) = $from.$f { $to.$f = v; } )* };
    }
    if let Some(f) = &p.npc_walk_back {
        set!(s.npc_simulated.navmesh, f; plan_path_h, plan_path_v, goal_locate_radius, start_locate_radius, plan_direct_h, plan_direct_v, arrive_h, arrive_v, arrive_heading, line_up_h, line_up_v, leg_h, leg_v, leg_epsilon_sq, steer_full_lock, steer_gain, throttle_gain, slow_along, slow_gain, throttle_floor, floor_along, stall_steer, magnitude_cap, move_step_sq, move_anchor_sq, turn_step, turn_anchor, stall_ticks, push_period, stuck_waypoint_ticks, stuck_stall_ticks, stall_recovery_ticks);
    }
    if let Some(f) = &p.skater_path {
        set!(s.npc_simulated.path, f; line_end_t, lost_radius, line_end_lost_radius, lost_max_speed);
    }
    if let Some(f) = &p.npc_line_controller {
        let c = &mut s.npc_simulated.controller;
        set!(c, f; wipe_out, safe_slope, stuck_clear_distance, stuck_start_fraction, stuck_start_approach, reposition_ahead, reposition_past, remembered_window, safe_jump_frames, safe_flag_frames, safe_run, stuck_clear_ticks, stuck_hold_ticks, stuck_gate_ticks, resnap_frames, stuck_limits);
        if let Some(v) = f.reposition_interval {
            c.reposition_interval = u64::from(v);
        }
    }
    if let Some(m) = &p.traffic_horn {
        s.traffic_horn = m.clone();
    }
    if let Some(f) = &p.ped_brain {
        let b = &mut s.ped_brain;
        b.enabled = f.enabled.unwrap_or(b.enabled);
        b.mood = f.mood.unwrap_or(b.mood);
        b.values.wander_speed = f.wander_speed.unwrap_or(b.values.wander_speed);
        b.values.warn_seconds = f.warn_seconds.unwrap_or(b.values.warn_seconds);
        b.values.know_about_seconds = f.know_about_seconds.unwrap_or(b.values.know_about_seconds);
        b.values.mood_want_memory_seconds = f.mood_want_memory_seconds.unwrap_or(b.values.mood_want_memory_seconds);
        b.values.conversation_turn_seconds = f.conversation_turn_seconds.unwrap_or(b.values.conversation_turn_seconds);
        b.values.conversation_gather_seconds = f.conversation_gather_seconds.unwrap_or(b.values.conversation_gather_seconds);
        b.values.unreachable_radius = f.unreachable_radius.unwrap_or(b.values.unreachable_radius);
        b.values.reach_snap_window = f.reach_snap_window.unwrap_or(b.values.reach_snap_window);
        b.values.scatter_seconds = f.scatter_seconds.unwrap_or(b.values.scatter_seconds);
        b.values.scatter_arrival_radius = f.scatter_arrival_radius.unwrap_or(b.values.scatter_arrival_radius);
        b.values.scatter_channel_fade = f.scatter_channel_fade.unwrap_or(b.values.scatter_channel_fade);
        b.scatter_hold = f.scatter_hold.unwrap_or(b.scatter_hold);
        b.values.reach_check_seconds = f.reach_check_seconds.unwrap_or(b.values.reach_check_seconds);
        b.values.warn_speech = f.warn_speech.unwrap_or(b.values.warn_speech);
        b.run_from_honker.side_distance = f.run_from_honker_distance.unwrap_or(b.run_from_honker.side_distance);
        b.run_from_honker.run_speed = f.run_from_honker_speed.unwrap_or(b.run_from_honker.run_speed);
        b.object_flee.enabled = f.object_flee.unwrap_or(b.object_flee.enabled);
        b.object_flee.radius = f.object_flee_radius.unwrap_or(b.object_flee.radius);
        b.object_flee.min_speed = f.object_flee_min_speed.unwrap_or(b.object_flee.min_speed);
        b.object_flee.lane_margin = f.object_flee_lane_margin.unwrap_or(b.object_flee.lane_margin);
        b.vehicle_flee.enabled = f.vehicle_flee.unwrap_or(b.vehicle_flee.enabled);
        b.vehicle_flee.radius = f.vehicle_flee_radius.unwrap_or(b.vehicle_flee.radius);
        b.vehicle_flee.min_dot = f.vehicle_flee_min_dot.unwrap_or(b.vehicle_flee.min_dot);
        b.vehicle_flee.away = f.vehicle_flee_away.unwrap_or(b.vehicle_flee.away);
        b.vehicle_flee.toward = f.vehicle_flee_toward.unwrap_or(b.vehicle_flee.toward);
        b.vehicle_flee.require_on_road = f.vehicle_flee_require_on_road.unwrap_or(b.vehicle_flee.require_on_road);
        b.vehicle_flee.require_flagged = f.vehicle_flee_require_flagged.unwrap_or(b.vehicle_flee.require_flagged);
        b.flee.leg = f.flee_leg.unwrap_or(b.flee.leg);
        b.flee.arrival_radius = f.flee_arrival_radius.unwrap_or(b.flee.arrival_radius);
        b.flee.threat_speed_gate = f.flee_threat_speed_gate.unwrap_or(b.flee.threat_speed_gate);
        b.flee.side_blend = f.flee_side_blend.unwrap_or(b.flee.side_blend);
        b.flee.releg_on_nav_done = f.flee_releg_on_nav_done.unwrap_or(b.flee.releg_on_nav_done);
        b.plugins.water_disables_seats = f.water_disables_seats.unwrap_or(b.plugins.water_disables_seats);
        b.plugins.water_max_depth = f.water_max_depth.unwrap_or(b.plugins.water_max_depth);
        b.plugins.water_above = f.water_above.unwrap_or(b.plugins.water_above);
        let h = &mut b.values.hand_prop;
        h.attack_speed = f.attack_throw_speed.unwrap_or(h.attack_speed);
        h.attack_lead_seconds = f.attack_throw_lead_seconds.unwrap_or(h.attack_lead_seconds);
        h.jitter_max = f.attack_throw_jitter.unwrap_or(h.jitter_max);
        h.aim_lift = f.attack_throw_lift.unwrap_or(h.aim_lift);
        h.light_speed = f.light_throw_speed.unwrap_or(h.light_speed);
        h.skater_contact = f.hand_prop_skater_contact.unwrap_or(h.skater_contact);
        h.starting_props = f.starting_hand_props.unwrap_or(h.starting_props);
        let z = &mut b.values.zombie_follow;
        z.follow_distance = f.zombie_follow_distance.unwrap_or(z.follow_distance);
        z.sprint_distance = f.zombie_sprint_distance.unwrap_or(z.sprint_distance);
        z.ring_min = f.zombie_ring_min.unwrap_or(z.ring_min);
        z.ring_max = f.zombie_ring_max.unwrap_or(z.ring_max);
        z.sprint_speed = f.zombie_sprint_speed.unwrap_or(z.sprint_speed);
        z.walk_speed = f.zombie_walk_speed.unwrap_or(z.walk_speed);
        z.arrive_distance = f.zombie_arrive_distance.unwrap_or(z.arrive_distance);
        use skate_core::living_world::peds::skater_contact::{ReactionDirection, ReactionKind};
        b.values.trip_kind = f.trip_kind.as_deref().and_then(ReactionKind::from_name).unwrap_or(b.values.trip_kind);
        b.values.trip_direction = f.trip_direction.as_deref().and_then(ReactionDirection::from_name).unwrap_or(b.values.trip_direction);
        let a = &mut b.values.avoid;
        a.sense_radius = f.skater_avoid_sense.unwrap_or(a.sense_radius);
        a.min_speed = f.skater_avoid_min_speed.unwrap_or(a.min_speed);
        a.look_ahead = f.skater_avoid_look_ahead.unwrap_or(a.look_ahead);
        a.avoid_radius = f.skater_avoid_radius.unwrap_or(a.avoid_radius);
        a.jump_radius = f.skater_avoid_jump_radius.unwrap_or(a.jump_radius);
        a.jump_speed = f.skater_avoid_jump_speed.unwrap_or(a.jump_speed);
        a.sector_half_angle = f.skater_avoid_sector.unwrap_or(a.sector_half_angle);
        a.period = f.skater_avoid_period.unwrap_or(a.period).max(1.0 / 60.0);
        a.standing = f.skater_avoid_standing.unwrap_or(a.standing);
    }
    if let Some(f) = &p.npc_avoid {
        let a = &mut s.npc_avoid;
        a.enabled = f.enabled.unwrap_or(a.enabled);
        a.max_entries = f.max_entries.map_or(a.max_entries, |n| n as usize);
        a.skitch_cooldown_ticks = f.skitch_cooldown_ticks.unwrap_or(a.skitch_cooldown_ticks);
        a.radius_skater = f.radius_skater.unwrap_or(a.radius_skater);
        a.radius_pedestrian = f.radius_pedestrian.unwrap_or(a.radius_pedestrian);
        a.radius_vehicle = f.radius_vehicle.unwrap_or(a.radius_vehicle);
        a.radius_prop = f.radius_prop.unwrap_or(a.radius_prop);
        a.cone = f.cone.unwrap_or(a.cone);
        a.wide_cone = f.wide_cone.unwrap_or(a.wide_cone);
        a.wide_cone_distance = f.wide_cone_distance.unwrap_or(a.wide_cone_distance);
        a.skater_radius = f.skater_radius.unwrap_or(a.skater_radius);
        a.speed_margin = f.speed_margin.unwrap_or(a.speed_margin);
        a.stop_gap = f.stop_gap.unwrap_or(a.stop_gap);
        a.stop_gap_far = f.stop_gap_far.unwrap_or(a.stop_gap_far);
        a.stop_cone = f.stop_cone.unwrap_or(a.stop_cone);
        a.stop_gap_prop = f.stop_gap_prop.unwrap_or(a.stop_gap_prop);
        a.side_on_angle = f.side_on_angle.unwrap_or(a.side_on_angle);
        a.floor_headroom = f.floor_headroom.unwrap_or(a.floor_headroom);
        a.skitch_cos = f.skitch_cos.unwrap_or(a.skitch_cos);
        a.low_prop_height = f.low_prop_height.unwrap_or(a.low_prop_height);
        a.low_prop_time = f.low_prop_time.unwrap_or(a.low_prop_time);
        a.step_off_cap = f.step_off_cap.unwrap_or(a.step_off_cap);
        a.step_off_time = f.step_off_time.unwrap_or(a.step_off_time);
    }
    if let Some(m) = &p.skater_trick_profiles {
        let table = |t: &Option<Vec<skate_mods::world_tuning::TrickWeight>>| t.as_ref().map(|t| t.iter().map(|e| (e.trick, e.weight.max(0.0))).collect());
        s.skater_trick_profiles = m.iter().map(|(k, v)| (k.clone(), crate::living_world::npc_skaters::NpcTrickTables { regular: table(&v.regular), nollie: table(&v.nollie) })).collect();
    }
}

fn prop_tuning(base: &PropTuning, p: &PropTuningPatch) -> PropTuning {
    let f = |v: Option<f32>, d: f32| v.map_or(d, |v| v.max(0.0));
    PropTuning {
        contact_padding: f(p.contact_padding, base.contact_padding),
        penetration_slop: f(p.penetration_slop, base.penetration_slop),
        penetration_correction: f(p.penetration_correction, base.penetration_correction),
        max_depenetration_per_tick: f(p.max_depenetration_per_tick, base.max_depenetration_per_tick),
        restitution_threshold: f(p.restitution_threshold, base.restitution_threshold),
        skater_push_mass: f(p.skater_push_mass, base.skater_push_mass),
        push_transfer: f(p.push_transfer, base.push_transfer),
        body_push_speed: f(p.body_push_speed, base.body_push_speed),
        board_push_speed: f(p.board_push_speed, base.board_push_speed),
        penetration_push_speed: f(p.penetration_push_speed, base.penetration_push_speed),
        stuck_release_ticks: p.stuck_release_ticks.unwrap_or(base.stuck_release_ticks),
        collision_box: p.collision_box.map_or(base.collision_box, |b| {
            Some(PropBox { center: Vector3::new(b.center[0], b.center[1], b.center[2]), half_extents: Vector3::new(b.half_extents[0], b.half_extents[1], b.half_extents[2]) })
        }),
        water_density: p.water_density.or(base.water_density),
    }
}

/// Shipped table plus the patch; a template entry starts from the (patched) default.
pub(crate) fn props_table(p: &PropsPatch) -> PropTuningTable {
    let mut t = PropTuningTable::default();
    if let Some(d) = &p.default {
        t.default = prop_tuning(&t.default, d);
    }
    for (name, patch) in &p.by_template {
        let entry = prop_tuning(&t.default, patch);
        t.by_template.insert(name.clone(), entry);
    }
    if let Some(v) = &p.solver {
        let d = t.solver;
        t.solver = crate::physics::prop_dynamics::PropSolverSettings {
            row_solver: v.row_solver.unwrap_or(d.row_solver),
            iterations: v.iterations.map_or(d.iterations, |n| n.clamp(1, 256)),
            sleep_energy: v.sleep_energy.map_or(d.sleep_energy, |e| e.max(0.0)),
            sleep_frames: v.sleep_frames.map_or(d.sleep_frames, |n| n.max(1)),
            max_sleeps_per_step: v.max_sleeps_per_step.map_or(d.max_sleeps_per_step, |n| n.max(1)),
            rest_snap: v.rest_snap.unwrap_or(d.rest_snap),
            bail_clamp: v.bail_clamp.unwrap_or(d.bail_clamp),
            bail_linear_factor: v.bail_linear_factor.unwrap_or(d.bail_linear_factor),
            bail_angular_factor: v.bail_angular_factor.unwrap_or(d.bail_angular_factor),
            bail_heavy_mass: v.bail_heavy_mass.unwrap_or(d.bail_heavy_mass),
            bail_blend: v.bail_blend.unwrap_or(d.bail_blend),
            walkable_mass: v.walkable_mass.unwrap_or(d.walkable_mass),
            skater_push_box: v.skater_push_box.unwrap_or(d.skater_push_box),
            skater_rows: v.skater_rows.unwrap_or(d.skater_rows),
            board_row_mass: v.board_row_mass.unwrap_or(d.board_row_mass),
            ground_reduction: skate_core::physics::contact_reduction::ReductionRules {
                enabled: v.ground_reduction.unwrap_or(d.ground_reduction.enabled),
                normal_dot: v.reduction_normal_dot.map_or(d.ground_reduction.normal_dot, |x| x.clamp(-1.0, 1.0)),
                plane_distance: v.reduction_plane_distance.map_or(d.ground_reduction.plane_distance, |x| x.max(0.0)),
                chunk: v.reduction_chunk.map_or(d.ground_reduction.chunk, |n| n.clamp(1, 4096) as usize),
            },
        };
    }
    if let Some(v) = &p.upright {
        let d = t.upright;
        t.upright = crate::physics::prop_dynamics::PropUprightSettings {
            window_seconds: v.window_seconds.unwrap_or(d.window_seconds),
            tick_seconds: v.tick_seconds.unwrap_or(d.tick_seconds),
            stop_angle_deg: v.stop_angle_deg.unwrap_or(d.stop_angle_deg),
            max_angle_deg: v.max_angle_deg.unwrap_or(d.max_angle_deg),
            dead_band_deg: v.dead_band_deg.unwrap_or(d.dead_band_deg),
            gain_min: v.gain_min.unwrap_or(d.gain_min),
            gain_max: v.gain_max.unwrap_or(d.gain_max),
            gain_blend_start: v.gain_blend_start.unwrap_or(d.gain_blend_start),
            off_axis_spin: v.off_axis_spin.unwrap_or(d.off_axis_spin),
            command_rate: v.command_rate.unwrap_or(d.command_rate),
            fallback_angle_deg: v.fallback_angle_deg.unwrap_or(d.fallback_angle_deg),
            block_yaw: v.block_yaw.unwrap_or(d.block_yaw),
        };
    }
    t
}

pub(crate) fn carry_settings(p: &CarryPatch) -> CarrySettings {
    let d = CarrySettings::default();
    CarrySettings {
        buttons: CarryButtons { grab_bit: p.grab_bit.unwrap_or(d.buttons.grab_bit), placement_bit: p.placement_bit.unwrap_or(d.buttons.placement_bit) },
        grab_range: p.grab_range.filter(|r| *r > 0.0).unwrap_or(d.grab_range),
        locomotion: LocomotionOverrides {
            push_speed: p.push_speed,
            pull_speed: p.pull_speed,
            side_speed: p.side_speed,
            turn_rate: p.turn_rate,
            grip_reach: p.grip_reach,
            linear_clamp: p.linear_clamp,
            yaw_clamp: p.yaw_clamp,
            relatch: p.relatch,
            slew_per_tick: p.slew_per_tick,
            yaw_rate_feedback: p.yaw_rate_feedback,
            linear_controller: p.linear_controller,
            yaw_controller: p.yaw_controller,
            lever_rotation: p.lever_rotation,
            lever_yaw: p.lever_yaw,
            mass_speed: p.mass_speed,
            inertia_yaw_gain: p.inertia_yaw_gain,
            let_go_distance: p.let_go_distance,
            drop_board: p.drop_board,
            follow_step: p.follow_step,
            hold_angle_limit: p.hold_angle_limit,
            hold_max_angle_to_horizontal: p.hold_max_angle_to_horizontal,
            hold_box_extents: p.hold_box_extents,
            record_272_speed_scale: p.record_272_speed_scale,
            grab_end_exclusion: p.grab_end_exclusion,
            hand_ik_enter: p.hand_ik_enter,
            hand_ik_curve: p.hand_ik_curve,
            root_yaw_gain: p.root_yaw_gain,
            hand_ik_rate: p.hand_ik_rate,
            hand_ik_reach: p.hand_ik_reach,
        },
        move_rules: {
            let r = crate::physics::prop_dynamics::MoveCommandRules::default();
            crate::physics::prop_dynamics::MoveCommandRules {
                commanded_material: p.commanded_material.unwrap_or(r.commanded_material),
                upright_cos: p.upright_cos.unwrap_or(r.upright_cos),
                by_template: p
                    .by_template
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            crate::physics::prop_dynamics::PropMaterialBlocks {
                                held: v.material_held,
                                free: v.material_free,
                                free_upright: v.material_free_upright,
                                upright_pair: v.upright_pair,
                                restitution: v.restitution,
                                record_272: v.record_272,
                                linear_drag: v.linear_drag,
                                angular_drag: v.angular_drag,
                                mass: v.mass,
                                maximum_linear_velocity: v.maximum_linear_velocity,
                                maximum_angular_velocity: v.maximum_angular_velocity,
                                inertia_scale: v.inertia_scale,
                                inertia_offset: v.inertia_offset,
                                plugin_rotation_tolerance: v.plugin_rotation_tolerance,
                                plugin_max_distance: v.plugin_max_distance,
                            },
                        )
                    })
                    .collect(),
                apply_at_com: p.apply_at_com.unwrap_or(r.apply_at_com),
                yaw_replaces_torque: p.yaw_replaces_torque.unwrap_or(r.yaw_replaces_torque),
                ignore_vertical: p.ignore_vertical.unwrap_or(r.ignore_vertical),
                wake_on_command: p.wake_on_command.unwrap_or(r.wake_on_command),
            }
        },
    }
}

/// `sdk.engine.inspect(key, 'world_tuning:<domain>')`: the domain as the game uses it now.
pub(crate) fn read(world: &World, domain: &str) -> Value {
    let v3 = |v: Vector3| json!([v.x, v.y, v.z]);
    let tuning = |t: &PropTuning| {
        json!({
            "contact_padding": t.contact_padding, "penetration_slop": t.penetration_slop,
            "penetration_correction": t.penetration_correction, "max_depenetration_per_tick": t.max_depenetration_per_tick,
            "restitution_threshold": t.restitution_threshold, "skater_push_mass": t.skater_push_mass,
            "push_transfer": t.push_transfer, "body_push_speed": t.body_push_speed, "board_push_speed": t.board_push_speed,
            "penetration_push_speed": t.penetration_push_speed, "stuck_release_ticks": t.stuck_release_ticks,
            "collision_box": t.collision_box.map_or(Value::Null, |b| json!({"center": v3(b.center), "half_extents": v3(b.half_extents)})),
            "water_density": t.water_density,
        })
    };
    match domain {
        "living_world" => world.get_resource::<LivingWorldSettings>().map_or(Value::Null, |s| {
            // Separate (the json! macro's recursion limit).
            let npc_avoid = json!({"enabled": s.npc_avoid.enabled, "max_entries": s.npc_avoid.max_entries, "skitch_cooldown_ticks": s.npc_avoid.skitch_cooldown_ticks, "radius_skater": s.npc_avoid.radius_skater, "radius_pedestrian": s.npc_avoid.radius_pedestrian, "radius_vehicle": s.npc_avoid.radius_vehicle, "radius_prop": s.npc_avoid.radius_prop, "cone": s.npc_avoid.cone, "wide_cone": s.npc_avoid.wide_cone, "wide_cone_distance": s.npc_avoid.wide_cone_distance, "skater_radius": s.npc_avoid.skater_radius, "speed_margin": s.npc_avoid.speed_margin, "stop_gap": s.npc_avoid.stop_gap, "stop_gap_far": s.npc_avoid.stop_gap_far, "stop_cone": s.npc_avoid.stop_cone, "stop_gap_prop": s.npc_avoid.stop_gap_prop, "side_on_angle": s.npc_avoid.side_on_angle, "floor_headroom": s.npc_avoid.floor_headroom, "skitch_cos": s.npc_avoid.skitch_cos, "low_prop_height": s.npc_avoid.low_prop_height, "low_prop_time": s.npc_avoid.low_prop_time, "step_off_cap": s.npc_avoid.step_off_cap, "step_off_time": s.npc_avoid.step_off_time});
            macro_rules! fields {
                ($from:expr; $($f:ident),*) => {{
                    let mut m = serde_json::Map::new();
                    $( m.insert(stringify!($f).into(), json!($from.$f)); )*
                    serde_json::Value::Object(m)
                }};
            }
            let nb = &s.npc_simulated.navmesh;
            let npc_walk_back = fields!(nb; plan_path_h, plan_path_v, goal_locate_radius, start_locate_radius, plan_direct_h, plan_direct_v, arrive_h, arrive_v, arrive_heading, line_up_h, line_up_v, leg_h, leg_v, leg_epsilon_sq, steer_full_lock, steer_gain, throttle_gain, slow_along, slow_gain, throttle_floor, floor_along, stall_steer, magnitude_cap, move_step_sq, move_anchor_sq, turn_step, turn_anchor, stall_ticks, push_period, stuck_waypoint_ticks, stuck_stall_ticks, stall_recovery_ticks);
            let po = &s.ped_obstacles;
            let ped_obstacles = fields!(po; enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, target_snap, step_height, held_is_obstacle, moving_solid, skaters, skater_half_extents, contact_slide, contact_slide_dot, avoid_horizon, avoid_ray_scale, avoid_accel, avoid_scale, avoid_head_on_dot, avoid_crossing_dot, avoider_extent, avoider_extent_tail, path_turn_accel, path_turn_full_deg, path_turn_boost_deg, path_turn_boost_rate, path_turn_boost_max, path_turn_max_deg, path_turn_radius, controller_step, contact_slow_fraction, contact_max_speed, brake_decel, brake_goal_radius, brake_body_radius, brake_away_fraction);
            let lc = &s.npc_simulated.controller;
            let npc_line_controller = fields!(lc; wipe_out, safe_slope, stuck_clear_distance, stuck_start_fraction, stuck_start_approach, reposition_ahead, reposition_past, remembered_window, reposition_interval, safe_jump_frames, safe_flag_frames, safe_run, stuck_clear_ticks, stuck_hold_ticks, stuck_gate_ticks, resnap_frames, stuck_limits);
            let ped_brain = json!({"enabled": s.ped_brain.enabled, "mood": s.ped_brain.mood, "wander_speed": s.ped_brain.values.wander_speed, "warn_seconds": s.ped_brain.values.warn_seconds, "know_about_seconds": s.ped_brain.values.know_about_seconds, "mood_want_memory_seconds": s.ped_brain.values.mood_want_memory_seconds, "conversation_turn_seconds": s.ped_brain.values.conversation_turn_seconds, "conversation_gather_seconds": s.ped_brain.values.conversation_gather_seconds, "unreachable_radius": s.ped_brain.values.unreachable_radius, "reach_check_seconds": s.ped_brain.values.reach_check_seconds, "reach_snap_window": s.ped_brain.values.reach_snap_window, "scatter_seconds": s.ped_brain.values.scatter_seconds, "scatter_arrival_radius": s.ped_brain.values.scatter_arrival_radius, "scatter_channel_fade": s.ped_brain.values.scatter_channel_fade, "scatter_hold": s.ped_brain.scatter_hold, "run_from_honker_distance": s.ped_brain.run_from_honker.side_distance, "run_from_honker_speed": s.ped_brain.run_from_honker.run_speed, "warn_speech": s.ped_brain.values.warn_speech, "water_disables_seats": s.ped_brain.plugins.water_disables_seats, "water_max_depth": s.ped_brain.plugins.water_max_depth, "water_above": s.ped_brain.plugins.water_above});
            let npc_simulated = json!({"enabled": s.npc_simulated.enabled, "radius": s.npc_simulated.radius, "max": s.npc_simulated.max, "respawn_seconds": s.npc_simulated.respawn_seconds, "respawn_min": s.npc_simulated.respawn_min, "respawn_max": s.npc_simulated.respawn_max, "placement_hold_seconds": s.npc_simulated.placement_hold_seconds, "respawn_hold_seconds": s.npc_simulated.respawn_hold_seconds, "anticipation_distance": s.npc_simulated.signals.anticipation_distance, "anticipation_frames": s.npc_simulated.signals.anticipation_frames, "max_crossed_nodes": s.npc_simulated.signals.max_crossed_nodes, "walk_back": s.npc_simulated.walk_back, "walk_arrive_distance": s.npc_simulated.navmesh.arrive_h, "walk_stuck_ticks": s.npc_simulated.navmesh.stuck_waypoint_ticks, "board_yaw": s.npc_simulated.board_yaw_on, "board_yaw_dead_zone": s.npc_simulated.board_yaw.dead_zone, "manual_value": s.npc_simulated.signals.manual_value, "board_yaw_mag_scale": s.npc_simulated.board_yaw.mag_scale, "air_steer": s.npc_simulated.air_steer_on, "air_steer_dead_zone_deg": s.npc_simulated.air_steer.dead_zone_deg, "air_steer_full_deg": s.npc_simulated.air_steer.full_deg, "avoid_anchor_ticks": s.npc_simulated.ground_steer.avoid_anchor_ticks, "bump": s.npc_simulated.bump});
            let mut out = json!({
                "npc_walk_back": npc_walk_back,
                "npc_line_controller": npc_line_controller,
                "skater_path": fields!(s.npc_simulated.path; line_end_t, lost_radius, line_end_lost_radius, lost_max_speed),
                "npc_draw_distance": s.npc_draw_distance,
                "user_npc_draw_distance": s.user_npc_draw_distance,
                "npc_grind_disttocog": s.npc_grind_disttocog,
                "skater_fade": {"fade_in_seconds": s.skater_fade.fade_in_seconds, "fade_seconds": s.skater_fade.fade_seconds, "despawn_alpha": s.skater_fade.despawn_alpha, "bail_fade": s.skater_fade.bail_fade, "bail_fade_lead_seconds": s.skater_fade.bail_fade_lead_seconds},
                "skater_line_chain": {"radius": s.skater_line_chain.radius, "max_candidates": s.skater_line_chain.max_candidates, "blend_seconds": s.skater_line_chain.blend_seconds, "keep_facing": s.skater_line_chain.keep_facing, "facing_rule": s.skater_line_chain.facing_rule.name(), "steer_dead_zone_deg": s.skater_line_chain.steer_dead_zone_deg, "steer_full_deg": s.skater_line_chain.steer_full_deg, "fakie_high_speed": s.skater_line_chain.fakie.high_speed, "fakie_low_speed": s.skater_line_chain.fakie.low_speed, "fakie_slow_seconds": s.skater_line_chain.fakie.slowly_backwards_seconds, "fakie_spawn_seconds": s.skater_line_chain.fakie.after_teleport_seconds, "fallback_valid_m": s.skater_line_chain.fallback_valid_d2.sqrt()},
                "ped_fade": {"distance": s.ped_fade.distance, "fade_in_seconds": s.ped_fade.fade_in_seconds, "enabled": s.ped_fade.enabled},
                "ped_obstacles": ped_obstacles,
                "npc_skater_props": {"enabled": s.npc_skater_props.enabled},
                "ped_body": {"enabled": s.ped_body.enabled, "look_ahead": s.ped_body.look_ahead, "range": s.ped_body.range,
                    "reaction_part": s.ped_body.reaction_part,
                    "parts": s.ped_body.parts.iter().map(|p| json!({"from": p.from, "to": p.to, "centre": p.centre, "radius": p.radius, "half_length": p.half_length, "mass": p.mass, "friction": p.friction})).collect::<Vec<_>>()},
                "ped_vehicle_contact": {"enabled": s.ped_vehicle_contact.enabled, "push": s.ped_vehicle_contact.push},
                "on_foot_vehicle_ground": s.on_foot_vehicle_ground,
                "skater_clips": s.skater_clips,
                "skater_blend_seconds": s.skater_blend_seconds,
                "skater_stance": s.skater_stance.iter().map(|(k, v)| (k.clone(), v.name())).collect::<std::collections::BTreeMap<_, _>>(),
                "skater_stance_events": s.skater_stance_events,
                "skaters": {"enabled": s.skaters.enabled, "density": s.skaters.density},
                "pedestrians": {"enabled": s.pedestrians.enabled, "density": s.pedestrians.density},
                "vehicles": {"enabled": s.vehicles.enabled, "density": s.vehicles.density},
                "ambient_skaters": s.ambient_skaters,
                "zombie": s.zombie,
                "free_play": s.free_play.map(|f| json!({"traffic": f.traffic, "pedestrians": f.pedestrians, "ai_skaters": f.ai_skaters})),
                "npc_simulated": npc_simulated,
                "npc_avoid": npc_avoid,
                "ped_brain": ped_brain,
                "npc_tricks": {"mode": s.npc_tricks.mode.name(), "gate_window": s.npc_tricks.params.gate_window, "min_air_frames": s.npc_tricks.params.min_air_frames},
                "skater_trick_profiles": s.skater_trick_profiles.iter().map(|(k, v)| {
                    let table = |t: &Option<Vec<(i16, f32)>>| t.as_ref().map(|t| t.iter().map(|e| json!({"trick": e.0, "weight": e.1})).collect::<Vec<_>>());
                    (k.clone(), json!({"regular": table(&v.regular), "nollie": table(&v.nollie)}))
                }).collect::<serde_json::Map<_, _>>(),
            });
            // Inserted after: one more key in the literal passes json!'s recursion limit.
            if let Some(b) = out.get_mut("ped_brain").and_then(|v| v.as_object_mut()) {
                let f = s.ped_brain.object_flee;
                b.extend([("object_flee".into(), json!(f.enabled)), ("object_flee_radius".into(), json!(f.radius)), ("object_flee_min_speed".into(), json!(f.min_speed)), ("object_flee_lane_margin".into(), json!(f.lane_margin))]);
                let v = s.ped_brain.vehicle_flee;
                b.extend([("vehicle_flee".into(), json!(v.enabled)), ("vehicle_flee_radius".into(), json!(v.radius)), ("vehicle_flee_min_dot".into(), json!(v.min_dot)), ("vehicle_flee_away".into(), json!(v.away)), ("vehicle_flee_toward".into(), json!(v.toward)), ("vehicle_flee_require_on_road".into(), json!(v.require_on_road)), ("vehicle_flee_require_flagged".into(), json!(v.require_flagged))]);
                let l = s.ped_brain.flee;
                b.extend([("flee_leg".into(), json!(l.leg)), ("flee_arrival_radius".into(), json!(l.arrival_radius)), ("flee_threat_speed_gate".into(), json!(l.threat_speed_gate)), ("flee_side_blend".into(), json!(l.side_blend)), ("flee_releg_on_nav_done".into(), json!(l.releg_on_nav_done))]);
            }
            out
        }),
        "props" => world.get_resource::<PropTuningSettings>().map_or(Value::Null, |s| {
            let by: serde_json::Map<String, Value> = s.0.by_template.iter().map(|(k, t)| (k.clone(), tuning(t))).collect();
            let v = s.0.solver;
            let u = s.0.upright;
            json!({"default": tuning(&s.0.default), "by_template": by, "solver": {
                "row_solver": v.row_solver, "iterations": v.iterations, "sleep_energy": v.sleep_energy,
                "sleep_frames": v.sleep_frames, "max_sleeps_per_step": v.max_sleeps_per_step, "rest_snap": v.rest_snap,
                "bail_clamp": v.bail_clamp, "bail_linear_factor": v.bail_linear_factor, "bail_angular_factor": v.bail_angular_factor,
                "bail_heavy_mass": v.bail_heavy_mass, "bail_blend": v.bail_blend, "walkable_mass": v.walkable_mass, "skater_push_box": v.skater_push_box, "skater_rows": v.skater_rows, "board_row_mass": v.board_row_mass,
                "ground_reduction": v.ground_reduction.enabled, "reduction_normal_dot": v.ground_reduction.normal_dot,
                "reduction_plane_distance": v.ground_reduction.plane_distance, "reduction_chunk": v.ground_reduction.chunk,
            }, "upright": {
                "window_seconds": u.window_seconds, "tick_seconds": u.tick_seconds, "stop_angle_deg": u.stop_angle_deg,
                "max_angle_deg": u.max_angle_deg, "dead_band_deg": u.dead_band_deg, "gain_min": u.gain_min, "gain_max": u.gain_max,
                "gain_blend_start": u.gain_blend_start, "off_axis_spin": u.off_axis_spin, "command_rate": u.command_rate,
                "fallback_angle_deg": u.fallback_angle_deg, "block_yaw": u.block_yaw,
            }})
        }),
        "carry" => world.get_resource::<CarrySettings>().map_or(Value::Null, |c| {
            // The tuning in effect: the live carry's base (setup data) with this patch applied.
            let base = world.get_resource::<crate::physics::GamePhysics>().map_or_else(
                crate::physics::prop_carry::CarryLocomotion::default,
                |p| p.prop_carry.base_locomotion(),
            );
            let l = c.locomotion.apply(base);
            let m = l.move_object;
            let g = |g: skate_core::player::offboard::move_object::ControllerGains| json!([g.proportional, g.filtered, g.derivative, g.filter]);
            let curve = |c: skate_core::point_graph::PointGraph<8>| json!([c.x, c.y]);
            let r = &c.move_rules;
            let by: serde_json::Map<String, Value> = r
                .by_template
                .iter()
                .map(|(k, b)| (k.clone(), json!({"material_held": b.held, "material_free": b.free, "material_free_upright": b.free_upright,
                    "upright_pair": b.upright_pair, "restitution": b.restitution, "record_272": b.record_272,
                    "linear_drag": b.linear_drag, "angular_drag": b.angular_drag, "mass": b.mass,
                    "maximum_linear_velocity": b.maximum_linear_velocity, "maximum_angular_velocity": b.maximum_angular_velocity,
                    "inertia_scale": b.inertia_scale, "inertia_offset": b.inertia_offset})))
                .collect();
            json!({"grab_bit": c.buttons.grab_bit, "placement_bit": c.buttons.placement_bit, "grab_range": c.grab_range,
                "push_speed": m.push_speed, "pull_speed": m.pull_speed, "side_speed": m.side_speed, "turn_rate": l.turn_rate,
                "grip_reach": m.follow_reach, "linear_clamp": m.linear_clamp, "yaw_clamp": m.yaw_clamp, "relatch": m.relatch,
                "slew_per_tick": m.slew_per_tick, "yaw_rate_feedback": m.yaw_rate_feedback, "linear_controller": g(m.linear_controller), "yaw_controller": g(m.yaw_controller),
                "lever_rotation": curve(m.lever_rotation), "lever_yaw": curve(m.lever_yaw), "mass_speed": curve(m.mass_speed),
                "inertia_yaw_gain": curve(m.inertia_yaw_gain), "let_go_distance": l.let_go_distance, "drop_board": l.drop_board,
                "follow_step": m.follow_step, "hold_angle_limit": m.hold_angle_limit,
                "hold_max_angle_to_horizontal": m.hold_max_angle_to_horizontal, "hold_box_extents": m.hold_box_extents,
                "record_272_speed_scale": m.record_272_speed_scale, "grab_end_exclusion": m.grab_end_exclusion,
                "hand_ik_enter": m.hand_ik_enter, "hand_ik_curve": curve(m.hand_ik_curve), "root_yaw_gain": curve(m.root_yaw_gain), "hand_ik_rate": m.hand_ik_rate, "hand_ik_reach": m.hand_ik_reach,
                "commanded_material": r.commanded_material, "upright_cos": r.upright_cos, "apply_at_com": r.apply_at_com,
                "yaw_replaces_torque": r.yaw_replaces_torque, "ignore_vertical": r.ignore_vertical,
                "wake_on_command": r.wake_on_command, "by_template": by})
        }),
        "shadows" => world.get_resource::<WorldShadowSettings>().map_or(Value::Null, |s| json!({"world_floor": s.floor.to_array()})),
        "backdrop" => world.get_resource::<BackdropSettings>().map_or(Value::Null, |s| json!({"visible": s.visible, "proxy_terrain": s.proxy_terrain})),
        "respawn" => world.get_resource::<RespawnSettings>().map_or(Value::Null, |s| json!({"air_timeout_ticks": s.air_timeout_ticks})),
        "exposure" => world.get_resource::<ExposureMeter>().map_or(Value::Null, |s| json!({"meter_weights": s.weights.to_array(), "meter_scale": s.scale})),
        "decals" => world.get_resource::<DecalSettings>().map_or(Value::Null, |s| json!({"opacity": s.opacity})),
        "prop_sounds" => world.get_resource::<crate::game_audio::prop_sounds::PropSoundSettings>().map_or(Value::Null, |s| json!({
            "enabled": !s.disabled, "types": serde_json::to_value(&s.types).unwrap_or(Value::Null)})),
        "video_screens" => world.get_resource::<VideoScreenSettings>().map_or(Value::Null, |s| json!({
            "enabled": s.enabled, "width": s.size.x, "height": s.size.y, "rate_hz": s.rate_hz, "max_distance": s.max_distance})),
        "ghost" => world.get_resource::<GhostTuning>().map_or(Value::Null, |s| json!({"enabled": s.0.enabled, "fade_in_seconds": s.0.fade_in_seconds, "hold_alpha": s.0.hold_alpha})),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new();
        w.insert_resource(LivingWorldSettings::default());
        w.init_resource::<PropTuningSettings>();
        w.init_resource::<CarrySettings>();
        w.init_resource::<WorldShadowSettings>();
        w.init_resource::<BackdropSettings>();
        w.init_resource::<RespawnSettings>();
        w.init_resource::<ExposureMeter>();
        w.init_resource::<GhostTuning>();
        w.init_resource::<DecalSettings>();
        w.init_resource::<VideoScreenSettings>();
        w
    }

    #[test]
    fn video_screens_set_merge_and_reset_on_disable() {
        let mut w = world();
        set(&mut w, "dev.a", "video_screens", Some(json!({"width": 256, "height": 128}))).unwrap();
        set(&mut w, "dev.b", "video_screens", Some(json!({"width": 1024, "rate_hz": 15.0, "enabled": false}))).unwrap();
        let s = *w.resource::<VideoScreenSettings>();
        assert_eq!((s.size, s.rate_hz, s.enabled), (UVec2::new(256, 128), 15.0, false), "first writer wins per field");
        assert_eq!(read(&w, "video_screens")["height"], json!(128));
        assert!(set(&mut w, "dev.a", "video_screens", Some(json!({"width": 4096}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<VideoScreenSettings>().size, UVec2::new(1024, 288));
        clear_owner(&mut w, "dev.b");
        assert_eq!(*w.resource::<VideoScreenSettings>(), VideoScreenSettings::default(), "mod disable restores the defaults");
    }

    #[test]
    fn decals_default_to_retail_set_and_reset_on_disable() {
        let mut w = world();
        assert_eq!(read(&w, "decals"), json!({"opacity": 1.0f32}), "retail blends decals at full strength");
        set(&mut w, "dev.a", "decals", Some(json!({"opacity": 0.35}))).unwrap();
        set(&mut w, "dev.b", "decals", Some(json!({"opacity": 0.8}))).unwrap();
        assert_eq!(w.resource::<DecalSettings>().opacity, 0.35, "first writer wins");
        assert!(set(&mut w, "dev.a", "decals", Some(json!({"opacity": 1.5}))).is_err());
        assert!(set(&mut w, "dev.a", "decals", Some(json!({"strength": 1}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<DecalSettings>().opacity, 0.8);
        clear_all(&mut w);
        assert_eq!(*w.resource::<DecalSettings>(), DecalSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn prop_sounds_merge_per_type_and_reset_on_disable() {
        use crate::game_audio::prop_sounds::{overridden, PropSoundSettings};
        let mut w = world();
        w.init_resource::<PropSoundSettings>();
        set(&mut w, "dev.a", "prop_sounds", Some(json!({"types": {"dumpster": {"impact_min": 0.4}}}))).unwrap();
        set(&mut w, "dev.b", "prop_sounds", Some(json!({"types": {"dumpster": {"impact_min": 0.9, "cooldown": 3}, "mailbox": {"material": 52}}}))).unwrap();
        let s = w.resource::<PropSoundSettings>().clone();
        assert_eq!(s.types["dumpster"].impact_min, Some(0.4), "first writer wins per field");
        assert_eq!(s.types["dumpster"].cooldown, Some(3));
        let r = overridden(None, &s.types["mailbox"]);
        assert_eq!((r.material, r.impact_min), (52, 0.0), "a type without a record starts from a silent one");
        assert!(set(&mut w, "dev.a", "prop_sounds", Some(json!({"types": {"x": {"material": 200}}}))).is_err());
        clear_all(&mut w);
        assert_eq!(*w.resource::<PropSoundSettings>(), PropSoundSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn ghost_fade_defaults_off_set_and_reset_on_disable() {
        let mut w = world();
        assert_eq!(read(&w, "ghost"), json!({"enabled": false, "fade_in_seconds": 1.0f32, "hold_alpha": 0.68f32}));
        set(&mut w, "dev.a", "ghost", Some(json!({"fade_in_seconds": 2.5}))).unwrap();
        set(&mut w, "dev.b", "ghost", Some(json!({"fade_in_seconds": 0.5, "enabled": true}))).unwrap();
        let g = w.resource::<GhostTuning>().0;
        assert_eq!(g.fade_in_seconds, 2.5, "first writer wins");
        assert!(g.enabled, "unset field from the next writer");
        assert!(set(&mut w, "dev.a", "ghost", Some(json!({"fade_in_seconds": -1.0}))).is_err());
        assert!(set(&mut w, "dev.a", "ghost", Some(json!({"hold_alpha": 2.0}))).is_err());
        assert!(set(&mut w, "dev.a", "ghost", Some(json!({"colour": 1}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<GhostTuning>().0.fade_in_seconds, 0.5);
        clear_all(&mut w);
        assert_eq!(*w.resource::<GhostTuning>(), GhostTuning::default(), "mod disable restores the default");
    }

    #[test]
    fn exposure_meter_defaults_to_retail_set_and_reset_on_disable() {
        let mut w = world();
        assert_eq!(*w.resource::<ExposureMeter>(), ExposureMeter { weights: Vec3::new(0.3, 0.4, 0.3), scale: 2.515 });
        set(&mut w, "dev.a", "exposure", Some(json!({"meter_weights": [0.2126, 0.7152, 0.0722]}))).unwrap();
        set(&mut w, "dev.b", "exposure", Some(json!({"meter_weights": [1.0, 0.0, 0.0], "meter_scale": 3.0}))).unwrap();
        let m = *w.resource::<ExposureMeter>();
        assert_eq!(m.weights, Vec3::new(0.2126, 0.7152, 0.0722), "first writer wins");
        assert_eq!(m.scale, 3.0, "unset field from the next writer");
        assert_eq!(read(&w, "exposure")["meter_scale"], json!(3.0f32));
        assert!(set(&mut w, "dev.a", "exposure", Some(json!({"meter_scale": -2.0}))).is_err());
        clear_owner(&mut w, "dev.b");
        assert_eq!(w.resource::<ExposureMeter>().scale, 2.515);
        clear_all(&mut w);
        assert_eq!(*w.resource::<ExposureMeter>(), ExposureMeter::default(), "mod disable restores retail");
    }

    #[test]
    fn respawn_air_timeout_defaults_to_retail_set_and_reset_on_disable() {
        let mut w = world();
        assert_eq!(w.resource::<RespawnSettings>().air_timeout_ticks, 300, "retail count > 300");
        assert_eq!(read(&w, "respawn"), json!({"air_timeout_ticks": 300}));
        set(&mut w, "dev.a", "respawn", Some(json!({"air_timeout_ticks": 120}))).unwrap();
        set(&mut w, "dev.b", "respawn", Some(json!({"air_timeout_ticks": 600}))).unwrap();
        assert_eq!(w.resource::<RespawnSettings>().air_timeout_ticks, 120, "first writer wins");
        assert!(set(&mut w, "dev.a", "respawn", Some(json!({"air_timeout_ticks": 0}))).is_err());
        assert_eq!(w.resource::<RespawnSettings>().air_timeout_ticks, 120, "a rejected patch keeps the old one");
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<RespawnSettings>().air_timeout_ticks, 600, "the next mod's patch applies");
        clear_all(&mut w);
        assert_eq!(*w.resource::<RespawnSettings>(), RespawnSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn backdrop_visible_by_default_set_and_reset_on_disable() {
        let mut w = world();
        assert!(w.resource::<BackdropSettings>().visible, "retail draws the backdrop");
        set(&mut w, "dev.a", "backdrop", Some(json!({"visible": false}))).unwrap();
        set(&mut w, "dev.b", "backdrop", Some(json!({"visible": true}))).unwrap();
        assert!(!w.resource::<BackdropSettings>().visible, "first writer wins");
        assert_eq!(read(&w, "backdrop"), json!({"visible": false, "proxy_terrain": true}));
        assert!(set(&mut w, "dev.a", "backdrop", Some(json!({"visible": "no"}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert!(w.resource::<BackdropSettings>().visible);
        set(&mut w, "dev.a", "backdrop", Some(json!({"visible": false}))).unwrap();
        clear_all(&mut w);
        assert_eq!(*w.resource::<BackdropSettings>(), BackdropSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn proxy_terrain_defaults_to_retail_set_and_reset_on_disable() {
        let mut w = world();
        assert!(w.resource::<BackdropSettings>().proxy_terrain, "retail draws the unpaired proxy cells");
        set(&mut w, "dev.a", "backdrop", Some(json!({"proxy_terrain": false}))).unwrap();
        set(&mut w, "dev.b", "backdrop", Some(json!({"proxy_terrain": true, "visible": false}))).unwrap();
        let s = *w.resource::<BackdropSettings>();
        assert!(!s.proxy_terrain && !s.visible, "first writer per field");
        assert!(set(&mut w, "dev.a", "backdrop", Some(json!({"proxy_terrain": 1}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert!(w.resource::<BackdropSettings>().proxy_terrain, "mod stop restores retail");
        clear_owner(&mut w, "dev.b");
        assert_eq!(*w.resource::<BackdropSettings>(), BackdropSettings::default());
    }

    #[test]
    fn world_shadow_floor_defaults_to_retail_set_and_reset() {
        let mut w = world();
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::new(0.05, 0.09, 0.13));
        set(&mut w, "dev.a", "shadows", Some(json!({"world_floor": [0.0, 0.0, 0.0]}))).unwrap();
        set(&mut w, "dev.b", "shadows", Some(json!({"world_floor": [0.5, 0.5, 0.5]}))).unwrap();
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::ZERO, "first writer wins");
        assert_eq!(read(&w, "shadows")["world_floor"], json!([0.0f32, 0.0f32, 0.0f32]));
        assert!(set(&mut w, "dev.a", "shadows", Some(json!({"world_floor": [2.0, 0.0, 0.0]}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::splat(0.5));
        clear_all(&mut w);
        assert_eq!(*w.resource::<WorldShadowSettings>(), WorldShadowSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn ped_vehicle_contact_is_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let retail = LivingWorldSettings::default().ped_vehicle_contact;
        assert!(retail.enabled && retail.push, "retail: cars push peds out of the way");
        assert_eq!(read(&w, "living_world")["ped_vehicle_contact"], json!({"enabled": true, "push": true}));
        set(&mut w, "dev.a", "living_world", Some(json!({"ped_vehicle_contact": {"push": false}}))).unwrap();
        let s = w.resource::<LivingWorldSettings>().ped_vehicle_contact;
        assert!(s.enabled && !s.push, "absent fields keep retail");
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_vehicle_contact": {"knockdown": true}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<LivingWorldSettings>().ped_vehicle_contact, retail, "mod disable restores retail");
    }

    /// Ask 80 follow-up: every NPC skater walk-back (controller B) and line controller value is mod-reachable.
    #[test]
    fn npc_controller_settings_are_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let retail = LivingWorldSettings::default().npc_simulated;
        assert_eq!((retail.navmesh.start_locate_radius, retail.controller.resnap_frames, retail.controller.stuck_limits), (f32::MAX, 240, [30, 30, 2, 180, 30, 3]));
        assert_eq!(read(&w, "living_world")["npc_line_controller"]["resnap_frames"], json!(240));
        set(&mut w, "dev.a", "living_world", Some(json!({
            "npc_simulated": {"walk_arrive_distance": 3.0},
            "npc_walk_back": {"start_locate_radius": 0.5, "arrive_h": 2.0},
            "npc_line_controller": {"resnap_frames": 120, "stuck_limits": [10, 10, 2, 90, 10, 3]},
        }))).unwrap();
        let s = w.resource::<LivingWorldSettings>().npc_simulated;
        assert_eq!((s.navmesh.start_locate_radius, s.navmesh.goal_locate_radius), (0.5, f32::MAX), "absent fields keep retail");
        assert_eq!(s.navmesh.arrive_h, 2.0, "the nested key wins over the walk_arrive_distance alias");
        assert_eq!((s.controller.resnap_frames, s.controller.stuck_limits), (120, [10, 10, 2, 90, 10, 3]));
        assert_eq!(read(&w, "living_world")["npc_walk_back"]["start_locate_radius"], json!(0.5));
        // A second mod: the first writer keeps each field it set, the second fills the rest.
        set(&mut w, "dev.b", "living_world", Some(json!({"npc_line_controller": {"resnap_frames": 60, "safe_run": 4}}))).unwrap();
        let c = w.resource::<LivingWorldSettings>().npc_simulated.controller;
        assert_eq!((c.resnap_frames, c.safe_run), (120, 4));
        for bad in [json!({"npc_walk_back": {"arrive_h": -1.0}}), json!({"npc_walk_back": {"push_period": 0}}), json!({"npc_line_controller": {"stuck_limits": [1, 2, 3, 4, 5]}}), json!({"npc_line_controller": {"resnap": 1}})] {
            assert!(set(&mut w, "dev.c", "living_world", Some(bad.clone())).is_err(), "{bad}");
        }
        clear_owner(&mut w, "dev.a");
        clear_owner(&mut w, "dev.b");
        let back = w.resource::<LivingWorldSettings>().npc_simulated;
        assert_eq!((back.navmesh, back.controller), (retail.navmesh, retail.controller), "mod disable restores retail");
    }

    /// Line end stage 3 (b701 section 3): the path follower's line end and lost values and the mode 1 fallback's
    /// valid-line distance are mod-reachable, read back and reset on disable.
    #[test]
    fn npc_line_end_settings_are_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let (path, chain) = (LivingWorldSettings::default().npc_simulated.path, LivingWorldSettings::default().skater_line_chain);
        assert_eq!((path.line_end_t, path.lost_radius, path.line_end_lost_radius, path.lost_max_speed, chain.fallback_valid_d2), (0.8, 6.0, 4.5, 14.0, 36.0));
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["fallback_valid_m"], json!(6.0));
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_path": {"line_end_t": 0.5, "line_end_lost_radius": 3.0}, "skater_line_chain": {"fallback_valid_m": 2.0}}))).unwrap();
        let s = w.resource::<LivingWorldSettings>();
        assert_eq!((s.npc_simulated.path.line_end_t, s.npc_simulated.path.line_end_lost_radius, s.npc_simulated.path.lost_radius, s.skater_line_chain.fallback_valid_d2), (0.5, 3.0, 6.0, 4.0));
        assert_eq!(read(&w, "living_world")["skater_path"]["line_end_t"], json!(0.5));
        for bad in [json!({"skater_path": {"line_end_t": 1.5}}), json!({"skater_path": {"lost_radius": -1.0}}), json!({"skater_line_chain": {"fallback_valid_m": -2.0}}), json!({"skater_path": {"window": 3}})] {
            assert!(set(&mut w, "dev.c", "living_world", Some(bad.clone())).is_err(), "{bad}");
        }
        clear_owner(&mut w, "dev.a");
        let back = w.resource::<LivingWorldSettings>();
        assert_eq!((back.npc_simulated.path, back.skater_line_chain.fallback_valid_d2), (path, chain.fallback_valid_d2), "mod disable restores retail");
    }

    /// b616: the NPC air steer is on (retail), mod-reachable and reset on disable.
    #[test]
    fn npc_air_steer_is_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let retail = LivingWorldSettings::default().npc_simulated;
        if std::env::var("SKATE_NPC_AIR_STEER").is_err() {
            assert!(retail.air_steer_on);
        }
        assert_eq!((retail.air_steer.dead_zone_deg, retail.air_steer.full_deg), (2.0, 10.0));
        set(&mut w, "dev.a", "living_world", Some(json!({"npc_simulated": {"air_steer": false, "air_steer_full_deg": 20.0}}))).unwrap();
        let s = w.resource::<LivingWorldSettings>().npc_simulated;
        assert_eq!((s.air_steer_on, s.air_steer.dead_zone_deg, s.air_steer.full_deg), (false, 2.0, 20.0));
        assert_eq!(read(&w, "living_world")["npc_simulated"]["air_steer"], json!(false));
        assert!(set(&mut w, "dev.b", "living_world", Some(json!({"npc_simulated": {"air_steer_dead_zone_deg": -1.0}}))).is_err());
        clear_owner(&mut w, "dev.a");
        let back = w.resource::<LivingWorldSettings>().npc_simulated;
        assert_eq!((back.air_steer_on, back.air_steer), (retail.air_steer_on, retail.air_steer), "mod disable restores retail");
    }

    /// b612: the avoid anchor of the far aim is mod-reachable and reset on disable.
    #[test]
    fn npc_avoid_anchor_is_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        assert_eq!(LivingWorldSettings::default().npc_simulated.ground_steer.avoid_anchor_ticks, 15.0);
        set(&mut w, "dev.a", "living_world", Some(json!({"npc_simulated": {"avoid_anchor_ticks": 30.0}}))).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().npc_simulated.ground_steer.avoid_anchor_ticks, 30.0);
        assert_eq!(read(&w, "living_world")["npc_simulated"]["avoid_anchor_ticks"], json!(30.0));
        assert!(set(&mut w, "dev.b", "living_world", Some(json!({"npc_simulated": {"avoid_anchor_ticks": -1.0}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<LivingWorldSettings>().npc_simulated.ground_steer.avoid_anchor_ticks, 15.0);
    }

    #[test]
    fn ped_nav_is_mod_reachable_and_reset_on_disable() {
        use skate_core::living_world::peds::CrosswalkRule;
        let mut w = world();
        let default = LivingWorldSettings::default().ped_nav;
        assert_eq!((default.wander.turn_edges, default.crosswalk), ([25.0, 70.0, 115.0, 160.0], CrosswalkRule::Off));
        set(&mut w, "dev.a", "living_world", Some(json!({"ped_nav": {"steer_rate": 90.0, "stuck_seconds": 4.0, "give_up_seconds": 0.0, "crosswalk": "walk_signal"}}))).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().ped_nav.wander.give_up_seconds, 0.0, "the NOT RETAIL give-up can be switched off");
        let s = w.resource::<LivingWorldSettings>().ped_nav.clone();
        assert!((s.wander.steer_rate.to_degrees() - 90.0).abs() < 1e-3);
        assert_eq!((s.wander.stuck_seconds, s.wander.stall_seconds, s.crosswalk), (4.0, default.wander.stall_seconds, CrosswalkRule::WalkSignal));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_nav": {"turn_edges": [70.0, 25.0, 115.0, 160.0]}}))).is_err(), "edges ascend");
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_nav": {"crosswalk": "always"}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<LivingWorldSettings>().ped_nav, default, "mod disable restores the default");
    }

    #[test]
    fn ped_body_is_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let default = LivingWorldSettings::default().ped_body;
        assert!(default.enabled && default.parts.len() == 3, "solid peds, retail three parts");
        assert_eq!(read(&w, "living_world")["ped_body"]["parts"][0]["from"], json!("Hips"));
        let arm = json!({"from": "LeftForeArm", "to": "LeftHand", "centre": 0.5, "radius": 0.1, "half_length": 0.2, "mass": 5.0});
        set(&mut w, "dev.a", "living_world", Some(json!({"ped_body": {"parts": [arm], "range": 20.0}}))).unwrap();
        let s = w.resource::<LivingWorldSettings>().ped_body.clone();
        assert_eq!((s.parts.len(), s.parts[0].mass, s.range, s.look_ahead), (1, 5.0, 20.0, default.look_ahead), "absent fields keep the default");
        assert_eq!(s.parts[0].friction, crate::living_world::ped_body::DEFAULT_PART_FRICTION, "no friction given: the default");
        let slippery = json!({"from": "Hips", "to": "Head", "centre": 0.0, "radius": 0.2, "half_length": 0.5, "mass": 10.0, "friction": 0.1});
        set(&mut w, "dev.a", "living_world", Some(json!({"ped_body": {"parts": [slippery], "reaction_part": 0}}))).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().ped_body.parts[0].friction, 0.1);
        assert_eq!(read(&w, "living_world")["ped_body"]["parts"][0]["friction"], json!(0.1f32));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_body": {"reaction_part": 9}}))).is_err(), "no such part index");
        let massless = json!({"from": "Hips", "to": "Head", "centre": 0.0, "radius": 0.2, "half_length": 0.5, "mass": 0.0});
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_body": {"parts": [massless]}}))).is_err(), "no massless part");
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_body": {"shape": "box"}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<LivingWorldSettings>().ped_body, default, "mod disable restores the default");
    }

    #[test]
    fn living_world_values_set_and_reset_on_disable() {
        let mut w = world();
        w.resource_mut::<LivingWorldSettings>().set_user_draw_distance(1.5);
        set(&mut w, "dev.a", "living_world", Some(json!({
            "npc_draw_distance": 3.0,
            "skater_fade": {"fade_in_seconds": 0.0, "fade_seconds": 2.5, "despawn_alpha": 0.05},
            "skater_line_chain": {"radius": 0.0, "blend_seconds": 0.6, "keep_facing": true, "facing_rule": "per_node", "steer_full_deg": 12.0, "fakie_high_speed": 2.5, "fakie_spawn_seconds": 0.0},
            "ped_fade": {"distance": [90.0, 110.0], "fade_in_seconds": 2.0, "enabled": false},
        }))).unwrap();
        {
            let s = w.resource::<LivingWorldSettings>();
            assert_eq!(s.npc_draw_distance, 3.0);
            assert_eq!(s.user_npc_draw_distance, 1.5, "the player's choice is kept aside");
            assert_eq!((s.skater_fade.fade_in_seconds, s.skater_fade.fade_seconds, s.skater_fade.despawn_alpha), (0.0, 2.5, 0.05));
            assert_eq!((s.ped_fade.distance, s.ped_fade.fade_in_seconds, s.ped_fade.enabled), ([90.0, 110.0], 2.0, false));
            assert_eq!((s.skater_line_chain.radius, s.skater_line_chain.max_candidates), (0.0, 16), "absent fields keep retail");
            assert_eq!(s.skater_line_chain.blend_seconds, 0.6);
            assert!(s.skater_line_chain.keep_facing, "the fix 16 option is mod-reachable");
            assert_eq!(s.skater_line_chain.facing_rule, skate_core::living_world::replay::FacingRule::PerNode, "the fix 23 rule is mod-reachable");
            assert_eq!((s.skater_line_chain.steer_dead_zone_deg, s.skater_line_chain.steer_full_deg), (2.0, 12.0));
            let f = s.skater_line_chain.fakie;
            assert_eq!((f.high_speed, f.low_speed, f.slowly_backwards_seconds, f.after_teleport_seconds), (2.5, 0.5, 0.2, 0.0), "the fakie rule is mod-reachable, absent fields keep retail");
        }
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["fakie_high_speed"], json!(2.5));
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["radius"], json!(0.0));
        assert!((read(&w, "living_world")["skater_line_chain"]["blend_seconds"].as_f64().unwrap() - 0.6).abs() < 1e-6);
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["keep_facing"], json!(true));
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["facing_rule"], json!("per_node"));
        assert_eq!(read(&w, "living_world")["skater_fade"]["fade_seconds"], json!(2.5));
        clear_owner(&mut w, "dev.a");
        let s = w.resource::<LivingWorldSettings>();
        assert_eq!(s.npc_draw_distance, 1.5, "disable restores the player's menu choice");
        let retail = LivingWorldSettings::default();
        assert_eq!((s.skater_fade, s.ped_fade, s.skater_line_chain), (retail.skater_fade, retail.ped_fade, retail.skater_line_chain));
        assert!(!s.skater_line_chain.keep_facing, "disable turns the fix 16 option off");
        assert_eq!(s.skater_line_chain.facing_rule, skate_core::living_world::replay::FacingRule::RidingEntry, "disable returns to the default (retail) rule");
        assert_eq!((s.skater_line_chain.steer_dead_zone_deg, s.skater_line_chain.steer_full_deg), (2.0, 10.0));
        assert_eq!(s.skater_line_chain.fakie, skate_core::living_world::replay::retail::FAKIE, "disable restores the retail fakie rule");
    }

    #[test]
    fn first_writer_wins_and_nil_gives_fields_back() {
        let mut w = world();
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_fade": {"fade_seconds": 2.0}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_fade": {"fade_seconds": 4.0}, "npc_draw_distance": 2.0}))).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().skater_fade.fade_seconds, 2.0);
        assert_eq!(w.resource::<LivingWorldSettings>().npc_draw_distance, 2.0);
        set(&mut w, "dev.a", "living_world", None).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().skater_fade.fade_seconds, 4.0);
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"bogus": 1}))).is_err());
        clear_all(&mut w);
        assert_eq!(*w.resource::<LivingWorldSettings>(), LivingWorldSettings::default());
    }

    #[test]
    fn npc_skater_clips_set_merge_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_clips": {"rolling": "A_CYC"}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_clips": {"rolling": "B_CYC", "air": "C"}}))).unwrap();
        {
            let s = w.resource::<LivingWorldSettings>();
            assert_eq!((s.skater_clips["rolling"].as_str(), s.skater_clips["air"].as_str()), ("A_CYC", "C"));
        }
        assert_eq!(read(&w, "living_world")["skater_clips"]["air"], json!("C"));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_clips": {"flying": "X"}}))).is_err());
        clear_all(&mut w);
        assert!(w.resource::<LivingWorldSettings>().skater_clips.is_empty());
        // The stable phase ids the mod API accepts are the engine's.
        use skate_core::living_world::replay::ReplayPhase as P;
        let names = [P::Rolling, P::Crouched, P::Air, P::AirTrick, P::GroundTrick, P::OffBoard].map(P::name);
        assert_eq!(names, skate_mods::world_tuning::NPC_SKATER_PHASES);
    }

    #[test]
    fn npc_skater_stance_set_merge_and_reset() {
        use crate::living_world::npc_skaters::npc_stance;
        use skate_core::living_world::stance::NaturalStance as S;
        let records: std::collections::BTreeMap<String, String> = [("deerman".to_owned(), "deerman_of_darkwoods".to_owned())].into();
        let mut w = world();
        let stance = |w: &World, key: &str| npc_stance(&records, &w.resource::<LivingWorldSettings>().skater_stance, key);
        // Retail table by default (the character key resolves to its record).
        assert_eq!((stance(&w, "josh_kalis"), stance(&w, "chris_cole"), stance(&w, "deerman")), (S::Goofy, S::Regular, S::Goofy));
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_stance": {"CD56C7FE01EBE665": "regular"}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_stance": {"CD56C7FE01EBE665": "goofy", "deerman_of_darkwoods": "regular"}}))).unwrap();
        assert_eq!((stance(&w, "josh_kalis"), stance(&w, "deerman"), stance(&w, "cuz")), (S::Regular, S::Regular, S::Goofy), "first writer wins, others merge");
        assert_eq!(read(&w, "living_world")["skater_stance"]["deerman_of_darkwoods"], json!("regular"));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_stance": {"josh_kalis": "switch"}}))).is_err());
        clear_all(&mut w);
        assert!(w.resource::<LivingWorldSettings>().skater_stance.is_empty());
        assert_eq!((stance(&w, "josh_kalis"), stance(&w, "deerman")), (S::Goofy, S::Goofy), "reset = retail table");
    }

    #[test]
    fn living_world_density_counts_and_free_play_set_merge_and_reset() {
        let mut w = world();
        assert_eq!(read(&w, "living_world")["free_play"], json!(null), "career free roam by default");
        set(&mut w, "dev.a", "living_world", Some(json!({"free_play": {"traffic": 0.3}, "pedestrians": {"density": 2.0}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"free_play": {"traffic": 0.9, "ai_skaters": false}, "ambient_skaters": 5}))).unwrap();
        let s = w.resource::<LivingWorldSettings>();
        let f = s.free_play.expect("Free Play on");
        assert_eq!((f.traffic, f.pedestrians, f.ai_skaters), (0.3, 1.0, false), "first writer wins per field, unset = reset value");
        assert_eq!((s.pedestrians.density, s.ambient_skaters), (2.0, 5));
        // The settings reach the population config.
        let mut config = skate_core::living_world::PopulationConfig::retail();
        s.apply(&mut config);
        assert_eq!((config.pedestrians.density, config.skaters.desired), (2.0, 5));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"free_play": {"pedestrians": 2.0}}))).is_err());
        clear_all(&mut w);
        let s = w.resource::<LivingWorldSettings>();
        assert_eq!((s.free_play, s.pedestrians.density, s.ambient_skaters), (None, 1.0, 3));
    }

    #[test]
    fn npc_skater_trick_choice_set_merge_and_reset() {
        use crate::living_world::npc_skaters::npc_trick_profile;
        use skate_core::living_world::npc_tricks::{TrickMode, TrickProfile};
        let mut w = world();
        let mut data = skate_data::living_world::SkaterTrickProfiles::default();
        data.profiles.insert("default".into(), TrickProfile { regular: vec![(96, 1.0)], nollie: vec![(117, 1.0)] });
        data.profiles.insert("street_medium".into(), TrickProfile { regular: vec![(92, 1.0)], nollie: vec![] });
        data.character_profile.insert("jake".into(), "street_medium".into());
        let profile = |w: &World, key: &str| npc_trick_profile(&data, &w.resource::<LivingWorldSettings>().skater_trick_profiles, key).map(std::borrow::Cow::into_owned);
        assert_eq!(read(&w, "living_world")["npc_tricks"], json!({"mode": "profile", "gate_window": 300, "min_air_frames": 50}), "retail by default");
        assert_eq!(profile(&w, "jake").unwrap().regular, [(92, 1.0)], "the character's aiprofile");
        set(&mut w, "dev.a", "living_world", Some(json!({"npc_tricks": {"mode": "recorded"}, "skater_trick_profiles": {"street_medium": {"regular": [{"trick": 128, "weight": 2.0}]}}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"npc_tricks": {"mode": "none", "min_air_frames": 20}, "skater_trick_profiles": {"jake": {"nollie": [{"trick": 127, "weight": 1.0}]}}}))).unwrap();
        let t = w.resource::<LivingWorldSettings>().npc_tricks;
        assert_eq!((t.mode, t.params.min_air_frames, t.params.gate_window), (TrickMode::Recorded, 20, 300), "first writer wins, others merge");
        // The character key's table wins per table; the profile name's fills the other.
        assert_eq!(profile(&w, "jake").unwrap(), TrickProfile { regular: vec![(128, 2.0)], nollie: vec![(127, 1.0)] });
        assert_eq!(profile(&w, "cuz").unwrap().regular, [(96, 1.0)], "others keep the disc's default");
        assert_eq!(read(&w, "living_world")["skater_trick_profiles"]["jake"]["nollie"][0]["trick"], json!(127));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"npc_tricks": {"mode": "scripted"}}))).is_err());
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_trick_profiles": {"jake": {"regular": [{"trick": 332, "weight": 1.0}]}}}))).is_err());
        clear_all(&mut w);
        assert_eq!(w.resource::<LivingWorldSettings>().npc_tricks, Default::default());
        assert_eq!(profile(&w, "jake").unwrap().regular, [(92, 1.0)], "reset = the disc's tables");
    }

    #[test]
    fn npc_skater_stance_events_set_merge_and_reset() {
        use skate_core::living_world::stance::StanceEvents;
        let mut w = world();
        let events = |w: &World| StanceEvents::with_overrides(&w.resource::<LivingWorldSettings>().skater_stance_events);
        assert_eq!(events(&w), StanceEvents::retail(), "retail by default");
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_stance_events": {"mirrored": "my_mirror"}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_stance_events": {"mirrored": "other", "switch": ""}}))).unwrap();
        let e = events(&w);
        assert_eq!((e.board_backward.as_str(), e.mirrored.as_str(), e.switch.as_str()), ("animboardbackward", "my_mirror", ""), "first writer wins, others merge");
        assert_eq!(read(&w, "living_world")["skater_stance_events"]["switch"], json!(""));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_stance_events": {"fakie": "x"}}))).is_err());
        clear_all(&mut w);
        assert_eq!(events(&w), StanceEvents::retail(), "reset = retail");
    }

    #[test]
    fn npc_skater_blend_seconds_set_merge_and_reset() {
        use crate::living_world::npc_skaters::{RETAIL_BLEND_SECONDS, blend_seconds};
        use skate_core::living_world::replay::ReplayPhase as P;
        let mut w = world();
        assert_eq!(blend_seconds(&w.resource::<LivingWorldSettings>().skater_blend_seconds, P::Air), RETAIL_BLEND_SECONDS);
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_blend_seconds": {"air": 0.4}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_blend_seconds": {"air": 0.1, "default": 0.0}}))).unwrap();
        {
            let s = &w.resource::<LivingWorldSettings>().skater_blend_seconds;
            assert_eq!((blend_seconds(s, P::Air), blend_seconds(s, P::Rolling)), (0.4, 0.0));
        }
        assert_eq!(read(&w, "living_world")["skater_blend_seconds"]["default"], json!(0.0));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_blend_seconds": {"air": -1.0}}))).is_err());
        clear_all(&mut w);
        assert!(w.resource::<LivingWorldSettings>().skater_blend_seconds.is_empty());
    }

    #[test]
    fn prop_tuning_and_collision_box_set_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "props", Some(json!({
            "default": {"max_depenetration_per_tick": 0.1, "stuck_release_ticks": 10},
            "by_template": {"bench01": {"push_transfer": 0.2, "collision_box": {"center": [0.0, 0.4, 0.0], "half_extents": [1.0, 0.4, 0.3]}}},
        }))).unwrap();
        {
            let t = &w.resource::<PropTuningSettings>().0;
            assert_eq!((t.default.max_depenetration_per_tick, t.default.stuck_release_ticks), (0.1, 10));
            let b = t.for_template("bench01");
            assert_eq!((b.push_transfer, b.max_depenetration_per_tick), (0.2, 0.1), "template starts from the patched default");
            let bx = b.collision_box.unwrap();
            assert_eq!((bx.center.y, bx.half_extents.x), (0.4, 1.0));
            assert_eq!(t.for_template("other").push_transfer, PropTuning::default().push_transfer);
        }
        assert_eq!(read(&w, "props")["by_template"]["bench01"]["collision_box"]["half_extents"], json!([1.0f32, 0.4f32, 0.3f32]));
        clear_owner(&mut w, "dev.a");
        assert_eq!(*w.resource::<PropTuningSettings>(), PropTuningSettings::default());
    }

    #[test]
    fn prop_solver_settings_set_validate_and_reset() {
        use crate::physics::prop_dynamics::PropSolverSettings;
        let mut w = world();
        let retail = PropSolverSettings::default();
        assert_eq!((retail.row_solver, retail.iterations, retail.sleep_energy, retail.sleep_frames, retail.max_sleeps_per_step, retail.rest_snap),
            (true, 25, 1e-5, 2, 100, false), "retail DMO island defaults");
        set(&mut w, "dev.a", "props", Some(json!({"solver": {"iterations": 10, "sleep_energy": 0.5, "sleep_frames": 30, "rest_snap": true}}))).unwrap();
        {
            let v = w.resource::<PropTuningSettings>().0.solver;
            assert_eq!((v.row_solver, v.iterations, v.sleep_energy, v.sleep_frames, v.max_sleeps_per_step, v.rest_snap), (true, 10, 0.5, 30, 100, true));
        }
        assert_eq!(read(&w, "props")["solver"]["iterations"], json!(10));
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"iterations": 0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"sleep_energy": -1.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"sleep_frames": 0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"unknown": 1}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<PropTuningSettings>().0.solver, retail);
    }

    #[test]
    fn prop_upright_settings_set_validate_and_reset() {
        let mut w = world();
        let retail = crate::physics::prop_dynamics::PropUprightSettings::default();
        assert_eq!((retail.window_seconds, retail.stop_angle_deg, retail.max_angle_deg, retail.dead_band_deg, retail.gain_min, retail.gain_max, retail.command_rate, retail.fallback_angle_deg, retail.block_yaw),
            (2.0, 10.0, 70.0, 5.0, 3.0, 5.0, 60.0, 120.0, true));
        set(&mut w, "dev.a", "props", Some(json!({"upright": {"window_seconds": 4.0, "block_yaw": false}}))).unwrap();
        {
            let u = w.resource::<PropTuningSettings>().0.upright;
            assert_eq!((u.window_seconds, u.block_yaw, u.gain_min), (4.0, false, 3.0));
        }
        assert_eq!(read(&w, "props")["upright"]["window_seconds"], json!(4.0));
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"window_seconds": 0.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"gain_max": -1.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"unknown": 1}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<PropTuningSettings>().0.upright, retail);
    }

    #[test]
    fn carry_move_object_speeds_set_and_reset() {
        use crate::physics::prop_carry::CarryLocomotion;
        let mut w = world();
        set(&mut w, "dev.a", "carry", Some(json!({"push_speed": 2.5, "turn_rate": 0.4, "yaw_controller": [10.0, 0.0, 20.0, 0.2], "yaw_rate_feedback": 0.0,
            "mass_speed": [[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]]}))).unwrap();
        let c = w.resource::<CarrySettings>().clone();
        assert_eq!((c.locomotion.push_speed, c.locomotion.turn_rate), (Some(2.5), Some(0.4)));
        assert_eq!(c.locomotion.pull_speed, None, "unset fields stay unset");
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        // Base from the setup data (a mod-changed base proves the overrides sit on top of it).
        let mut base = skate_core::player::offboard::move_object::MoveObjectTuning::default();
        base.pull_speed = 1.75;
        carry.set_base_tuning(base);
        c.apply_to(&mut carry);
        let l = carry.locomotion();
        assert_eq!((l.move_object.push_speed, l.move_object.pull_speed, l.turn_rate), (2.5, 1.75, Some(0.4)));
        assert_eq!(l.move_object.yaw_controller.derivative, 20.0);
        assert_eq!(l.move_object.yaw_rate_feedback, 0.0, "a mod may switch the yaw-rate feedback off");
        assert_eq!(l.move_object.mass_speed.y, [1.0; 8]);
        assert_eq!(read(&w, "carry")["turn_rate"], json!(0.4f32));
        clear_owner(&mut w, "dev.a");
        let reset = w.resource::<CarrySettings>().clone();
        assert_eq!(reset, CarrySettings::default(), "mod disable restores the retail tuning");
        reset.apply_to(&mut carry);
        assert_eq!(carry.locomotion(), CarryLocomotion::from_tuning(base));
        // Invalid values are rejected by the patch validation.
        assert!(set(&mut w, "dev.a", "carry", Some(json!({"linear_controller": [1.0, 0.0, 1.0, 2.0]}))).is_err());
        assert!(set(&mut w, "dev.a", "carry", Some(json!({"yaw_rate_feedback": -1.0}))).is_err());
    }

    /// The Move Object command rules (retail defaults) are mod knobs, per prop
    /// type blocks included, and go back to retail on mod disable.
    #[test]
    fn carry_move_command_rules_set_and_reset() {
        use crate::physics::prop_dynamics::{MoveCommandRules, PropMaterialBlocks, RETAIL_COMMANDED_MATERIAL};
        let mut w = world();
        assert_eq!(read(&w, "carry")["commanded_material"], json!(RETAIL_COMMANDED_MATERIAL));
        assert_eq!(read(&w, "carry")["apply_at_com"], json!(true));
        assert_eq!(read(&w, "carry")["upright_cos"], json!(0.65f32));
        set(&mut w, "dev.a", "carry", Some(json!({"commanded_material": [0.2, 0.0], "apply_at_com": false, "wake_on_command": false,
            "upright_cos": 0.8, "by_template": {"template/bin": {"material_held": [0.5, 0.0], "material_free": [0.9, 0.1],
            "material_free_upright": [1.0, 0.9], "upright_pair": true, "restitution": 0.25, "angular_drag": 0.5,
            "mass": 40.0, "maximum_angular_velocity": 20.0, "inertia_offset": [0.0, 0.1, 0.0]}}}))).unwrap();
        let r = w.resource::<CarrySettings>().move_rules.clone();
        assert_eq!(r.commanded_material, [0.2, 0.0]);
        assert!(!r.apply_at_com && !r.wake_on_command && r.yaw_replaces_torque && r.ignore_vertical);
        assert_eq!(r.upright_cos, 0.8);
        assert_eq!(
            r.by_template["template/bin"],
            PropMaterialBlocks {
                held: Some([0.5, 0.0]),
                free: Some([0.9, 0.1]),
                free_upright: Some([1.0, 0.9]),
                upright_pair: Some(true),
                restitution: Some(0.25),
                record_272: None,
                linear_drag: None,
                angular_drag: Some(0.5),
                mass: Some(40.0),
                maximum_linear_velocity: None,
                maximum_angular_velocity: Some(20.0),
                inertia_scale: None,
                inertia_offset: Some([0.0, 0.1, 0.0]),
                plugin_rotation_tolerance: None,
                plugin_max_distance: None,
            }
        );
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["upright_pair"], json!(true));
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["angular_drag"], json!(0.5f32));
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["material_free"], json!([0.9f32, 0.1f32]));
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<CarrySettings>().move_rules, MoveCommandRules::default(), "mod disable restores retail");
    }

    #[test]
    fn carry_buttons_and_grab_range_set_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "carry", Some(json!({"grab_bit": 21, "placement_bit": 22, "grab_range": 3.5}))).unwrap();
        let c = w.resource::<CarrySettings>().clone();
        assert_eq!((c.buttons.grab_bit, c.buttons.placement_bit, c.grab_range), (21, 22, 3.5));
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        c.apply_to(&mut carry);
        assert_eq!((carry.buttons().grab_bit, carry.grab_range()), (21, 3.5));
        clear_owner(&mut w, "dev.a");
        let d = w.resource::<CarrySettings>().clone();
        assert_eq!(d, CarrySettings::default());
        d.apply_to(&mut carry);
        assert_eq!((carry.buttons(), carry.grab_range()), (CarryButtons::default(), 2.0));
    }
}
