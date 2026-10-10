//! The district's global presentation model: retail's world record points each
//! district at one extra model outside its world stream (world fields
//! `951898F6C0FA6856` model / `CA5A157A65E75934` textures, for example
//! `DIST_Water_Industrial`). It holds Industrial's sea (`ocean.reflection`
//! around the docks and harbour), the far sea planes of DownTown and University
//! (`environment.reflective_simple`) and the distant tree walls. Setup exports
//! it to `private/native-backdrops/<map>.skate` (`tools/asset_pipeline/backdrop.py`).
//!
//! Without it Industrial's harbour shows the clear colour: a black void.
//!
//! Far-proxy terrain: retail also streams `proxy<District>_100_Proxy.big`
//! (`proxyworld.default` hills plus their trees) and swaps it per grid cell:
//! while the full-detail cell `cPres_X_Z_high` is active its partner
//! `cPres_X_Z_high_proxy` is deactivated (TU3 proxy world manager
//! sub_8247EF40, streamer sub_8247BB50 -> sub_82C985A8). The engine keeps every
//! district cell loaded, so setup exports only the unpaired proxy cells to
//! `private/native-backdrops/<map>.proxy.skate` (Industrial's south hills under
//! the tree wall; DownTown and University have none). Its draws carry
//! [`ProxyTerrain`] and follow `BackdropSettings::proxy_terrain`.
//!
//! Moddability: `BackdropSettings` is the one authority; mods reach it through
//! `sdk.world.set_tuning('backdrop', {visible = false})` (world tuning domain
//! `backdrop`), and a disabled mod gives it back (retail: visible). The package
//! itself is a content file a mod can replace.
use bevy::prelude::*;
use skate_data::skate_map::SkateMap;

/// Marks every draw spawned from the backdrop package.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Backdrop;

/// Marks every draw spawned from the far-proxy terrain package.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ProxyTerrain;

/// Backdrop presentation. Retail draws both: `visible` (global presentation
/// model) and `proxy_terrain` (unpaired far-proxy cells) default to true.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropSettings {
    pub visible: bool,
    pub proxy_terrain: bool,
}

impl Default for BackdropSettings {
    fn default() -> Self {
        Self { visible: true, proxy_terrain: true }
    }
}

pub(crate) struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BackdropSettings>().add_systems(PostUpdate, apply_visibility);
    }
}

/// Settings changes reach every backdrop draw; a new map's draws get the
/// current setting when they appear.
fn apply_visibility(
    settings: Res<BackdropSettings>,
    mut draws: Query<
        (&mut Visibility, Option<Ref<Backdrop>>, Option<Ref<ProxyTerrain>>),
        Or<(With<Backdrop>, With<ProxyTerrain>)>,
    >,
) {
    let shown = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    for (mut visibility, backdrop, proxy) in &mut draws {
        let wanted = match (backdrop, proxy) {
            (Some(tag), _) if settings.is_changed() || tag.is_added() => shown(settings.visible),
            (_, Some(tag)) if settings.is_changed() || tag.is_added() => shown(settings.proxy_terrain),
            _ => continue,
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// Reads and checks `private/native-backdrops/<map>.skate`. A presentation
/// supplement: missing or invalid leaves the map without it, never fails it.
pub(crate) fn load_package(asset_root: &std::path::Path, map_name: &str) -> Option<SkateMap> {
    load(asset_root, map_name, "")
}

/// Reads and checks `private/native-backdrops/<map>.proxy.skate` (unpaired
/// far-proxy cells); missing or invalid leaves the map without it.
pub(crate) fn load_proxy_package(asset_root: &std::path::Path, map_name: &str) -> Option<SkateMap> {
    load(asset_root, map_name, ".proxy")
}

fn load(asset_root: &std::path::Path, map_name: &str, kind: &str) -> Option<SkateMap> {
    let path = asset_root
        .join("private")
        .join("native-backdrops")
        .join(format!("{map_name}{kind}.skate"));
    if !path.is_file() {
        return None;
    }
    let map = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data))
    {
        Ok(map) => map,
        Err(error) => {
            error!("SKATE_BACKDROP: {}: {error}", path.display());
            return None;
        }
    };
    if let Err(reason) = check_package(&map, map_name) {
        error!("SKATE_BACKDROP: invalid render-only package {}: {reason}", path.display());
        return None;
    }
    info!(
        "SKATE_BACKDROP: {map_name}{kind} triangles={} materials={}",
        map.geometry.indices.len() / 3,
        map.materials.len()
    );
    Some(map)
}

/// Presentation only: never route collision, lights, doors or rails from it.
fn check_package(map: &SkateMap, map_name: &str) -> Result<(), &'static str> {
    if map.name != map_name {
        return Err("map name differs");
    }
    if !map.geometry.collision.is_empty()
        || !map.lights.is_empty()
        || !map.doors.is_empty()
        || !map.rails.is_empty()
    {
        return Err("carries gameplay data");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_visibility_follows_settings_and_new_draws() {
        let mut app = App::new();
        app.add_plugins(BackdropPlugin);
        let first = app.world_mut().spawn((Backdrop, Visibility::Inherited)).id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(first).unwrap(), Visibility::Inherited, "retail draws it");
        app.world_mut().resource_mut::<BackdropSettings>().visible = false;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(first).unwrap(), Visibility::Hidden);
        // A map loaded while hidden spawns hidden.
        let later = app.world_mut().spawn((Backdrop, Visibility::Inherited)).id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(later).unwrap(), Visibility::Hidden);
        *app.world_mut().resource_mut::<BackdropSettings>() = BackdropSettings::default();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(first).unwrap(), Visibility::Inherited);
        assert_eq!(*app.world().get::<Visibility>(later).unwrap(), Visibility::Inherited);
    }

    #[test]
    fn proxy_terrain_visibility_is_its_own_switch() {
        let mut app = App::new();
        app.add_plugins(BackdropPlugin);
        let model = app.world_mut().spawn((Backdrop, Visibility::Inherited)).id();
        let hills = app.world_mut().spawn((ProxyTerrain, Visibility::Inherited)).id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(hills).unwrap(), Visibility::Inherited, "retail draws it");
        app.world_mut().resource_mut::<BackdropSettings>().proxy_terrain = false;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(hills).unwrap(), Visibility::Hidden);
        assert_eq!(*app.world().get::<Visibility>(model).unwrap(), Visibility::Inherited);
        let later = app.world_mut().spawn((ProxyTerrain, Visibility::Inherited)).id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(later).unwrap(), Visibility::Hidden);
        *app.world_mut().resource_mut::<BackdropSettings>() = BackdropSettings::default();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(hills).unwrap(), Visibility::Inherited);
        assert_eq!(*app.world().get::<Visibility>(later).unwrap(), Visibility::Inherited);
    }

    #[test]
    fn missing_package_is_not_an_error() {
        assert!(load_package(std::path::Path::new("does-not-exist"), "Industrial").is_none());
        assert!(load_proxy_package(std::path::Path::new("does-not-exist"), "Industrial").is_none());
    }

    /// Asset-backed: Industrial's backdrop holds the harbour sea where the
    /// district has none (the black void at the docks, x -400..-560,
    /// z -70..-270). `SKATE3_ASSET_ROOT=<assets>`.
    #[test]
    #[ignore]
    fn industrial_backdrop_covers_the_docks_sea() {
        let root = std::path::PathBuf::from(std::env::var("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let map = load_package(&root, "Industrial").expect("Industrial backdrop package");
        let shader = |m: &skate_data::skate_map::Material| {
            let b = m.retail_definition.as_deref()?;
            let n = u32::from_le_bytes(b.get(16..20)?.try_into().ok()?) as usize;
            Some(String::from_utf8_lossy(b.get(20..20 + n)?).into_owned())
        };
        // The sea surface (`ocean.default`) must lie under open water next to
        // the docks (points checked against the 2026-10-07 session path);
        // the reflection sheets alone are not the sea.
        let docks = [[-552.6, -80.4], [-600.0, -150.0], [-580.0, -120.0], [-540.0, -200.0], [-460.7, -229.7]];
        let mut covered = [false; 5];
        let mut reflections = 0;
        for tri in map.geometry.indices.chunks_exact(3) {
            let v = &map.geometry.vertices[tri[0] as usize];
            let Some(m) = (v.material as usize).checked_sub(1).and_then(|i| map.materials.get(i)) else { continue };
            let p: [[f32; 2]; 3] = core::array::from_fn(|k| {
                let q = map.geometry.vertices[tri[k] as usize].position;
                [q[0], q[2]]
            });
            match shader(m).as_deref() {
                Some("ocean.default") => {
                    for (d, hit) in docks.iter().zip(covered.iter_mut()) {
                        let side = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
                        let s = [side(p[0], p[1]), side(p[1], p[2]), side(p[2], p[0])];
                        *hit |= s.iter().all(|v| *v >= 0.) || s.iter().all(|v| *v <= 0.);
                    }
                }
                Some("ocean.reflection") => reflections += 1,
                _ => {}
            }
        }
        assert!(covered.iter().all(|c| *c), "sea surface missing under docks points: {covered:?}");
        assert!(reflections > 0, "no ocean.reflection sheets");
    }
}
