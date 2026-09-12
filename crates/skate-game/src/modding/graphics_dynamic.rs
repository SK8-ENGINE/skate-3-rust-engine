//! Graphics extension 3 host: procedural mesh buffers and scene lights.
use super::{Mods, resolve_body};
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use skate_mods::graphics_dynamic::{LightOptions, MeshBufferOptions, MeshBufferWrite};
use std::{collections::BTreeMap, path::Path};

type Key = (String, String);
const MAX_BUFFERS_PER_MOD: usize = 32;
const MAX_BUFFERS_TOTAL: usize = 128;
const MAX_LIGHTS_PER_MOD: usize = 16;
const MAX_LIGHTS_TOTAL: usize = 64;

const MAX_TEXTURE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TEXTURES_PER_MOD: usize = 16;
const MAX_TEXTURES_TOTAL: usize = 64;

struct CachedTexture {
    handle: Handle<Image>,
}

#[derive(Resource, Default)]
struct ModMeshTextures {
    clips: BTreeMap<(String, String), CachedTexture>,
}

struct MeshBuffer {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    body: Option<String>,
    transform: skate_mods::scene::TransformState,
    visible: bool,
}

struct OwnedLight {
    entity: Entity,
    body: Option<String>,
    origin: Vec3,
    offset: Vec3,
    kind: String,
    direction: Vec3,
}

#[derive(Resource, Default)]
struct ModGraphicsDynamic {
    buffers: BTreeMap<Key, MeshBuffer>,
    lights: BTreeMap<Key, OwnedLight>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ModMeshTextures>();
    app.init_resource::<ModGraphicsDynamic>();
    app.add_systems(Update, sync.after(super::update).after(super::present_camera));
}

fn decode_texture_png(input: &[u8]) -> Result<Image, String> {
    let image = Image::from_buffer(
        input,
        ImageType::MimeType("image/png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| format!("PNG decode failed: {e}"))?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err("Texture dimensions out of range".into());
    }
    Ok(image)
}

fn ensure_texture(
    world: &mut World, cache: &mut ModMeshTextures, owner: &str, root: &Path, path: &str,
) -> Result<Handle<Image>, String> {
    let key = (owner.to_owned(), path.to_owned());
    if let Some(clip) = cache.clips.get(&key) {
        return Ok(clip.handle.clone());
    }
    if !skate_mods::graphics_dynamic::valid_texture_path(path) {
        return Err("Invalid texture path".into());
    }
    if cache.clips.len() >= MAX_TEXTURES_TOTAL
        || cache.clips.keys().filter(|(o, _)| o == owner).count() >= MAX_TEXTURES_PER_MOD
    {
        return Err("Texture cache limit reached (16/mod, 64 total)".into());
    }
    let input = skate_mods::read_bounded(root, path, MAX_TEXTURE_BYTES)?;
    let image = decode_texture_png(&input)?;
    let Some(mut assets) = world.get_resource_mut::<Assets<Image>>() else {
        return Err("Image assets unavailable".into());
    };
    let handle = assets.add(image);
    cache.clips.insert(key, CachedTexture { handle: handle.clone() });
    Ok(handle)
}

fn release_owner_textures(world: &mut World, cache: &mut ModMeshTextures, owner: &str) {
    let keys: Vec<_> = cache.clips.keys().filter(|(o, _)| o == owner).cloned().collect();
    for key in keys {
        if let Some(clip) = cache.clips.remove(&key) {
            world.resource_mut::<Assets<Image>>().remove(clip.handle.id());
        }
    }
}

fn world_point(
    mods: &Mods, owner: &str, body: Option<&str>, origin: Vec3, offset: Vec3,
) -> Option<Vec3> {
    if let Some(body) = body {
        let snapshot = mods.world.read(resolve_body(mods, owner, body).ok()?)?;
        let q = snapshot.rotation;
        let rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
        Some(Vec3::from_array(snapshot.position) + rotation * (origin + offset))
    } else {
        Some(origin + offset)
    }
}

fn remove_buffer(world: &mut World, state: &mut ModGraphicsDynamic, key: &Key) {
    if let Some(buffer) = state.buffers.remove(key) {
        world.despawn(buffer.entity);
        world.resource_mut::<Assets<Mesh>>().remove(buffer.mesh.id());
        world.resource_mut::<Assets<StandardMaterial>>().remove(buffer.material.id());
    }
}

fn remove_light(world: &mut World, state: &mut ModGraphicsDynamic, key: &Key) {
    if let Some(light) = state.lights.remove(key) {
        world.despawn(light.entity);
    }
}

pub(super) fn mesh_buffer(
    world: &mut World, mods: &Mods, owner: &str, key: String, options: MeshBufferOptions,
) -> Result<(), String> {
    if !options.validate() {
        return Err("Invalid mesh buffer options".into());
    }
    if options.body.is_some() {
        resolve_body(mods, owner, options.body.as_deref().unwrap())?;
    }
    let root = mods
        .manager
        .packages
        .get(owner)
        .ok_or("Missing mesh buffer owner")?
        .root
        .clone();
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let slot = (owner.to_owned(), key);
        if !state.buffers.contains_key(&slot)
            && (state.buffers.len() >= MAX_BUFFERS_TOTAL
                || state.buffers.keys().filter(|(o, _)| o == owner).count() >= MAX_BUFFERS_PER_MOD)
        {
            return Err("Mesh buffer limit reached (32/mod, 128 total)".into());
        }
        remove_buffer(world, &mut state, &slot);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(empty_mesh());
        let texture = if let Some(path) = options.texture.as_deref() {
            Some(world.resource_scope(|world, mut cache: Mut<ModMeshTextures>| {
                ensure_texture(world, &mut cache, owner, &root, path)
            })?)
        } else {
            None
        };
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::linear_rgb(options.tint[0], options.tint[1], options.tint[2]),
            base_color_texture: texture,
            alpha_mode: if options.blend { AlphaMode::Blend } else { AlphaMode::Opaque },
            unlit: options.unlit,
            cull_mode: None,
            depth_bias: options.depth_bias,
            ..default()
        });
        let transform = skate_mods::scene::TransformState {
            position: options.position.unwrap_or([0.0; 3]),
            rotation: options.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
            scale: options.scale,
        };
        let visibility = if options.visible { Visibility::Visible } else { Visibility::Hidden };
        let entity = world
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                super::graphics::transform(&transform),
                visibility,
                crate::retail_character::ModGraphicsLit,
            ))
            .id();
        state.buffers.insert(
            slot,
            MeshBuffer {
                entity,
                mesh,
                material,
                body: options.body,
                transform,
                visible: options.visible,
            },
        );
        Ok(())
    })
}

pub(super) fn mesh_buffer_write(
    world: &mut World, _mods: &Mods, owner: &str, key: &str, data: MeshBufferWrite,
) -> Result<(), String> {
    if !data.validate() {
        return Err("Invalid mesh buffer write".into());
    }
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let slot = (owner.to_owned(), key.to_owned());
        let buffer = state.buffers.get_mut(&slot).ok_or("Unknown mesh buffer key")?;
        write_mesh(world, &buffer.mesh, data.positions.clone(), &data)?;
        if let Some(mut visibility) = world.get_mut::<Visibility>(buffer.entity) {
            *visibility = if buffer.visible && !data.positions.is_empty() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
        Ok(())
    })
}

pub(super) fn mesh_buffer_append(
    world: &mut World, _mods: &Mods, owner: &str, key: &str, data: MeshBufferWrite,
) -> Result<(), String> {
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let slot = (owner.to_owned(), key.to_owned());
        let buffer = state.buffers.get_mut(&slot).ok_or("Unknown mesh buffer key")?;
        let (base_verts, base_indices) = mesh_counts(world, &buffer.mesh)?;
        if !data.validate_append(base_verts, base_indices) {
            return Err("Invalid mesh buffer append".into());
        }
        append_mesh(world, &buffer.mesh, &data)?;
        if let Some(mut visibility) = world.get_mut::<Visibility>(buffer.entity) {
            *visibility = if buffer.visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
        Ok(())
    })
}

fn mesh_counts(world: &World, handle: &Handle<Mesh>) -> Result<(usize, usize), String> {
    let meshes = world.resource::<Assets<Mesh>>();
    let Some(mesh) = meshes.get(handle) else {
        return Err("Mesh buffer asset missing".into());
    };
    let verts = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x3(values) => Some(values.len()),
            _ => None,
        })
        .unwrap_or(0);
    let indices = match mesh.indices() {
        Some(Indices::U32(values)) => values.len(),
        Some(Indices::U16(values)) => values.len(),
        None => 0,
    };
    Ok((verts, indices))
}

fn read_indices(mesh: &Mesh) -> Vec<u32> {
    match mesh.indices() {
        Some(Indices::U32(values)) => values.clone(),
        Some(Indices::U16(values)) => values.iter().map(|i| *i as u32).collect(),
        None => Vec::new(),
    }
}

fn append_mesh(world: &mut World, handle: &Handle<Mesh>, data: &MeshBufferWrite) -> Result<(), String> {
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let Some(mesh) = meshes.get_mut(handle) else {
        return Err("Mesh buffer asset missing".into());
    };
    let mut positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x3(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x3(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut colors = mesh
        .attribute(Mesh::ATTRIBUTE_COLOR)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x4(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut uvs = mesh
        .attribute(Mesh::ATTRIBUTE_UV_0)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x2(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut indices = read_indices(mesh);
    if positions.len() != normals.len() {
        normals.resize(positions.len(), [0.0, 1.0, 0.0]);
    }
    if positions.len() != colors.len() {
        colors.resize(positions.len(), [1.0, 1.0, 1.0, 1.0]);
    }
    if positions.len() != uvs.len() {
        uvs.resize(positions.len(), [0.0, 0.0]);
    }
    let new_normals = data
        .normals
        .clone()
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; data.positions.len()]);
    let new_colors = data
        .colors
        .as_ref()
        .filter(|flat| flat.len() == data.positions.len() * 4)
        .map(|flat| {
            flat.chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![[1.0, 1.0, 1.0, 1.0]; data.positions.len()]);
    let new_uvs = data
        .uvs
        .as_ref()
        .filter(|flat| flat.len() == data.positions.len() * 2)
        .map(|flat| {
            flat.chunks_exact(2)
                .map(|uv| [uv[0], uv[1]])
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![[0.0, 0.0]; data.positions.len()]);
    positions.extend(data.positions.iter().cloned());
    normals.extend(new_normals);
    colors.extend(new_colors);
    uvs.extend(new_uvs);
    indices.extend(data.indices.iter().copied());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if indices.is_empty() {
        mesh.remove_indices();
    } else {
        mesh.insert_indices(Indices::U32(indices));
    }
    Ok(())
}

fn write_mesh(
    world: &mut World,
    handle: &Handle<Mesh>,
    positions: Vec<[f32; 3]>,
    data: &MeshBufferWrite,
) -> Result<(), String> {
    let normals = data
        .normals
        .clone()
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
    let colors = data
        .colors
        .as_ref()
        .filter(|flat| flat.len() == positions.len() * 4)
        .map(|flat| {
            flat.chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![[1.0, 1.0, 1.0, 1.0]; positions.len()]);
    let uvs = data
        .uvs
        .as_ref()
        .filter(|flat| flat.len() == positions.len() * 2)
        .map(|flat| {
            flat.chunks_exact(2)
                .map(|uv| [uv[0], uv[1]])
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let Some(mesh) = meshes.get_mut(handle) else {
        return Err("Mesh buffer asset missing".into());
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(data.indices.clone()));
    Ok(())
}

pub(super) fn light(
    world: &mut World, mods: &Mods, owner: &str, key: String, options: LightOptions,
) -> Result<(), String> {
    if !options.validate() {
        return Err("Invalid light options".into());
    }
    if options.body.is_some() {
        resolve_body(mods, owner, options.body.as_deref().unwrap())?;
    }
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let slot = (owner.to_owned(), key);
        if !state.lights.contains_key(&slot)
            && (state.lights.len() >= MAX_LIGHTS_TOTAL
                || state.lights.keys().filter(|(o, _)| o == owner).count() >= MAX_LIGHTS_PER_MOD)
        {
            return Err("Light limit reached (16/mod, 64 total)".into());
        }
        remove_light(world, &mut state, &slot);
        let position = Vec3::from_array(options.position.unwrap_or([0.0; 3]));
        let direction = options
            .direction
            .map(Vec3::from_array)
            .filter(|d| d.length_squared() > 1e-6)
            .map(|d| d.normalize())
            .unwrap_or(-Vec3::Z);
        let color = Color::linear_rgb(options.color[0], options.color[1], options.color[2]);
        let entity = if options.kind == "spot" {
            world
                .spawn((
                    SpotLight {
                        color,
                        intensity: options.intensity,
                        range: options.range,
                        inner_angle: options.inner_angle,
                        outer_angle: options.outer_angle,
                        ..default()
                    },
                    Transform::from_translation(position).looking_to(direction, Vec3::Y),
                ))
                .id()
        } else {
            world
                .spawn((
                    PointLight {
                        color,
                        intensity: options.intensity,
                        range: options.range,
                        ..default()
                    },
                    Transform::from_translation(position),
                ))
                .id()
        };
        state.lights.insert(
            slot,
            OwnedLight {
                entity,
                body: options.body,
                origin: position,
                offset: Vec3::from_array(options.offset),
                kind: options.kind,
                direction,
            },
        );
        Ok(())
    })
}

pub(super) fn remove(world: &mut World, owner: &str, key: &str) {
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let slot = (owner.to_owned(), key.to_owned());
        remove_buffer(world, &mut state, &slot);
        remove_light(world, &mut state, &slot);
    });
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let buffers: Vec<_> = state.buffers.keys().filter(|(o, _)| o == owner).cloned().collect();
        for key in buffers {
            remove_buffer(world, &mut state, &key);
        }
        let lights: Vec<_> = state.lights.keys().filter(|(o, _)| o == owner).cloned().collect();
        for key in lights {
            remove_light(world, &mut state, &key);
        }
    });
    world.resource_scope(|world, mut cache: Mut<ModMeshTextures>| {
        release_owner_textures(world, &mut cache, owner);
    });
}

pub(super) fn clear(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        let buffers: Vec<_> = state.buffers.keys().cloned().collect();
        for key in buffers {
            remove_buffer(world, &mut state, &key);
        }
        let lights: Vec<_> = state.lights.keys().cloned().collect();
        for key in lights {
            remove_light(world, &mut state, &key);
        }
    });
    world.resource_scope(|world, mut cache: Mut<ModMeshTextures>| {
        for (_, clip) in std::mem::take(&mut cache.clips) {
            world.resource_mut::<Assets<Image>>().remove(clip.handle.id());
        }
    });
}

fn empty_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
        .with_inserted_indices(Indices::U32(Vec::new()))
}

fn sync(world: &mut World) {
    let (origins, buffer_poses, dead_buffers) = {
        let mods = world.resource::<Mods>();
        let state = world.resource::<ModGraphicsDynamic>();
        let origins: Vec<(Key, Option<Vec3>)> = state
            .lights
            .iter()
            .map(|(key, light)| {
                (
                    key.clone(),
                    world_point(mods, &key.0, light.body.as_deref(), light.origin, light.offset),
                )
            })
            .collect();
        let buffer_poses = state
            .buffers
            .iter()
            .filter_map(|(key, buffer)| {
                let body = buffer.body.as_deref()?;
                let body_id = resolve_body(mods, &key.0, body).ok()?;
                let snapshot = mods.world.read(body_id)?;
                let q = snapshot.rotation;
                let rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
                let mut transform = super::graphics::transform(&buffer.transform);
                transform.translation = Vec3::from_array(snapshot.position)
                    + rotation * Vec3::from_array(buffer.transform.position);
                transform.rotation = rotation * Quat::from_array(buffer.transform.rotation);
                Some((key.clone(), transform))
            })
            .collect::<Vec<_>>();
        let dead_buffers: Vec<Key> = state
            .buffers
            .iter()
            .filter(|(key, buffer)| {
                buffer.body.is_some()
                    && world_point(
                        mods,
                        &key.0,
                        buffer.body.as_deref(),
                        Vec3::from_array(buffer.transform.position),
                        Vec3::ZERO,
                    )
                    .is_none()
            })
            .map(|(key, _)| key.clone())
            .collect();
        (origins, buffer_poses, dead_buffers)
    };
    world.resource_scope(|world, mut state: Mut<ModGraphicsDynamic>| {
        for (key, position) in origins {
            if position.is_none() {
                remove_light(world, &mut state, &key);
                continue;
            }
            let position = position.unwrap();
            let Some(light) = state.lights.get(&key) else { continue };
            if let Some(mut transform) = world.get_mut::<Transform>(light.entity) {
                transform.translation = position;
                if light.kind == "spot" {
                    transform.look_to(light.direction, Vec3::Y);
                }
            }
        }
        for (key, transform) in buffer_poses {
            let Some(buffer) = state.buffers.get(&key) else { continue };
            if let Some(mut entity) = world.get_mut::<Transform>(buffer.entity) {
                *entity = transform;
            }
        }
        for key in dead_buffers {
            remove_buffer(world, &mut state, &key);
        }
    });
}
