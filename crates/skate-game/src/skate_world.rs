//! Authored `.skate` world geometry → renderable scene.
//!
//! The draw-count architecture lives here (RFC 1 §1–§6). `.skate` vertices
//! already carry a per-vertex material index, so triangles of different
//! materials can share a mesh as long as the shader indexes its material table
//! with that attribute. Geometry is therefore grouped by **space and render
//! class**, not by material, and draw count stops scaling with material count.
//!
//! Collision is a separate concern that happens to live in the same file
//! because it reads the same package. It is byte-for-byte the behaviour physics
//! was validated against and must not be "improved" here.
use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexFormat},
    prelude::*,
};
use skate_core::{
    math::Vector3,
    physics::{
        board_world::{
            BoardWorld, WorldTriangle,
            query_metadata::{Bounds, QueryMesh, QueryMetadata, QueryPool},
        },
        collision::TriangleFeature,
        contact::RetailContactMaterial,
        drive_frames::RetailAffineTransform,
    },
};
use skate_data::skate_map::SkateMap;
use std::collections::HashMap;

use crate::map_render::{AssetSink, SceneCommands};
use crate::retail_render::{MaterialTable, RenderClass, WorldMaterial};

/// Per-vertex index into the world material table.
///
/// `@interpolate(flat)` in the shader. Interpolating it would silently corrupt
/// material lookup across triangle interiors, which is the one failure mode of
/// this design that does not announce itself.
pub(crate) const ATTRIBUTE_MATERIAL_INDEX: MeshVertexAttribute =
    MeshVertexAttribute::new("MaterialIndex", 0x534B_4D49, VertexFormat::Uint32);

/// Triangles per spatial leaf. The single tuning dial trading draw count against
/// culling granularity (RFC 1 §5): ~1.9M Downtown triangles at this size gives
/// ~120 leaves.
const LEAF_TRIANGLE_BUDGET: usize = 16_384;

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct SceneStats {
    pub leaves: usize,
    pub draws: usize,
    pub triangles: usize,
    pub slabs: usize,
}

// ---------------------------------------------------------------------------
// Render path
// ---------------------------------------------------------------------------

/// One triangle, reduced to what partitioning needs.
struct Triangle {
    indices: [u32; 3],
    centroid: Vec3,
    /// Slab owning this triangle's material; a leaf spanning two slabs must split.
    slab: u16,
    class: RenderClass,
}

/// Static geometry of one package, partitioned into draws; `tag` goes on
/// every draw.
#[allow(clippy::too_many_arguments)]
fn spawn_static<T: Bundle + Clone>(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
    tag: T,
) -> (SceneStats, [usize; RenderClass::ALL.len()]) {
    let table = MaterialTable::build(map, tuning, environment, materials, images, buffers);

    let mut triangles: Vec<Triangle> = Vec::with_capacity(map.geometry.indices.len() / 3);
    for tri in map.geometry.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]];
        // The format guarantees all three corners share a material, and the
        // loader enforces it, so corner 0 is authoritative. `Vertex::material`
        // is one-based with zero meaning "no material", matching the collision
        // path below and `map.materials` indexing everywhere else.
        let Some(source) = (map.geometry.vertices[a as usize].material as usize).checked_sub(1)
        else {
            continue;
        };
        let Some(entry) = table.entry(source) else { continue };
        let position = |i: u32| Vec3::from_array(map.geometry.vertices[i as usize].position);
        triangles.push(Triangle {
            indices: [a, b, c],
            centroid: (position(a) + position(b) + position(c)) / 3.0,
            slab: entry.slab,
            class: entry.class,
        });
    }

    // Partition by render class first: classes cannot share a draw (RFC 1 §4),
    // so splitting here keeps every leaf single-class for free.
    let mut stats = SceneStats { slabs: table.slab_count(), triangles: triangles.len(), ..default() };
    let mut buckets: HashMap<(RenderClass, u16), Vec<usize>> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        buckets.entry((triangle.class, triangle.slab)).or_default().push(index);
    }

    // Draws per render class. The class mix is what decides whether the draw
    // budget holds on a big map: spatial subdivision is a number we choose, but
    // every two-sided or blended material forces an extra bucket we do not.
    let mut per_class = [0usize; RenderClass::ALL.len()];
    for ((class, slab), mut members) in buckets {
        let leaves = partition(&mut members, &triangles);
        stats.leaves += leaves.len();
        per_class[class as usize] += leaves.len();
        for leaf in leaves {
            let mesh = merge(map, &table, &triangles, &leaf);
            let aabb = mesh.1;
            commands.spawn((
                Name::new(format!("world {class:?} slab {slab}")),
                Mesh3d(meshes.add(mesh.0)),
                MeshMaterial3d(table.material(slab, class)),
                Transform::default(),
                // Precomputed so Bevy's CalculateBounds never walks this mesh
                // (RFC 1 D4). The extents were already computed while merging.
                aabb,
                tag.clone(),
            ));
            stats.draws += 1;
        }
    }
    (stats, per_class)
}

/// The district's global presentation model (`private/native-backdrops/<map>.skate`:
/// ocean surfaces, distant tree walls, far sea planes), drawn through the same
/// retail material path as the district. Every draw carries
/// [`crate::retail_backdrop::Backdrop`] so its visibility follows
/// `BackdropSettings` (mod-reachable). No lights, no collision.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_backdrop(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_backdrop").entered();
    let tag = crate::retail_backdrop::Backdrop;
    spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, tag).0
}

/// The unpaired far-proxy cells (`private/native-backdrops/<map>.proxy.skate`,
/// see [`crate::retail_backdrop`]): every draw carries
/// [`crate::retail_backdrop::ProxyTerrain`]. No lights, no collision.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_proxy_terrain(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_proxy_terrain").entered();
    let tag = crate::retail_backdrop::ProxyTerrain;
    spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, tag).0
}

pub(crate) fn spawn(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_world").entered();
    let (stats, per_class) =
        spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, ());
    spawn_lights(map, commands);
    eprintln!(
        "SKATE_RENDER_READY draws={} triangles={} slabs={} materials={} \
         opaque={} opaque_two_sided={} cutout={} cutout_two_sided={} \
         blended={} blended_two_sided={}",
        stats.draws,
        stats.triangles,
        stats.slabs,
        map.materials.len(),
        per_class[RenderClass::Opaque as usize],
        per_class[RenderClass::OpaqueTwoSided as usize],
        per_class[RenderClass::Cutout as usize],
        per_class[RenderClass::CutoutTwoSided as usize],
        per_class[RenderClass::Blended as usize],
        per_class[RenderClass::BlendedTwoSided as usize],
    );
    stats
}

/// Median-split over triangle centroids until every leaf fits the budget.
///
/// Sorting by the longest axis of the current extent keeps leaves roughly cubic,
/// which matters because leaf AABBs are what frustum culling tests.
fn partition(members: &mut Vec<usize>, triangles: &[Triangle]) -> Vec<Vec<usize>> {
    let mut pending = vec![std::mem::take(members)];
    let mut leaves = Vec::new();
    while let Some(mut group) = pending.pop() {
        if group.len() <= LEAF_TRIANGLE_BUDGET {
            leaves.push(group);
            continue;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &i in &group {
            min = min.min(triangles[i].centroid);
            max = max.max(triangles[i].centroid);
        }
        let extent = max - min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        group.sort_unstable_by(|&a, &b| {
            triangles[a].centroid[axis].total_cmp(&triangles[b].centroid[axis])
        });
        let half = group.split_off(group.len() / 2);
        pending.push(group);
        pending.push(half);
    }
    leaves
}

/// Concatenate a leaf's triangles into one mesh, preserving each vertex's
/// material index so the shader can still tell them apart.
fn merge(
    map: &SkateMap,
    table: &MaterialTable,
    triangles: &[Triangle],
    leaf: &[usize],
) -> (Mesh, Aabb) {
    let mut remap: HashMap<u32, u32> = HashMap::with_capacity(leaf.len() * 2);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(leaf.len() * 2);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(leaf.len() * 2);
    let mut uv: Vec<[f32; 2]> = Vec::with_capacity(leaf.len() * 2);
    let mut lightmap_uv: Vec<[f32; 2]> = Vec::with_capacity(leaf.len() * 2);
    let mut decal_uv: Vec<[f32; 4]> = Vec::with_capacity(leaf.len() * 2);
    let mut material_index: Vec<u32> = Vec::with_capacity(leaf.len() * 2);
    let mut tangents: Vec<[f32; 4]> = Vec::with_capacity(leaf.len() * 2);
    let mut indices: Vec<u32> = Vec::with_capacity(leaf.len() * 3);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for &triangle in leaf {
        for source in triangles[triangle].indices {
            let next = positions.len() as u32;
            let index = *remap.entry(source).or_insert(next);
            if index == next {
                let v = &map.geometry.vertices[source as usize];
                let position = Vec3::from_array(v.position);
                min = min.min(position);
                max = max.max(position);
                positions.push(v.position);
                normals.push(v.normal);
                uv.push(v.uv);
                lightmap_uv.push(v.lightmap_uv);
                // Decal UVs ride in COLOR to match the retail shader, which
                // reads them from `color.xy`.
                let [du, dv] = v.decal_uv.unwrap_or_default();
                decal_uv.push([du, dv, 0.0, 1.0]);
                // One-based, as in the bucketing pass above. Triangles whose
                // material is absent were dropped there, so every vertex
                // reachable here has one.
                material_index
                    .push(table.slab_index(v.material.saturating_sub(1) as usize));
                tangents.push(tangent(v));
            }
            indices.push(index);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, decal_uv)
        .with_inserted_attribute(ATTRIBUTE_MATERIAL_INDEX, material_index)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_indices(Indices::U32(indices));
    let aabb = Aabb::from_min_max(min, max);
    (mesh, aabb)
}

/// Decodes the authored tangent frame, which the format stores as a signed-byte
/// binormal plus a handedness byte, both scaled by 127.
///
/// A merged mesh spans many materials, so unlike a per-material mesh it cannot
/// choose between authored and generated tangents for the whole draw. Vertices
/// with no authored frame get a zero tangent instead, which both Bevy's
/// local-to-world helper and the shader read as "fall back to derivatives" —
/// a per-vertex decision rather than a per-mesh one.
fn tangent(v: &skate_data::skate_map::Vertex) -> [f32; 4] {
    let Some(frame) = v.tangent_frame else {
        return [0.0; 4];
    };
    let frame = frame.map(|b| (b as i8 as f32 / 127.).max(-1.));
    let binormal = Vec3::new(frame[0], frame[1], frame[2]);
    let tangent = binormal.cross(Vec3::from_array(v.normal)) * frame[3];
    [tangent.x, tangent.y, tangent.z, frame[3]]
}

fn spawn_lights(map: &SkateMap, commands: &mut SceneCommands) {
    for light in &map.lights {
        let position = Vec3::from_array(light.position);
        let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
        match light.kind {
            0 => commands.spawn((
                PointLight {
                    color,
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    // RFC 1 D5: no shadow casting anywhere in v1.
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_translation(position),
            )),
            1 => commands.spawn((
                SpotLight {
                    color,
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    outer_angle: light.outer_cos.clamp(-1., 1.).acos(),
                    inner_angle: light.inner_cos.clamp(-1., 1.).acos(),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_translation(position)
                    .looking_to(Vec3::from_array(light.direction), Vec3::Y),
            )),
            // Area lights are parsed but have no Bevy equivalent; validation
            // already warned about them.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic props (DMO instances)
//
// Ported from SK8-ENGINE PR #15 (laaledesiempre, phase 0) onto the current
// renderer: each MOBJ record becomes one root entity with its own transform,
// and its template geometry is merged per (render class, slab) like the
// static world, but in template space and shared between instances.
// ---------------------------------------------------------------------------

/// Marker on the root entity of one spawned dynamic-prop (DMO) instance.
/// Later phases move these entities; static batches never contain them.
#[derive(Component)]
pub(crate) struct PropInstance {
    pub id: u32,
    pub template: String,
    pub name: String,
}

/// The district's movable-prop package (`private/native-props/<map>.skate`)
/// and its MOBJ placements. The package is a presentation supplement: a
/// missing or invalid file leaves the map without props instead of failing it.
pub(crate) fn load_prop_package(
    asset_root: &std::path::Path,
    map_name: &str,
) -> Option<(SkateMap, Vec<skate_data::skate_map::StaticObject>)> {
    let path = asset_root
        .join("private")
        .join("native-props")
        .join(format!("{map_name}.skate"));
    if !path.is_file() {
        return None;
    }
    let map = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data))
    {
        Ok(map) => map,
        Err(error) => {
            error!("SKATE_PROPS: {}: {error}", path.display());
            return None;
        }
    };
    // Presentation only; never route lights, doors or rails from it.
    if map.name != map_name
        || !map.geometry.collision.is_empty()
        || !map.lights.is_empty()
        || !map.doors.is_empty()
        || !map.rails.is_empty()
    {
        error!("SKATE_PROPS: invalid render-only package {}", path.display());
        return None;
    }
    let mut objects = Vec::new();
    for extension in map.extensions.iter().filter(|e| e.tag == *b"MOBJ") {
        match skate_data::skate_map::parse_static_objects(&map, extension) {
            Ok(parsed) => objects.extend(parsed),
            Err(error) => {
                error!("SKATE_PROPS: {}: {error}", path.display());
                return None;
            }
        }
    }
    if objects.is_empty() {
        // Packages written before MOBJ schema 4 bake every prop into one
        // static mesh with no placements; setup groups maps/environment
        // rewrite them.
        warn!(
            "SKATE_PROPS: {} has no prop placements; re-run setup (maps) to get movable props",
            path.display()
        );
        return None;
    }
    Some((map, objects))
}

/// Row-vector affine (v @ basis + translation) as a column-vector matrix:
/// the basis rows become the matrix columns.
pub(crate) fn prop_affine(t: &[f32; 12]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(t[0], t[1], t[2], 0.),
        Vec4::new(t[3], t[4], t[5], 0.),
        Vec4::new(t[6], t[7], t[8], 0.),
        Vec4::new(t[9], t[10], t[11], 1.),
    )
}

/// One template's render parts: a merged template-space mesh per
/// (render class, slab), with its bounds.
type PropPart = (RenderClass, u16, Handle<Mesh>, Aabb);

fn prop_template_parts(
    map: &SkateMap,
    table: &MaterialTable,
    object: &skate_data::skate_map::StaticObject,
    meshes: &mut impl AssetSink<Mesh>,
) -> Vec<PropPart> {
    let start = object.first_index as usize;
    let end = start + object.index_count as usize;
    let Some(range) = map.geometry.indices.get(start..end) else {
        return Vec::new();
    };
    let mut triangles = Vec::with_capacity(range.len() / 3);
    for tri in range.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]];
        let Some(source) = (map.geometry.vertices[a as usize].material as usize).checked_sub(1)
        else {
            continue;
        };
        let Some(entry) = table.entry(source) else { continue };
        let position = |i: u32| Vec3::from_array(map.geometry.vertices[i as usize].position);
        triangles.push(Triangle {
            indices: [a, b, c],
            centroid: (position(a) + position(b) + position(c)) / 3.0,
            slab: entry.slab,
            class: entry.class,
        });
    }
    // First-seen order keeps the spawn order deterministic.
    let mut buckets: Vec<((RenderClass, u16), Vec<usize>)> = Vec::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let key = (triangle.class, triangle.slab);
        match buckets.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(index),
            None => buckets.push((key, vec![index])),
        }
    }
    buckets
        .into_iter()
        .map(|((class, slab), members)| {
            let (mesh, aabb) = merge(map, table, &triangles, &members);
            (class, slab, meshes.add(mesh), aabb)
        })
        .collect()
}

/// Spawn one root entity per MOBJ object. Template meshes are built once per
/// geometry range and shared by every instance that places it.
pub(crate) fn spawn_instances(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> usize {
    let _span = info_span!("spawn_prop_instances").entered();
    let table = MaterialTable::build(map, tuning, environment, materials, images, buffers);
    let mut templates: HashMap<(u32, u32), Vec<PropPart>> = HashMap::new();
    let mut draws = 0;
    for object in objects {
        let parts = templates
            .entry((object.first_index, object.index_count))
            .or_insert_with(|| prop_template_parts(map, &table, object, meshes));
        // The exporter prefixes the template ID to the authored locator name.
        let (template, name) = object
            .name
            .split_once('/')
            .unwrap_or(("", object.name.as_str()));
        let children: Vec<_> = parts
            .iter()
            .map(|(class, slab, mesh, aabb)| {
                (
                    Name::new(format!("{} {class:?} slab {slab}", object.name)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(table.material(*slab, *class)),
                    Transform::default(),
                    *aabb,
                )
            })
            .collect();
        draws += children.len();
        commands.spawn_with_children(
            (
                Name::new(object.name.clone()),
                PropInstance {
                    id: object.id,
                    template: template.to_string(),
                    name: name.to_string(),
                },
                Transform::from_matrix(prop_affine(&object.transform)),
                Visibility::default(),
            ),
            children,
        );
    }
    eprintln!(
        "SKATE_PROP_INSTANCES count={} templates={} draws={draws}",
        objects.len(),
        templates.len()
    );
    objects.len()
}

// ---------------------------------------------------------------------------
// Validation and collision
//
// Everything below is carried over unchanged. Physics was validated against this
// exact behaviour, including the bit patterns and epsilons, and RFC 1 §10.1 puts
// it out of scope for the renderer rewrite.
// ---------------------------------------------------------------------------

pub(crate) fn validate_runtime(map: &SkateMap) -> Result<(), String> {
    let _span = info_span!("validate_map").entered();
    let archive = retail_archive(map)?;
    if map.geometry.collision.is_empty() && archive.is_none() {
        return Err("SKATE map has no collision geometry".into());
    }
    for triangle in map.geometry.collision.iter().filter(|_| archive.is_none()) {
        if let Some(edges) = triangle.native_edges {
            decode_native_edges(edges)?;
        }
    }
    if !map.doors.is_empty() {
        return Err(format!(
            "Map '{}' contains {} hinged doors. This imported game has no door body/controller adapter yet; refusing to drop their geometry or turn them into static walls.",
            map.name,
            map.doors.len()
        ));
    }
    for extension in &map.extensions {
        let tag = String::from_utf8_lossy(&extension.tag);
        if extension.tag == *b"RWCM" {
            continue;
        }
        if extension.tag == skate_data::trigger_volumes::EXTENSION_TAG {
            // Named trigger volumes (data only, never collision); see trigger_volumes.rs.
            if extension.schema != skate_data::trigger_volumes::EXTENSION_SCHEMA {
                return Err(format!("SKATE extension TVOL schema {} is not supported", extension.schema));
            }
            skate_data::trigger_volumes::parse(&extension.payload)?;
            continue;
        }
        if extension.tag == *b"MOBJ" {
            skate_data::skate_map::validate_static_objects(map, extension)?;
            continue;
        }
        if extension.tag == *b"SKYB" && extension.schema == 1 {
            eprintln!(
                "SKATE LIMITATION: SKYB retail sky retained; using the map horizon until its shader adapter is available."
            );
            continue;
        }
        if extension.tag != *b"WMET" && extension.tag != *b"WCFG" && extension.tag != *b"BMAT" {
            return Err(format!(
                "SKATE extension {tag} schema {} is decoded but its runtime adapter is not implemented. Refusing to silently omit potentially required world geometry.",
                extension.schema
            ));
        }
        eprintln!(
            "SKATE LIMITATION: {tag} extension retained; its runtime behavior is not connected."
        );
    }
    if map.textures.iter().any(|t| t.width == 0) {
        return Err(
            "SKATE contains external texture placeholders; supply a package with embedded textures"
                .into(),
        );
    }
    if !map.routes.is_empty() {
        eprintln!(
            "SKATE LIMITATION: {} NPC routes parsed; supplied game has no NPC controller.",
            map.routes.len()
        );
    }
    if map.lights.iter().any(|l| l.kind == 2) {
        eprintln!(
            "SKATE LIMITATION: area-light records retained; Bevy adapter currently renders point and spot lights only."
        );
    }
    eprintln!(
        "SKATE_MAP_LOADED name={:?} version={} render_triangles={} collision_triangles={} textures={} spawn={:?}",
        map.name,
        map.version,
        map.geometry.indices.len() / 3,
        map.geometry.collision.len(),
        map.textures.len(),
        map.spawn
    );
    Ok(())
}

/// TU3 ClusteredMesh::GetUnitVolumes (82AC8A68): fdivs then fsubs,
/// using the pi-squared word at 822F88D0. This is not acos/angle decoding.
/// Bit 7 denotes an unmatched compiler edge and is not a triangle flag.
fn decode_native_edges(edges: [u8; 3]) -> Result<(u32, [f32; 3]), String> {
    let mut flags = 1 | TriangleFeature::ONE_SIDED | TriangleFeature::USE_EDGE_COSINES;
    let mut cosines = [0.; 3];
    for (i, code) in edges.into_iter().enumerate() {
        let exponent = code & 0x1f;
        // The native signed 32-bit shift becomes negative at 28 and zero
        // above it. Reject those malformed codes instead of producing NaNs.
        if exponent >= 28 {
            return Err(format!(
                "Invalid SKATE native edge angle code {exponent} at corner {i}"
            ));
        }
        cosines[i] = 1.0 - f32::from_bits(0x411d_e9e7) / ((8_u32 << exponent) as f32);
        flags |= u32::from(code & 0x20) << i;
        flags |= u32::from(code & 0x40) << (i + 3);
    }
    Ok((flags, cosines))
}

fn retail_archive(map: &SkateMap) -> Result<Option<&[u8]>, String> {
    let mut archives = map.extensions.iter().filter(|e| e.tag == *b"RWCM");
    let Some(archive) = archives.next() else {
        return Ok(None);
    };
    if archive.schema != 1 || archives.next().is_some() {
        return Err("SKATE requires one RWCM extension with schema 1".into());
    }
    Ok(Some(&archive.payload))
}

fn retail_collision_world(
    archive: &[u8],
    material: RetailContactMaterial,
) -> Result<BoardWorld, String> {
    let mut triangles = Vec::new();
    let mut packed_surfaces = Vec::new();
    let mut meshes = Vec::new();
    let count = skate_data::retail_collision::visit_clusters(archive, |_, cluster| {
        // Preserve cluster order and partition further only for group filters.
        let mut cursor = 0;
        while cursor < cluster.len() {
            let group = cluster[cursor].group;
            let start = triangles.len();
            while cursor < cluster.len() && cluster[cursor].group == group {
                let source = cluster[cursor];
                let (flags, cosines) = match source.edges {
                    Some(edges) => {
                        let (mut flags, cosines) = decode_native_edges(edges)?;
                        flags &= !TriangleFeature::ONE_SIDED;
                        if source.one_sided {
                            flags |= TriangleFeature::ONE_SIDED;
                        }
                        (flags, cosines)
                    }
                    // TriangleVolume ctor82AC7770 retains these defaults when
                    // the unit has no edge data; the mesh sidedness is not read.
                    None => (0x1e1, [-1.; 3]),
                };
                triangles.push(
                    WorldTriangle::from_vertices(
                        source.points.map(|p| Vector3::new(p[0], p[1], p[2])),
                        material,
                        u32::from(source.surface),
                        flags,
                        cosines,
                        0.,
                    )
                    .ok_or("Invalid RWCM collision triangle")?,
                );
                packed_surfaces.push(source.surface);
                cursor += 1;
            }
            let range = start..triangles.len();
            let bounds = Bounds::from_points(
                triangles[range.clone()]
                    .iter()
                    .flat_map(|t| t.triangle.vertices),
            )
            .ok_or("Invalid RWCM cluster bounds")?;
            meshes.push(QueryMesh {
                geometry: 0, rejection_flags: 0,
                triangle_range: range,
                local_to_world: RetailAffineTransform::IDENTITY,
                world_to_local: RetailAffineTransform::IDENTITY,
                local_bounds: bounds,
                // Native static registration uses matchingID -1; the unit group
                // only splits clusters and must not filter actor queries.
                matching_group: -1,
                pool: QueryPool::Ground,
            });
        }
        Ok(())
    })?;
    eprintln!(
        "SKATE_RWCM_READY triangles={count} query_clusters={} source=embedded",
        meshes.len()
    );
    BoardWorld::with_query_metadata(
        triangles,
        QueryMetadata {
            packed_surfaces,
            meshes,
            static_edges: vec![],
            island_flags: 0,
        },
    )
    .map_err(str::to_owned)
}

pub(crate) fn collision_world(
    map: &SkateMap,
    material: RetailContactMaterial,
) -> Result<BoardWorld, String> {
    if let Some(archive) = retail_archive(map)? {
        return retail_collision_world(archive, material);
    }
    portable_world(&map.geometry.collision, &map.materials, material)
}

/// Welded vertices and reconstructed edge features (flags, cosines) of a
/// triangle soup: the 1 mm welding and edge pairing shared by the portable map
/// world and prop instances created mid-game. `reconstruct` = false keeps the
/// default features (fully native maps decode theirs per triangle).
fn welded_edge_features(
    triangles: &[[[f32; 3]; 3]],
    reconstruct: bool,
) -> Result<(Vec<Vec3>, Vec<[usize; 3]>, Vec<u32>, Vec<[f32; 3]>), String> {
    // Match the reference RW mesh compiler's 1 mm vertex welding and reversed
    // edge pairing. Triangle diagonals are adjacency, never authored ledges.
    let mut welded = HashMap::<[i64; 3], usize>::new();
    let mut positions = Vec::<Vec3>::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    for points in triangles {
        let ids = points.map(|p| {
            let inverse = 1.0 / f64::from(0.001_f32);
            let key = p.map(|v| (f64::from(v) * inverse).round() as i64);
            *welded.entry(key).or_insert_with(|| {
                let id = positions.len();
                positions.push(Vec3::from_array(p));
                id
            })
        });
        let [a, b, c] = points.map(Vec3::from_array);
        let normal = (b - a)
            .cross(c - a)
            .try_normalize()
            .ok_or("Invalid SKATE collision triangle normal")?;
        vertices.push(ids);
        normals.push(normal);
    }
    let mut cosines = vec![[1.; 3]; vertices.len()];
    let mut flags =
        vec![TriangleFeature::ONE_SIDED | TriangleFeature::USE_EDGE_COSINES | 0xe0; vertices.len()];
    // Fully native maps need no reconstructed adjacency. Mixed maps still
    // include every face when finding neighbors for their authored geometry.
    if reconstruct {
        let mut open = HashMap::<(usize, usize), (usize, usize)>::new();
        for (i, ids) in vertices.iter().enumerate() {
            for edge in 0..3 {
                let (a, b) = (ids[edge], ids[(edge + 1) % 3]);
                if let Some((other, oe)) = open.remove(&(b, a)) {
                    let cosine = normals[i].dot(normals[other]).clamp(-1., 1.);
                    let orientation =
                        (positions[b] - positions[a]).dot(normals[i].cross(normals[other]));
                    // ExtendedEdgeCosine / MakeEdgeCode in rw_collision_mesh.cpp:
                    // orientation >= -1e-6 is convex; flat edges have no convex bit.
                    for (ti, e) in [(i, edge), (other, oe)] {
                        cosines[ti][e] = cosine;
                        if orientation <= -1.0e-6 || cosine >= 1. {
                            flags[ti] &= !(0x20 << e);
                        }
                    }
                } else {
                    open.entry((a, b)).or_insert((i, edge));
                }
            }
        }
        let mut adjacent = vec![Vec::new(); positions.len()];
        for (i, ids) in vertices.iter().enumerate() {
            for &v in ids {
                adjacent[v].push(i);
            }
        }
        for (v, faces) in adjacent.iter().enumerate() {
            let reference = normals[faces[0]];
            if faces
                .iter()
                .all(|&i| (reference.dot(normals[i]) - 1.).abs() <= 0.01)
            {
                for &i in faces {
                    for corner in 0..3 {
                        if vertices[i][corner] == v {
                            flags[i] |= 0x200 << corner;
                        }
                    }
                }
            }
        }
    }
    Ok((positions, vertices, flags, cosines))
}

/// Portable triangle world: 1 mm vertex welding, reconstructed adjacency and
/// contiguous-range broadphase metadata. Shared by the playable map and by
/// static prop instances, which supply already-placed world-space triangles.
fn portable_world(
    collision: &[skate_data::skate_map::Collision],
    materials: &[skate_data::skate_map::Material],
    material: RetailContactMaterial,
) -> Result<BoardWorld, String> {
    let points: Vec<_> = collision.iter().map(|t| t.points).collect();
    let (positions, vertices, mut flags, mut cosines) =
        welded_edge_features(&points, collision.iter().any(|t| t.native_edges.is_none()))?;
    let mut triangles = Vec::with_capacity(vertices.len());
    let mut packed_surfaces = Vec::with_capacity(vertices.len());
    for (i, source) in collision.iter().enumerate() {
        if let Some(edges) = source.native_edges {
            (flags[i], cosines[i]) = decode_native_edges(edges)?;
        }
        let m = &materials[source.material as usize - 1];
        // Exact EncodeRwSurfaceId mapping from the reference native adapter.
        packed_surfaces.push((m.audio | (m.physics << 7) | (m.pattern << 12)) as u16);
        let points = vertices[i].map(|id| {
            let p = positions[id];
            Vector3::new(p.x, p.y, p.z)
        });
        // Keep the supplied game's original static-world contact combine values.
        // The native map bridge supplies packed surfaces, not a guessed split of
        // the package's single friction scalar into static/dynamic coefficients.
        triangles.push(
            WorldTriangle::from_vertices(
                points,
                material,
                source.surface,
                flags[i],
                cosines[i],
                0.,
            )
            .ok_or("Invalid SKATE collision volume")?,
        );
    }
    // Portable maps have no native cluster hierarchy. Bound contiguous ranges
    // once at load time so the existing BVH can reject distant geometry. Keep
    // triangle order and mesh identity/filter values: contact tie-breaking,
    // packed surfaces and adjacency must not change with this acceleration.
    let mut meshes = Vec::new();
    for start in (0..triangles.len()).step_by(64) {
        let end = (start + 64).min(triangles.len());
        let bounds = Bounds::from_points(triangles[start..end].iter().flat_map(|t| t.triangle.vertices))
            .ok_or("SKATE collision bounds empty")?;
        meshes.push(QueryMesh {
            geometry: 0, rejection_flags: 0,
            triangle_range: start..end,
            local_to_world: RetailAffineTransform::IDENTITY,
            world_to_local: RetailAffineTransform::IDENTITY,
            local_bounds: bounds,
            matching_group: -1,
            pool: QueryPool::Ground,
        });
    }
    let metadata = QueryMetadata {
        packed_surfaces,
        meshes,
        static_edges: vec![],
        island_flags: 0,
    };
    BoardWorld::with_query_metadata(triangles, metadata).map_err(str::to_owned)
}

/// One prop instance's share of the prop collision world: its world triangle
/// range plus the winding-fixed template-space source triangles. Rigid-motion
/// invariance of adjacency flags and edge cosines lets `rebake` skip welding.
pub(crate) struct PropCollisionInstance {
    pub id: u32,
    /// Index of the originating MOBJ record; `usize::MAX` for an instance
    /// added mid-game ([`PropCollisionLayer::add_instance`]).
    pub object: usize,
    pub range: std::ops::Range<usize>,
    local: Vec<[Vector3; 3]>,
}

impl PropCollisionInstance {
    /// Winding-fixed template-space triangles with per-axis scale folded in.
    pub fn local_points(&self) -> &[[Vector3; 3]] {
        &self.local
    }
}

/// Static collision for spawned DMO prop instances, kept out of the map
/// collision world so dynamic instances can be rebaked independently.
pub(crate) struct PropCollisionLayer {
    world: BoardWorld,
    instances: Vec<PropCollisionInstance>,
    contact_material: RetailContactMaterial,
    /// Instances added mid-game and retired since; their parked triangle
    /// ranges are reused by the next instance of the same size.
    retired: Vec<usize>,
}

impl PropCollisionLayer {
    pub fn world(&self) -> &BoardWorld {
        &self.world
    }
    pub fn world_mut(&mut self) -> &mut BoardWorld {
        &mut self.world
    }
    pub fn instances(&self) -> &[PropCollisionInstance] {
        &self.instances
    }

    /// Re-bake one instance at a new rigid pose (rotation basis columns plus
    /// translation of the template origin). Scale, if any, is baked into the
    /// local triangles at load and is not reapplied here.
    pub fn rebake(
        &mut self,
        instance: usize,
        basis: [[f32; 3]; 3],
        translation: Vector3,
    ) -> Result<(), String> {
        let entry = &self.instances[instance];
        let transform = |p: Vector3| {
            Vector3::new(
                p.x * basis[0][0] + p.y * basis[1][0] + p.z * basis[2][0] + translation.x,
                p.x * basis[0][1] + p.y * basis[1][1] + p.z * basis[2][1] + translation.y,
                p.x * basis[0][2] + p.y * basis[1][2] + p.z * basis[2][2] + translation.z,
            )
        };
        let mut triangles = Vec::with_capacity(entry.local.len());
        for (i, &local) in entry.local.iter().enumerate() {
            let source = self.world.triangles()[entry.range.start + i];
            let points = local.map(transform);
            triangles.push(
                WorldTriangle::from_vertices(
                    points,
                    self.contact_material,
                    source.tag,
                    source.triangle.feature.flags,
                    source.triangle.feature.edge_cosines,
                    0.,
                )
                .ok_or("Rebaked prop collision triangle is invalid")?,
            );
        }
        self.world
            .replace_triangles(entry.range.clone(), &triangles)
            .map_err(str::to_owned)
    }

    /// A layer with no instances yet (a map without placed props, which can
    /// still get props created mid-game).
    pub fn empty(material: RetailContactMaterial) -> Result<Self, String> {
        let metadata = QueryMetadata {
            packed_surfaces: vec![],
            meshes: vec![],
            static_edges: vec![],
            island_flags: 0,
        };
        Ok(Self {
            world: BoardWorld::with_query_metadata(vec![], metadata).map_err(str::to_owned)?,
            instances: Vec::new(),
            contact_material: material,
            retired: Vec::new(),
        })
    }

    /// Add one instance mid-game (a released hand prop, a mod's prop): `local`
    /// are template-space triangles with scale folded in, baked at `basis` /
    /// `translation` like [`Self::rebake`]; `surface` is the packed surface code
    /// (tag) of every triangle. Edge features come from the instance's own
    /// triangles (load-time instances also pair edges with touching neighbours).
    /// A retired slot with the same triangle count is reused, so repeated
    /// spawns of one template do not grow the world. Returns the instance index.
    pub fn add_instance(
        &mut self,
        id: u32,
        local: Vec<[Vector3; 3]>,
        surface: u32,
        basis: [[f32; 3]; 3],
        translation: Vector3,
    ) -> Result<usize, String> {
        let local: Vec<[Vector3; 3]> = local
            .into_iter()
            .filter(|t| {
                let [a, b, c] = t.map(|p| Vec3::new(p.x, p.y, p.z));
                (b - a).cross(c - a).length_squared() > 0.
            })
            .collect();
        if local.is_empty() {
            return Err("Prop instance has no valid triangles".into());
        }
        let transform = |p: Vector3| {
            [
                p.x * basis[0][0] + p.y * basis[1][0] + p.z * basis[2][0] + translation.x,
                p.x * basis[0][1] + p.y * basis[1][1] + p.z * basis[2][1] + translation.y,
                p.x * basis[0][2] + p.y * basis[1][2] + p.z * basis[2][2] + translation.z,
            ]
        };
        let points: Vec<[[f32; 3]; 3]> = local.iter().map(|t| t.map(transform)).collect();
        let (_, _, flags, cosines) = welded_edge_features(&points, true)?;
        let mut triangles = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            triangles.push(
                WorldTriangle::from_vertices(
                    p.map(|p| Vector3::new(p[0], p[1], p[2])),
                    self.contact_material,
                    surface,
                    flags[i],
                    cosines[i],
                    0.,
                )
                .ok_or("Prop instance triangle is invalid")?,
            );
        }
        let reuse = self
            .retired
            .iter()
            .position(|&slot| self.instances[slot].range.len() == triangles.len());
        let index = match reuse {
            Some(position) => {
                let slot = self.retired.remove(position);
                self.world
                    .replace_triangles(self.instances[slot].range.clone(), &triangles)
                    .map_err(str::to_owned)?;
                slot
            }
            None => {
                let range = self
                    .world
                    .append_triangles(&triangles, &vec![surface as u16; triangles.len()])
                    .map_err(str::to_owned)?;
                self.instances.push(PropCollisionInstance { id, object: usize::MAX, range, local: Vec::new() });
                self.instances.len() - 1
            }
        };
        let entry = &mut self.instances[index];
        entry.id = id;
        entry.local = local;
        Ok(index)
    }

    /// Retire an instance added with [`Self::add_instance`] (the released prop is
    /// removed): its triangles are parked far below the world and the slot is
    /// kept for the next instance of the same size.
    pub fn retire_instance(&mut self, instance: usize, park: Vector3) -> Result<(), String> {
        if instance >= self.instances.len() || self.retired.contains(&instance) {
            return Err("Unknown or already retired prop instance".into());
        }
        if self.instances[instance].object != usize::MAX {
            return Err("Map prop instances are not retired".into());
        }
        self.rebake(instance, [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], park)?;
        self.retired.push(instance);
        Ok(())
    }
}

/// Static collision for spawned DMO prop instances. No authored DMO collision
/// mesh is recovered, so each instance reuses its template's render triangles,
/// baked into world space with the instance transform at load. Reflections
/// flip winding to keep outward normals; degenerate render triangles are
/// skipped rather than rejecting the whole layer.
pub(crate) fn build_prop_layer(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    material: RetailContactMaterial,
) -> Result<Option<PropCollisionLayer>, String> {
    let mut collision = Vec::new();
    let mut instances = Vec::new();
    for (object_index, object) in objects.iter().enumerate() {
        let t = &object.transform;
        // Row-vector affine (v @ basis + translation), as in spawn_instances.
        let transform = |p: [f32; 3]| -> [f32; 3] {
            [
                p[0] * t[0] + p[1] * t[3] + p[2] * t[6] + t[9],
                p[0] * t[1] + p[1] * t[4] + p[2] * t[7] + t[10],
                p[0] * t[2] + p[1] * t[5] + p[2] * t[8] + t[11],
            ]
        };
        let determinant = t[0] * (t[4] * t[8] - t[5] * t[7])
            - t[1] * (t[3] * t[8] - t[5] * t[6])
            + t[2] * (t[3] * t[7] - t[4] * t[6]);
        // Row-vector rows are the world images of the local axes. Their lengths
        // are the per-axis scale; rebake applies rotation only, so local source
        // triangles carry the scale and stay exact for diagonal-scale placements.
        let scale = [
            (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt(),
            (t[3] * t[3] + t[4] * t[4] + t[5] * t[5]).sqrt(),
            (t[6] * t[6] + t[7] * t[7] + t[8] * t[8]).sqrt(),
        ];
        let range =
            object.first_index as usize..(object.first_index + object.index_count) as usize;
        let start = collision.len();
        let mut local = Vec::new();
        for tri in map.geometry.indices[range].chunks_exact(3) {
            let mut points =
                [tri[0], tri[1], tri[2]].map(|i| map.geometry.vertices[i as usize].position);
            if determinant < 0. {
                points.swap(1, 2);
            }
            let [a, b, c] = points.map(Vec3::from_array);
            let cross = (b - a).cross(c - a);
            if cross.length_squared() <= 0. {
                continue;
            }
            local.push(points.map(|p| {
                Vector3::new(p[0] * scale[0], p[1] * scale[1], p[2] * scale[2])
            }));
            let source = &map.materials
                [map.geometry.vertices[tri[0] as usize].material as usize - 1];
            // Wheels read the packed surface nibble from the triangle tag; use
            // the same EncodeRwSurfaceId mapping as static map collision.
            let surface = source.audio | (source.physics << 7) | (source.pattern << 12);
            collision.push(skate_data::skate_map::Collision {
                points: points.map(transform),
                surface,
                material: map.geometry.vertices[tri[0] as usize].material,
                native_edges: None,
            });
        }
        if collision.len() > start {
            instances.push(PropCollisionInstance {
                id: object.id,
                object: object_index,
                range: start..collision.len(),
                local,
            });
        }
    }
    if collision.is_empty() {
        return Ok(None);
    }
    Ok(Some(PropCollisionLayer {
        world: portable_world(&collision, &map.materials, material)?,
        instances,
        contact_material: material,
        retired: Vec::new(),
    }))
}

/// Retail per-type DMO data for the district's props: setup writes the map
/// from template id to the type's vault record (`types` in
/// `private/native-props/<map>.json`, from the template's EB000D +120); the
/// values come from the installation's stock vault
/// (`livingworld_dynamicobject_characteristics`,
/// [`crate::physics::prop_dynamics::dmo_type_blocks`]). `None` when the
/// sidecar has no type map (setup older than the type data): the props keep
/// their authored material (NOT RETAIL YET there; re-run setup group maps).
/// The authored grab splines per DMO template id (`native-props/<map>.json` `grab_splines`, doc 26i "Move Object
/// step 1"); `None` when the export has none.
pub(crate) fn load_dmo_grab_splines(asset_root: &std::path::Path, map_name: &str) -> Option<std::collections::BTreeMap<String, Vec<crate::living_world::vehicles::CarGrabSpline>>> {
    let path = asset_root.join("private").join("native-props").join(format!("{map_name}.json"));
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let map = sidecar.get("grab_splines")?.as_object()?;
    let splines: std::collections::BTreeMap<_, _> = map.iter().map(|(k, v)| (k.clone(), crate::living_world::vehicles::parse_grab_splines(v))).filter(|(_, v): &(String, Vec<_>)| !v.is_empty()).collect();
    (!splines.is_empty()).then_some(splines)
}

pub(crate) fn load_dmo_types(
    asset_root: &std::path::Path,
    map_name: &str,
) -> Option<std::collections::BTreeMap<String, crate::physics::prop_dynamics::DmoType>> {
    use crate::physics::prop_dynamics::{dmo_type_blocks, DmoType, DMO_TYPE_CLASS};
    let path = asset_root.join("private").join("native-props").join(format!("{map_name}.json"));
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let map = sidecar.get("types")?.as_object()?;
    if map.is_empty() {
        return None;
    }
    let collections = match skate_data::collections::Collections::load(asset_root) {
        Ok(c) => c,
        Err(error) => {
            warn!("SKATE_PROP_TYPES: {map_name}: {error}");
            return None;
        }
    };
    let names: std::collections::HashMap<String, &str> = collections
        .entries()
        .iter()
        .filter(|c| c.class_name == DMO_TYPE_CLASS)
        .map(|c| (skate_data::attrib_hash::numeric_name(&c.key), c.key.as_str()))
        .collect();
    let mut types = std::collections::BTreeMap::new();
    for (template, record) in map {
        let Some(record) = record.as_str() else { continue };
        let id = skate_data::attrib_hash::numeric_name(record);
        let key = names.get(&id).map_or(record, |name| *name);
        match dmo_type_blocks(&collections, key) {
            Ok(blocks) => {
                let priority = crate::physics::prop_dynamics::dmo_type_priority(&collections, key).unwrap_or_else(|error| {
                    warn!("SKATE_PROP_TYPES: {map_name} template {template}: priority: {error}");
                    Some(100)
                });
                types.insert(template.clone(), DmoType { key: key.to_owned(), blocks, priority });
            }
            Err(error) => warn!("SKATE_PROP_TYPES: {map_name} template {template}: {error}"),
        }
    }
    Some(types)
}

/// Load the district's prop package and build its collision layer plus the
/// dynamic bodies for every instance. The package is a presentation
/// supplement: missing or invalid files leave props uncollidable rather than
/// failing the map, matching the render path.
pub(crate) fn load_prop_layer(
    asset_root: &std::path::Path,
    map_name: &str,
    material: RetailContactMaterial,
    simulation: skate_core::physics::rigid_body::RetailSimulationStep,
) -> Option<(PropCollisionLayer, crate::physics::prop_dynamics::PropDynamics)> {
    let path = asset_root
        .join("private")
        .join("native-props")
        .join(format!("{map_name}.skate"));
    if !path.is_file() {
        return None;
    }
    let loaded = std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data));
    let map = match loaded {
        Ok(map) => map,
        Err(error) => {
            warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
            return None;
        }
    };
    let mut objects = Vec::new();
    for extension in map.extensions.iter().filter(|e| e.tag == *b"MOBJ") {
        match skate_data::skate_map::parse_static_objects(&map, extension) {
            Ok(parsed) => objects.extend(parsed),
            Err(error) => {
                warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
                return None;
            }
        }
    }
    match build_prop_layer(&map, &objects, material) {
        Ok(Some(layer)) => {
            info!(
                "SKATE_PROP_COLLISION: {map_name} instances={} triangles={}",
                objects.len(),
                layer.world().triangles().len()
            );
            let mut dynamics = crate::physics::prop_dynamics::PropDynamics::new(
                &objects,
                layer.instances(),
                simulation,
            );
            if let Some(types) = load_dmo_types(asset_root, map_name) {
                let typed = dynamics.set_type_data(&types);
                info!("SKATE_PROP_TYPES: {map_name} types={} props_with_type_data={typed}/{}", types.len(), objects.len());
            }
            if let Some(splines) = load_dmo_grab_splines(asset_root, map_name) {
                let with = dynamics.set_grab_splines(&splines);
                info!("SKATE_PROP_GRAB: {map_name} templates={} props_with_grab_splines={with}/{}", splines.len(), objects.len());
            }
            Some((layer, dynamics))
        }
        Ok(None) => None,
        Err(error) => {
            warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every prop type in a set-up map's type map resolves to retail type
    /// data from the stock vault, and the values are the retail record's
    /// (the default record: restitution 0.5, free pair 0.8 / 0.6).
    /// `SKATE3_ASSET_ROOT` = set-up assets (setup group maps run with the
    /// type map), `SKATE3_PROP_MAP` = map name (default DownTown).
    #[test]
    #[ignore = "Requires private installed assets"]
    fn installed_map_props_resolve_retail_type_data() {
        let root = std::path::PathBuf::from(std::env::var("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let map = std::env::var("SKATE3_PROP_MAP").unwrap_or_else(|_| "DownTown".into());
        let sidecar: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("private/native-props").join(format!("{map}.json"))).unwrap(),
        )
        .unwrap();
        let expected = sidecar["types"].as_object().expect("type map (re-run setup group maps)").len();
        let types = load_dmo_types(&root, &map).expect("type data");
        assert!(expected > 0);
        assert_eq!(types.len(), expected, "every template's record resolved");
        for (template, t) in &types {
            let b = t.blocks;
            for v in [b.restitution.unwrap(), b.free.unwrap()[0], b.free.unwrap()[1]] {
                assert!((0.0..=1.0).contains(&v), "{template} {} {v}", t.key);
            }
            assert_eq!(b.upright_pair, b.record_272);
            for v in [b.linear_drag.unwrap(), b.angular_drag.unwrap()] {
                assert!((0.0..=1.0).contains(&v), "{template} {} drag {v}", t.key);
            }
            // Body data of 82C4E568: mass +304, caps +292 / +296, inertia box +16 / +32.
            assert!(b.mass.unwrap() > 0.0, "{template} {} mass", t.key);
            assert!(b.maximum_linear_velocity.unwrap() > 0.0 && b.maximum_angular_velocity.unwrap() > 0.0);
            assert!(b.inertia_scale.is_some() && b.inertia_offset.is_some());
        }
        let collections = skate_data::collections::Collections::load(&root).unwrap();
        let default = crate::physics::prop_dynamics::dmo_type_blocks(&collections, "default").unwrap();
        assert_eq!((default.restitution, default.free, default.upright_pair), (Some(0.5), Some([0.8, 0.6]), Some(false)));
        assert_eq!((default.linear_drag, default.angular_drag), (Some(0.0), Some(0.0)));
        assert_eq!((default.mass, default.maximum_linear_velocity, default.maximum_angular_velocity), (Some(100.0), Some(100.0), Some(100.0)));
        assert_eq!((default.inertia_scale, default.inertia_offset), (Some([1.2, 1.2, 1.2]), Some([0.0; 3])));
        for (template, t) in &types {
            let b = t.blocks;
            eprintln!("{template} {} mass={:?} caps={:?}/{:?} box={:?}+{:?}", t.key, b.mass, b.maximum_linear_velocity, b.maximum_angular_velocity, b.inertia_scale, b.inertia_offset);
        }
        let mut keys: Vec<_> = types.values().map(|t| t.key.as_str()).collect();
        keys.dedup();
        eprintln!("{map}: {} templates, types {:?}", types.len(), keys);
    }

    /// Measures the static draw budget on a real installed map. This is the
    /// check that the whole architecture exists to pass, so it reports the class
    /// mix rather than only the total: if the budget is ever missed, the mix says
    /// whether to subdivide less or to stop splitting on a material property.
    ///
    /// `SKATE_BUDGET_MAP` names the `.skate` file; `SKATE_TRANSITION_TEST_ASSETS`
    /// gives the asset root.
    #[test]
    #[ignore = "Requires private installed assets; CPU only, no GPU or window"]
    fn static_draw_budget_holds_on_an_installed_map() {
        let root = std::path::PathBuf::from(
            std::env::var("SKATE_TRANSITION_TEST_ASSETS")
                .expect("SKATE_TRANSITION_TEST_ASSETS"),
        );
        let path = std::path::PathBuf::from(
            std::env::var("SKATE_BUDGET_MAP").expect("SKATE_BUDGET_MAP"),
        );
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<crate::retail_render::WorldMaterial>>();
        world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
        let mut scene = crate::map_render::PreparedScene::new(&world);
        let started = std::time::Instant::now();
        scene.prepare(Some(&map), &root);
        let elapsed = started.elapsed();
        let stats = scene.stats;
        eprintln!(
            "SKATE_BUDGET map={} draws={} triangles={} slabs={} materials={} prepare_ms={}",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            stats.draws,
            stats.triangles,
            stats.slabs,
            map.materials.len(),
            elapsed.as_millis()
        );
        assert!(
            stats.draws < 300,
            "static draw budget is under 300; got {} draws",
            stats.draws
        );
        assert!(stats.triangles > 0, "map produced no geometry");
    }

    fn triangle(centroid: Vec3) -> Triangle {
        Triangle { indices: [0, 1, 2], centroid, slab: 0, class: RenderClass::Opaque }
    }

    #[test]
    fn partition_respects_the_leaf_budget() {
        let triangles: Vec<Triangle> = (0..LEAF_TRIANGLE_BUDGET * 3 + 7)
            .map(|i| triangle(Vec3::new(i as f32, 0., 0.)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let leaves = partition(&mut members, &triangles);
        assert!(leaves.len() >= 4, "expected several leaves, got {}", leaves.len());
        for leaf in &leaves {
            assert!(leaf.len() <= LEAF_TRIANGLE_BUDGET, "leaf of {} triangles", leaf.len());
        }
    }

    #[test]
    fn partition_preserves_every_triangle_exactly_once() {
        let triangles: Vec<Triangle> = (0..5_000)
            .map(|i| triangle(Vec3::new((i % 71) as f32, (i % 13) as f32, i as f32)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let mut seen: Vec<usize> = partition(&mut members, &triangles)
            .into_iter()
            .flatten()
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..triangles.len()).collect::<Vec<_>>());
    }

    #[test]
    fn a_single_leaf_is_not_split() {
        let triangles: Vec<Triangle> = (0..64).map(|i| triangle(Vec3::splat(i as f32))).collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        assert_eq!(partition(&mut members, &triangles).len(), 1);
    }

    fn material() -> RetailContactMaterial {
        RetailContactMaterial {
            static_friction: 0.,
            dynamic_friction: 0.,
            restitution: 1.,
        }
    }

    /// A 4x1x1 slab template (x ±2, y 0..1, z ±0.5) with two placed instances:
    /// one translated, one rotated 90° about Y and translated.
    fn prop_fixture() -> (SkateMap, Vec<skate_data::skate_map::StaticObject>) {
        let corners = [
            [-2., 0., -0.5], [2., 0., -0.5], [2., 0., 0.5], [-2., 0., 0.5],
            [-2., 1., -0.5], [2., 1., -0.5], [2., 1., 0.5], [-2., 1., 0.5],
        ];
        let faces = [
            [4, 7, 6], [4, 6, 5], // +Y top
            [0, 1, 2], [0, 2, 3], // -Y bottom
            [1, 5, 6], [1, 6, 2], // +X
            [0, 7, 4], [0, 3, 7], // -X
            [3, 2, 6], [3, 6, 7], // +Z
            [0, 5, 1], [0, 4, 5], // -Z
        ];
        let vertex = |position| skate_data::skate_map::Vertex {
            position,
            normal: [0., 1., 0.],
            uv: [0.; 2],
            lightmap_uv: [0.; 2],
            material: 1,
            decal_uv: None,
            tangent_frame: None,
        };
        let map = SkateMap {
            version: 14,
            name: "props".into(),
            spawn: [0.; 3],
            heading: 0.,
            environment: vec![0.; 45],
            materials: vec![skate_data::skate_map::Material {
                name: "prop".into(),
                flags: 0,
                friction: 0.5,
                restitution: 0.1,
                color: [1.; 3],
                roughness: 0.5,
                emissive: 0.,
                textures: [0; 5],
                indirect_strength: 0.,
                alpha_mode: 0,
                alpha_cutoff: 0.5,
                audio: 3,
                physics: 1,
                pattern: 0,
                depth_layer: None,
                retail_definition: None,
            }],
            textures: vec![],
            geometry: skate_data::skate_map::Geometry {
                vertices: corners.into_iter().map(vertex).collect(),
                indices: faces.into_iter().flatten().collect(),
                collision: vec![],
            },
            rails: vec![],
            doors: vec![],
            lights: vec![],
            routes: vec![],
            extensions: vec![],
        };
        let object = |id, transform| skate_data::skate_map::StaticObject {
            id,
            name: format!("template/prop{id}"),
            transform,
            first_index: 0,
            index_count: 36,
            first_collision: 0,
            collision_count: 0,
            rails: vec![],
            physics: Default::default(),
        };
        let objects = vec![
            object(7, [1., 0., 0., 0., 1., 0., 0., 0., 1., 10., 5., 0.]),
            // 90° about Y (row-vector): x' = -z, z' = x; then z -= 10.
            object(8, [0., 0., 1., 0., 1., 0., -1., 0., 0., 0., 0., -10.]),
        ];
        (map, objects)
    }

    #[test]
    fn prop_instances_collide_as_placed_static_triangles() {
        let (map, objects) = prop_fixture();
        let layer = build_prop_layer(&map, &objects, material()).unwrap().unwrap();
        let mut world = layer.world;
        assert_eq!(world.triangles().len(), 24);
        // Translated instance: top face at y=6 between x 8..12.
        let hit = world
            .query_thin_line(Vector3::new(10., 10., 0.), Vector3::new(10., 0., 0.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - 6.).abs() < 1e-4);
        assert!(hit.geometry.normal.y > 0.99);
        // The packed surface tag follows the EncodeRwSurfaceId mapping.
        assert_eq!(hit.tag, 3 | (1 << 7));
        // Rotated instance: slab now spans z -12..-8, x ±0.5.
        let hit = world
            .query_thin_line(Vector3::new(0., 5., -10.), Vector3::new(0., -1., -10.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - 1.).abs() < 1e-4);
        // Outside the rotated footprint: unrotated, this line would hit.
        assert!(world
            .query_thin_line(Vector3::new(1.5, 5., -10.), Vector3::new(1.5, -1., -10.))
            .unwrap()
            .is_none());
        // A wheel sphere resting on the translated instance reports a contact.
        let (query, retention) = crate::physics::ground::query_settings();
        let volumes = [skate_core::physics::board_world::BoardWorldVolume {
            collision_group: 4,
            body: skate_core::physics::board_step::CollisionBody::Board(
                skate_core::physics::board::BodyId::Deck,
            ),
            primitive: skate_core::physics::world_contact::ContactPrimitive::Sphere(
                skate_core::physics::collision::Sphere {
                    center: Vector3::new(10., 6.05, 0.),
                    radius: 0.1,
                },
            ),
            motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::ZERO, ..Default::default() },
            material: material(),
        }];
        let contacts = world.query_primitives(&volumes, query, retention);
        assert!(!contacts.is_empty());
        assert!(contacts.iter().all(|c| c.contact.normal.y > 0.9));
    }

    #[test]
    fn prop_instances_spawn_one_placed_root_per_record_sharing_template_meshes() {
        use crate::map_render::{MapEntity, StagedAssets};
        let (map, objects) = prop_fixture();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<WorldMaterial>>();
        world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
        let mut meshes = StagedAssets::<Mesh>::new(&world);
        let mut images = StagedAssets::<Image>::new(&world);
        let mut materials = StagedAssets::<WorldMaterial>::new(&world);
        let mut buffers = StagedAssets::<bevy::render::storage::ShaderStorageBuffer>::new(&world);
        let mut commands = SceneCommands::default();
        let spawned = spawn_instances(
            &map,
            &objects,
            &crate::retail_render::MaterialTuning::default(),
            &crate::retail_sky::SkyEnvironment::default(),
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut buffers,
        );
        assert_eq!(spawned, 2);
        meshes.publish(&mut world);
        images.publish(&mut world);
        materials.publish(&mut world);
        buffers.publish(&mut world);
        commands.apply(&mut world);
        // Both records place the same range: one template mesh, built once.
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
        let mut roots: Vec<(u32, Transform, Vec<Entity>)> = world
            .query::<(&PropInstance, &Transform, &Children)>()
            .iter(&world)
            .map(|(prop, transform, children)| (prop.id, *transform, children.to_vec()))
            .collect();
        roots.sort_by_key(|(id, ..)| *id);
        assert_eq!(roots.iter().map(|(id, ..)| *id).collect::<Vec<_>>(), [7, 8]);
        // Only the roots carry MapEntity; children go with their parent.
        assert_eq!(world.query_filtered::<Entity, With<MapEntity>>().iter(&world).count(), 2);
        assert!((roots[0].1.translation - Vec3::new(10., 5., 0.)).length() < 1e-5);
        // Row-vector basis row 0 is (0, 0, 1): template +X maps to world +Z.
        let tip = roots[1].1.transform_point(Vec3::X);
        assert!((tip - Vec3::new(0., 0., -9.)).length() < 1e-5, "{tip}");
        let mesh = |entity: Entity| world.get::<Mesh3d>(entity).unwrap().0.id();
        assert_eq!(roots[0].2.len(), 1);
        assert_eq!(mesh(roots[0].2[0]), mesh(roots[1].2[0]));
    }
}
