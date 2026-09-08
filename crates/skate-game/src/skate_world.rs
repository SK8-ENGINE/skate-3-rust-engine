//! Adapter for authored .skate world geometry. Does not replace controllers.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
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

pub(crate) fn validate_runtime(map: &SkateMap) -> Result<(), String> {
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
    if map.materials.iter().any(|m| m.retail_definition.is_some()) {
        eprintln!(
            "SKATE_RENDER: retail world material adapter enabled; unsupported families retain portable PBR rendering."
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
        "SKATE LIMITATION: native frame-lighting/day-night controller is not connected; using package lighting and retail district sky where available."
    );
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
                triangle_range: range,
                local_to_world: RetailAffineTransform::IDENTITY,
                world_to_local: RetailAffineTransform::IDENTITY,
                local_bounds: bounds,
                matching_group: i32::from(group),
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
    // Match the reference RW mesh compiler's 1 mm vertex welding and reversed
    // edge pairing. Triangle diagonals are adjacency, never authored ledges.
    let mut welded = HashMap::<[i64; 3], usize>::new();
    let mut positions = Vec::<Vec3>::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    for tri in &map.geometry.collision {
        let ids = tri.points.map(|p| {
            let inverse = 1.0 / f64::from(0.001_f32);
            let key = p.map(|v| (f64::from(v) * inverse).round() as i64);
            *welded.entry(key).or_insert_with(|| {
                let id = positions.len();
                positions.push(Vec3::from_array(p));
                id
            })
        });
        let [a, b, c] = tri.points.map(Vec3::from_array);
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
    if map
        .geometry
        .collision
        .iter()
        .any(|t| t.native_edges.is_none())
    {
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
    let mut triangles = Vec::with_capacity(vertices.len());
    let mut packed_surfaces = Vec::with_capacity(vertices.len());
    for (i, source) in map.geometry.collision.iter().enumerate() {
        if let Some(edges) = source.native_edges {
            (flags[i], cosines[i]) = decode_native_edges(edges)?;
        }
        let m = &map.materials[source.material as usize - 1];
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
    let bounds = Bounds::from_points(triangles.iter().flat_map(|t| t.triangle.vertices))
        .ok_or("SKATE collision bounds empty")?;
    let metadata = QueryMetadata {
        packed_surfaces,
        meshes: vec![QueryMesh {
            triangle_range: 0..triangles.len(),
            local_to_world: RetailAffineTransform::IDENTITY,
            world_to_local: RetailAffineTransform::IDENTITY,
            local_bounds: bounds,
            matching_group: -1,
            pool: QueryPool::Ground,
        }],
        static_edges: vec![],
        island_flags: 0,
    };
    BoardWorld::with_query_metadata(triangles, metadata).map_err(str::to_owned)
}

/// Reuse identical render resources without changing authored triangles.
fn render_texture_ids(textures: &[skate_data::skate_map::Texture]) -> Vec<u32> {
    use std::hash::{Hash, Hasher};
    let mut buckets: HashMap<(u32, u32, u32, u64), Vec<usize>> = HashMap::new();
    let mut ids = vec![0];
    for (i, t) in textures.iter().enumerate() {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        t.rgba.hash(&mut hash);
        let bucket = buckets
            .entry((t.width, t.height, t.color_space, hash.finish()))
            .or_default();
        let same = bucket.iter().copied().find(|&j| textures[j].rgba == t.rgba);
        let canonical = same.unwrap_or_else(|| {
            bucket.push(i);
            i
        });
        ids.push(canonical as u32 + 1);
    }
    ids
}

fn render_material_ids(
    materials: &[skate_data::skate_map::Material],
    texture_ids: &[u32],
) -> Vec<usize> {
    material_ids(materials, texture_ids, true)
}

fn material_ids(
    materials: &[skate_data::skate_map::Material],
    texture_ids: &[u32],
    include_lightmap: bool,
) -> Vec<usize> {
    let mut unique = HashMap::new();
    materials
        .iter()
        .enumerate()
        .map(|(i, m)| {
            // Only fields consumed by this renderer belong in the identity. Source
            // names and collision surface metadata do not change a PBR material.
            let textures = m.textures.map(|id| texture_ids[id as usize]);
            let key = [
                textures[0],
                if include_lightmap { textures[1] } else { 0 },
                textures[2],
                textures[3],
                textures[4],
                m.color[0].to_bits(),
                m.color[1].to_bits(),
                m.color[2].to_bits(),
                m.roughness.to_bits(),
                m.emissive.to_bits(),
                m.indirect_strength.to_bits(),
                m.alpha_mode,
                m.alpha_cutoff.to_bits(),
            ];
            let retail = m.retail_definition.as_deref().and_then(crate::retail_render::Definition::parse)
                .filter(|d| d.supported());
            *unique.entry((key, retail)).or_insert(i)
        })
        .collect()
}

fn render_groups(
    geometry: &skate_data::skate_map::Geometry,
    material_ids: &[usize],
) -> Vec<(usize, Vec<u32>)> {
    let mut lookup = HashMap::new();
    let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
    for tri in geometry.indices.chunks_exact(3) {
        let material = material_ids[geometry.vertices[tri[0] as usize].material as usize - 1];
        let group = *lookup.entry(material).or_insert_with(|| {
            let index = groups.len();
            groups.push((material, Vec::new()));
            index
        });
        groups[group].1.extend_from_slice(tri);
    }
    groups
}

pub(crate) fn spawn(
    map: &SkateMap,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    retail_materials: &mut Assets<crate::retail_render::RetailWorldMaterial>,
    images: &mut Assets<Image>,
) {
    // Texture roles have different transfer functions even when sharing a record.
    let texture_ids = render_texture_ids(&map.textures);
    let mut cache = HashMap::<(u32, u8), Handle<Image>>::new();
    let mut texture = |id: u32, role: u8| -> Option<Handle<Image>> {
        let id = texture_ids[id as usize];
        if id == 0 {
            return None;
        }
        if role == 5 && map.textures[id as usize - 1].height != map.textures[id as usize - 1].width * 6 {
            // Older .skate exports contain only face zero. Never treat it as a cube.
            return None;
        }
        Some(
            cache
                .entry((id, role))
                .or_insert_with(|| {
                    let source = &map.textures[id as usize - 1];
                    let (format, bytes) = if role == 1 {
                        let mut bytes = Vec::with_capacity(source.rgba.len() * 2);
                        for pixel in source.rgba.chunks_exact(4) {
                            for (i, &byte) in pixel.iter().enumerate() {
                                let v = f32::from(byte) / 255.;
                                let linear = if i == 3 { 1. } else { v * v * 4. };
                                bytes.extend_from_slice(&half::f16::from_f32(linear).to_le_bytes());
                            }
                        }
                        (TextureFormat::Rgba16Float, bytes)
                    } else {
                        let srgb =
                            role == 0 && source.color_space == 1 && !(2..=3).contains(&map.version);
                        (
                            if srgb {
                                TextureFormat::Rgba8UnormSrgb
                            } else {
                                TextureFormat::Rgba8Unorm
                            },
                            source.rgba.clone(),
                        )
                    };
                    let cube = role == 5;
                    let height = if cube { source.height / 6 } else { source.height };
                    let layers = if cube { 6 } else { 1 };
                    let mut image = Image::new(
                        Extent3d {
                            width: source.width,
                            height,
                            depth_or_array_layers: layers,
                        },
                        TextureDimension::D2,
                        bytes,
                        format,
                        RenderAssetUsages::RENDER_WORLD,
                    );
                    if role == 3 || cube {
                        let (bytes, levels) = crate::retail_render::mip_chain(&source.rgba, source.width, height, layers);
                        image.data = Some(bytes);
                        image.texture_descriptor.mip_level_count = levels;
                    }
                    if cube {
                        image.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor {
                            dimension: Some(bevy::render::render_resource::TextureViewDimension::Cube), ..default()
                        });
                    }
                    let mut sampler = bevy::image::ImageSamplerDescriptor::linear();
                    if role != 1 && role != 4 && !cube {
                        sampler.address_mode_u = bevy::image::ImageAddressMode::Repeat;
                        sampler.address_mode_v = bevy::image::ImageAddressMode::Repeat;
                    }
                    image.sampler = ImageSampler::Descriptor(sampler);
                    images.add(image)
                })
                .clone(),
        )
    };
    let material_ids = render_material_ids(&map.materials, &texture_ids);
    // Lightmaps belong to mesh entities, not StandardMaterial. Distinct baked
    // lighting still needs separate geometry batches but can share a PBR material.
    let pbr_ids = self::material_ids(&map.materials, &texture_ids, false);
    let groups = render_groups(&map.geometry, &material_ids);
    eprintln!(
        "SKATE_RENDER_BATCHES count={} triangles={}",
        groups.len(),
        map.geometry.indices.len() / 3
    );
    let mut material_handles: Vec<Option<Handle<StandardMaterial>>> =
        vec![None; map.materials.len()];
    for (material_index, indices) in groups {
        let m = &map.materials[material_index];
        // Reindex each batch, preserving authored normals and both UV sets.
        let mut remap = HashMap::new();
        let mut vertices = Vec::new();
        let local: Vec<u32> = indices
            .into_iter()
            .map(|index| {
                *remap.entry(index).or_insert_with(|| {
                    let id = vertices.len() as u32;
                    vertices.push(&map.geometry.vertices[index as usize]);
                    id
                })
            })
            .collect();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vertices.iter().map(|v| v.normal).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_1,
            vertices.iter().map(|v| v.lightmap_uv).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_COLOR,
            vertices.iter().map(|v| { let uv = v.decal_uv.unwrap_or(v.uv); [uv[0], uv[1], 0., 1.] }).collect::<Vec<_>>(),
        )
        .with_inserted_indices(bevy::mesh::Indices::U32(local));
        if vertices.iter().all(|v| v.tangent_frame.is_some()) {
            let tangents: Vec<[f32; 4]> = vertices
                .iter()
                .map(|v| {
                    let frame = v
                        .tangent_frame
                        .unwrap()
                        .map(|b| (b as i8 as f32 / 127.).max(-1.));
                    let binormal = Vec3::new(frame[0], frame[1], frame[2]);
                    let tangent = binormal.cross(Vec3::from_array(v.normal)) * frame[3];
                    [tangent.x, tangent.y, tangent.z, frame[3]]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
        } else if m.textures[2] != 0 {
            if let Err(error) = mesh.generate_tangents() {
                warn!("SKATE material {} tangent generation: {error}", m.name);
            }
        }
        if let Some(definition) = m.retail_definition.as_deref()
            .and_then(crate::retail_render::Definition::parse).filter(|d| d.supported()) {
            let material = definition.build(m, &mut texture);
            let material = retail_materials.add(material);
            commands.spawn((Name::new(m.name.clone()), Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::default()));
            continue;
        }
        // Vertex colours above carry retail decal coordinates, never PBR tint.
        mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
        let material = material_handles[pbr_ids[material_index]]
            .get_or_insert_with(|| {
                let orm = texture(m.textures[3], 2);
                materials.add(StandardMaterial {
                    base_color: Color::linear_rgb(m.color[0], m.color[1], m.color[2]),
                    base_color_texture: texture(m.textures[0], 0),
                    normal_map_texture: texture(m.textures[2], 2),
                    metallic_roughness_texture: orm.clone(),
                    occlusion_texture: orm,
                    metallic: if m.textures[3] != 0 { 1. } else { 0. },
                    perceptual_roughness: m.roughness,
                    emissive: LinearRgba::rgb(
                        m.color[0] * m.emissive,
                        m.color[1] * m.emissive,
                        m.color[2] * m.emissive,
                    ),
                    emissive_texture: texture(m.textures[4], 0),
                    alpha_mode: match m.alpha_mode {
                        1 => AlphaMode::Mask(m.alpha_cutoff),
                        2 => AlphaMode::Blend,
                        _ => AlphaMode::Opaque,
                    },
                    lightmap_exposure: m.indirect_strength,
                    ..default()
                })
            })
            .clone();
        let mut entity = commands.spawn((
            Name::new(m.name.clone()),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::default(),
        ));
        if let Some(image) = texture(m.textures[1], 1) {
            entity.insert(bevy::pbr::Lightmap {
                image,
                uv_rect: Rect::new(0., 0., 1., 1.),
                bicubic_sampling: false,
            });
        }
    }
    for light in &map.lights {
        let color = Color::linear_rgb(light.color[0], light.color[1], light.color[2]);
        let transform = Transform::from_translation(Vec3::from_array(light.position));
        match light.kind {
            0 => {
                commands.spawn((
                    Name::new(light.name.clone()),
                    PointLight {
                        color,
                        intensity: light.intensity,
                        range: light.range,
                        radius: light.radius,
                        ..default()
                    },
                    transform,
                ));
            }
            1 => {
                commands.spawn((
                    Name::new(light.name.clone()),
                    SpotLight {
                        color,
                        intensity: light.intensity,
                        range: light.range,
                        radius: light.radius,
                        inner_angle: light.inner_cos.acos(),
                        outer_angle: light.outer_cos.acos(),
                        ..default()
                    },
                    transform.looking_to(Vec3::from_array(light.direction), Vec3::Y),
                ));
            }
            _ => {}
        }
    }
    commands.insert_resource(ClearColor(Color::linear_rgb(
        map.environment[3],
        map.environment[4],
        map.environment[5],
    )));
    info!(
        "SKATE_WORLD_READY name={:?} render_triangles={} collision_triangles={}",
        map.name,
        map.geometry.indices.len() / 3,
        map.geometry.collision.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn demo() -> SkateMap {
        SkateMap::parse(include_bytes!("../../../maps/format-demo.skate")).unwrap()
    }
    #[test]
    fn render_batches_preserve_complete_triangles_and_materials() {
        let map = demo();
        let material_ids = render_material_ids(&map.materials, &render_texture_ids(&map.textures));
        let chunks = render_groups(&map.geometry, &material_ids);
        assert_eq!(chunks.len(), 1);
        let mut original: Vec<_> = map
            .geometry
            .indices
            .chunks_exact(3)
            .map(|t| t.to_vec())
            .collect();
        let mut partitioned = Vec::new();
        for (material, indices) in chunks {
            for tri in indices.chunks_exact(3) {
                assert_eq!(
                    material + 1,
                    map.geometry.vertices[tri[0] as usize].material as usize
                );
                partitioned.push(tri.to_vec());
            }
        }
        original.sort();
        partitioned.sort();
        assert_eq!(partitioned, original);
    }
    #[test]
    fn render_material_sharing_requires_identical_rendered_fields() {
        let mut map = demo();
        let mut copy = demo().materials.remove(0);
        copy.name = "another source object".into();
        copy.audio = 42;
        map.materials.push(copy);
        let texture_ids = render_texture_ids(&map.textures);
        assert_eq!(render_material_ids(&map.materials, &texture_ids), [0, 0]);
        map.materials[1].roughness = 0.1234;
        assert_eq!(render_material_ids(&map.materials, &texture_ids), [0, 1]);
        map.materials[1].roughness = map.materials[0].roughness;
        map.materials[1].textures[1] = 0;
        assert_eq!(render_material_ids(&map.materials, &texture_ids), [0, 1]);
        assert_eq!(material_ids(&map.materials, &texture_ids, false), [0, 0]);
        map.materials[1].indirect_strength += 1.;
        assert_eq!(material_ids(&map.materials, &texture_ids, false), [0, 1]);
    }
    #[test]
    fn texture_sharing_preserves_pixels_dimensions_and_color_space() {
        let texture = |name: &str, color_space, pixel| skate_data::skate_map::Texture {
            name: name.into(),
            width: 1,
            height: 1,
            color_space,
            rgba: vec![pixel, 0, 0, 255],
        };
        let textures = vec![
            texture("first", 1, 30),
            texture("duplicate", 1, 30),
            texture("linear", 0, 30),
            texture("different", 1, 31),
        ];
        assert_eq!(render_texture_ids(&textures), [0, 1, 1, 3, 4]);
    }
    fn material() -> RetailContactMaterial {
        RetailContactMaterial {
            static_friction: 0.,
            dynamic_friction: 0.,
            restitution: 1.,
        }
    }
    #[test]
    fn native_edges_override_generated_adjacency() {
        let mut map = demo();
        map.geometry.collision[0].native_edges = Some([0x20, 0x42, 0x9a]);
        validate_runtime(&map).unwrap();
        let world = collision_world(&map, material()).unwrap();
        let f = world.triangles()[0].triangle.feature;
        assert!(f.edge_convex(0));
        assert!(!f.edge_convex(1));
        assert!(!f.edge_convex(2));
        assert!(!f.vertex_disabled(0));
        assert!(f.vertex_disabled(1));
        assert!(!f.vertex_disabled(2));
        assert_eq!(f.edge_cosines[0].to_bits(), 0xbe6f4f38);
        assert_eq!(f.edge_cosines[1].to_bits(), 0x3f310b0c);
        assert_eq!(f.edge_cosines[2], 1.);
        assert!(decode_native_edges([31, 0, 0]).is_err());
        assert_eq!(decode_native_edges([0, 2, 26]).unwrap().1, f.edge_cosines);
    }
    #[test]
    fn embedded_archive_is_authoritative_and_preserves_cluster_metadata() {
        let mut map = demo();
        map.geometry.collision.clear();
        map.extensions.push(skate_data::skate_map::Extension {
            tag: *b"RWCM",
            schema: 1,
            payload: include_bytes!("../../skate-data/tests/fixtures/retail-collision.rwcmset")
                .to_vec(),
        });
        validate_runtime(&map).unwrap();
        let world = collision_world(&map, material()).unwrap();
        assert_eq!(world.triangles().len(), 3);
        let metadata = world.query_metadata().unwrap();
        assert_eq!(metadata.meshes.len(), 3);
        assert_eq!(metadata.meshes[1].matching_group, 0x1234);
        assert_eq!(metadata.packed_surfaces, vec![0x4321; 3]);
        assert_eq!(world.triangles()[1].triangle.vertices[0].x, 10.);
    }
    #[test]
    fn native_unit_without_edge_data_keeps_constructor_defaults() {
        let mut archive =
            include_bytes!("../../skate-data/tests/fixtures/retail-collision.rwcmset").to_vec();
        let name_len = u32::from_le_bytes(archive[12..16].try_into().unwrap()) as usize;
        let cluster = 16 + name_len + 4 + 160;
        archive[cluster + 2..cluster + 4].copy_from_slice(&8_u16.to_be_bytes());
        archive[cluster + 80] = 0xc1;
        archive[cluster + 84..cluster + 88].copy_from_slice(&[0x34, 0x12, 0x21, 0x43]);
        let world = retail_collision_world(&archive, material()).unwrap();
        let f = world.triangles()[0].triangle.feature;
        assert_eq!(f.flags, 0x1e1);
        assert_eq!(f.edge_cosines, [-1.; 3]);
    }
    #[test]
    #[ignore = "requires SKATE_MAP_TEST_PATH pointing to a private map"]
    fn private_map_builds_collision_world() {
        let path = std::env::var("SKATE_MAP_TEST_PATH").unwrap();
        let map = SkateMap::load(std::path::Path::new(&path)).unwrap();
        validate_runtime(&map).unwrap();
        let world = collision_world(&map, material()).unwrap();
        let [x, y, z] = map.spawn;
        let hit = world
            .query_thin_line(Vector3::new(x, y + 1., z), Vector3::new(x, y - 10., z))
            .unwrap();
        assert!(hit.is_some(), "spawn has no supporting collision");
        eprintln!(
            "Private map world: {} triangles, spawn hit {:?}",
            world.triangles().len(),
            hit
        );
    }
    #[test]
    fn map_collision_uses_separate_geometry_and_surface_metadata() {
        let mut map = demo();
        map.geometry.vertices[0].position = [1000.; 3];
        map.materials[0].audio = 42;
        map.materials[0].physics = 4;
        map.materials[0].pattern = 7;
        let world = collision_world(&map, material()).unwrap();
        assert_eq!(world.triangles().len(), 2);
        assert_eq!(
            world.triangles()[0].triangle.vertices[0],
            Vector3::new(-30., 0., -30.)
        );
        assert!(world.triangles()[0].triangle.feature.normal.y > 0.999);
        assert_eq!(
            world.query_metadata().unwrap().packed_surfaces,
            vec![42 | (4 << 7) | (7 << 12); 2]
        );
        assert!(world.query_metadata().unwrap().static_edges.is_empty());
    }
    #[test]
    fn shared_flat_diagonal_is_not_a_convex_contact_edge() {
        let world = collision_world(&demo(), material()).unwrap();
        let a = world.triangles()[0].triangle.feature;
        let b = world.triangles()[1].triangle.feature;
        assert!(!a.edge_convex(2));
        assert!(!b.edge_convex(0));
        assert!(a.edge_convex(0));
        assert_eq!(a.edge_cosines[2], 1.);
        assert!(a.vertex_disabled(0));
    }
    #[test]
    fn render_adapter_creates_mesh_and_decoded_lightmap_without_a_window() {
        let map = demo();
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut images = Assets::<Image>::default();
        spawn(
            &map,
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            &mut materials,
            &mut images,
        );
        queue.apply(&mut world);
        assert_eq!(meshes.len(), 1);
        assert_eq!(materials.len(), 1);
        assert_eq!(images.len(), 2);
        assert_eq!(world.query::<&Mesh3d>().iter(&world).count(), 1);
        let lightmap = world
            .query::<&bevy::pbr::Lightmap>()
            .single(&world)
            .unwrap();
        let image = images.get(&lightmap.image).unwrap();
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba16Float);
        let data = image.data.as_ref().unwrap();
        let value = half::f16::from_le_bytes([data[0], data[1]]).to_f32();
        assert!((value - (64. / 255_f32).powi(2) * 4.).abs() < 0.0002);
    }
}
