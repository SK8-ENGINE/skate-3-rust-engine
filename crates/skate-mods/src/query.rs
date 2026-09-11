//! Sync dynamics queries available during Lua callbacks.
//! Host (game) installs a bridge for the duration of `Manager::call`; the engine
//! still has no vehicle/car type — only bodies, rays, springs, and mass math.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skate_dynamics::{SpringRayDesc, SpringRayHit};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RaycastFilter {
    #[default]
    All,
    Ground,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RaycastOptions {
    #[serde(default = "default_raycast_distance")]
    pub max_distance: f32,
    #[serde(default)]
    pub filter: RaycastFilter,
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_raycast_distance() -> f32 {
    100.
}

impl Default for RaycastOptions {
    fn default() -> Self {
        Self {
            max_distance: default_raycast_distance(),
            filter: RaycastFilter::All,
            exclude: Vec::new(),
        }
    }
}

pub trait DynamicsHost: Send {
    fn raycast(
        &mut self,
        origin: [f32; 3],
        direction: [f32; 3],
        options: &RaycastOptions,
    ) -> Option<Value>;

    fn velocity_at(&self, key: &str, point: [f32; 3]) -> Option<[f32; 3]>;

    fn effective_inv_mass(
        &self,
        key: &str,
        point: [f32; 3],
        direction: [f32; 3],
    ) -> Option<f32>;

    fn spring_ray(&mut self, key: &str, desc: SpringRayDesc) -> Option<SpringRayHit>;

    /// World-space torque impulse realizing a body-local angular acceleration for `dt`.
    fn local_ang_accel_impulse(
        &self,
        key: &str,
        local_accel: [f32; 3],
        dt: f32,
    ) -> Option<[f32; 3]>;
}

thread_local! {
    static HOST: RefCell<Option<*mut dyn DynamicsHost>> = const { RefCell::new(None) };
}

/// Install `host` for the duration of `f` (single-threaded game loop).
pub fn with_host<R>(host: &mut dyn DynamicsHost, f: impl FnOnce() -> R) -> R {
    let ptr: *mut dyn DynamicsHost = host;
    // SAFETY: `f` runs synchronously on this thread; the pointer is cleared before return.
    let static_ptr: *mut dyn DynamicsHost = unsafe { std::mem::transmute(ptr) };
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(static_ptr);
    });
    let out = f();
    HOST.with(|slot| {
        *slot.borrow_mut() = None;
    });
    out
}

fn with_mut<R>(f: impl FnOnce(&mut dyn DynamicsHost) -> R) -> Option<R> {
    HOST.with(|slot| {
        let ptr = *slot.borrow();
        let ptr = ptr?;
        // SAFETY: pointer is set only for the duration of `with_host` on the same thread.
        Some(f(unsafe { &mut *ptr }))
    })
}

fn with_ref<R>(f: impl FnOnce(&dyn DynamicsHost) -> R) -> Option<R> {
    HOST.with(|slot| {
        let ptr = *slot.borrow();
        let ptr = ptr?;
        Some(f(unsafe { &*ptr }))
    })
}

pub fn raycast_json(
    origin: [f32; 3],
    direction: [f32; 3],
    options: RaycastOptions,
) -> Value {
    if !options.max_distance.is_finite()
        || options.max_distance <= 0.
        || options.exclude.len() > 64
    {
        return Value::Null;
    }
    with_mut(|h| h.raycast(origin, direction, &options))
        .flatten()
        .unwrap_or(Value::Null)
}

pub fn velocity_at_json(key: String, point: [f32; 3]) -> Value {
    with_ref(|h| h.velocity_at(&key, point))
        .flatten()
        .map(|v| json!(v))
        .unwrap_or(Value::Null)
}

pub fn effective_inv_mass_json(key: String, point: [f32; 3], direction: [f32; 3]) -> Value {
    with_ref(|h| h.effective_inv_mass(&key, point, direction))
        .flatten()
        .map(|v| json!(v))
        .unwrap_or(Value::Null)
}

pub fn spring_ray_json(key: String, desc: SpringRayDesc) -> Value {
    with_mut(|h| h.spring_ray(&key, desc))
        .flatten()
        .map(|hit| {
            json!({
                "in_contact": hit.in_contact,
                "point": hit.point,
                "normal": hit.normal,
                "hard_point": hit.hard_point,
                "direction_ws": hit.direction_ws,
                "suspension_length": hit.suspension_length,
                "load": hit.load,
                "relative_velocity": hit.relative_velocity,
            })
        })
        .unwrap_or(Value::Null)
}

pub fn local_ang_accel_impulse_json(key: String, local_accel: [f32; 3], dt: f32) -> Value {
    with_ref(|h| h.local_ang_accel_impulse(&key, local_accel, dt))
        .flatten()
        .map(|v| json!(v))
        .unwrap_or(Value::Null)
}
