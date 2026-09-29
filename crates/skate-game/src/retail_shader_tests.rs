//! Naga validation for the retail shaders, with no GPU and no game app.
//!
//! Worth having because the interesting failure modes here are static: a slot
//! record read under non-uniform control flow, a binding number that drifted out
//! of step with `AsBindGroup`, or a struct whose WGSL layout stopped matching the
//! bytes Rust encodes. All of those are compile errors that Naga will name, and
//! none of them need a window.
//!
//! Bevy's own vertex interfaces are used for real, from the vendored sources.
//! Lighting resources and functions the retail shaders only *call* are interface
//! fixtures — the point is to type-check our code, not re-validate Bevy's.
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
};
use std::collections::HashMap;

/// Composes one of our shaders against the Bevy interface and validates it.
fn validate(source: &str, extras: &[&str]) -> naga::Module {
    let mut defs = HashMap::from([("MATERIAL_BIND_GROUP".into(), ShaderDefValue::UInt(3))]);
    for &name in [
        "VERTEX_POSITIONS",
        "VERTEX_UVS_A",
        "VERTEX_UVS_B",
        "VERTEX_TANGENTS",
        "VERTEX_COLORS",
        "VERTEX_NORMALS",
        "VERTEX_OUTPUT_INSTANCE_INDEX",
    ]
    .iter()
    .chain(extras)
    {
        defs.insert(name.into(), ShaderDefValue::Bool(true));
    }
    let mut composer = Composer::default().with_capabilities(naga::valid::Capabilities::all());
    let fixtures = [
        (
            "forward",
            include_str!("../../../vendor/bevy_pbr/src/render/forward_io.wgsl").to_string(),
        ),
        (
            "prepass",
            include_str!("../../../vendor/bevy_pbr/src/prepass/prepass_io.wgsl").to_string(),
        ),
        (
            "mesh",
            "#define_import_path bevy_pbr::mesh_bindings\n\
             struct Mesh { material_and_lightmap_bind_group_slot: u32 }\n\
             @group(2) @binding(0) var<storage> mesh: array<Mesh>;"
                .to_string(),
        ),
        (
            "frame",
            "#define_import_path bevy_pbr::mesh_view_bindings\n\
             struct View {view_from_world:mat4x4<f32>,clip_from_world:mat4x4<f32>,viewport:vec4<f32>,world_position:vec3<f32>,padding:f32}\n\
             struct Light {flags:u32}\n\
             struct Lights {n_directional_lights:u32,directional_lights:array<Light,10>}\n\
             @group(0) @binding(0) var<uniform> view:View;\n\
             @group(0) @binding(1) var<storage> lights:Lights;"
                .to_string(),
        ),
        (
            "shadows",
            "#define_import_path bevy_pbr::shadows\n\
             fn fetch_directional_shadow(id:u32,p:vec4<f32>,n:vec3<f32>,z:f32)->f32 {return 1.0;}"
                .to_string(),
        ),
        (
            "motion",
            "#define_import_path bevy_pbr::pbr_prepass_functions\n\
             fn calculate_motion_vector(p:vec4<f32>,q:vec4<f32>)->vec2<f32> {return vec2<f32>(0.0);}"
                .to_string(),
        ),
        (
            "transformations",
            "#define_import_path bevy_pbr::view_transformations\n\
             fn position_world_to_clip(p:vec3<f32>)->vec4<f32> {return vec4<f32>(p,1.0);}"
                .to_string(),
        ),
        (
            "mesh_functions",
            "#define_import_path bevy_pbr::mesh_functions\n\
             fn get_world_from_local(i:u32)->mat4x4<f32> {return mat4x4<f32>();}\n\
             fn mesh_position_local_to_world(m:mat4x4<f32>,p:vec4<f32>)->vec4<f32> {return m*p;}\n\
             fn mesh_normal_local_to_world(n:vec3<f32>,i:u32)->vec3<f32> {return n;}\n\
             fn mesh_tangent_local_to_world(m:mat4x4<f32>,t:vec4<f32>,i:u32)->vec4<f32> {return t;}"
                .to_string(),
        ),
        (
            "bindings",
            include_str!("retail_material_bindings.wgsl").to_string(),
        ),
        (
            "character_common",
            include_str!("retail_character_common.wgsl").to_string(),
        ),
    ];
    for (path, source) in fixtures {
        if let Err(error) = composer.add_composable_module(ComposableModuleDescriptor {
            source: &source,
            file_path: path,
            shader_defs: defs.clone(),
            ..Default::default()
        }) {
            panic!("{}", error.emit_to_string(&composer));
        }
    }
    let module = composer
        .make_naga_module(NagaModuleDescriptor {
            source,
            file_path: "retail",
            shader_defs: defs,
            ..Default::default()
        })
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&composer)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    module
}

/// The world shader is the one at risk: `slot` is a per-vertex attribute, so
/// every family branch is non-uniform control flow and implicit-derivative
/// sampling inside one is invalid WGSL. If this passes, the explicit gradients
/// are complete.
#[test]
fn world_shader_validates_under_non_uniform_material_slots() {
    validate(include_str!("retail_world.wgsl"), &[]);
}

/// The dome is one draw with no material table, so what is worth checking is the
/// binding numbers: they are written out by hand here and have to stay in step
/// with `SkyMaterial`'s `AsBindGroup` derive.
#[test]
fn sky_shader_validates() {
    validate(include_str!("retail_sky.wgsl"), &[]);
}

/// The shadow pass is the only place a second vertex layout is in play, and a
/// mismatch there aborts pipeline creation rather than degrading. Cover each
/// combination Bevy can ask for: cutting classes sample a page, and directional
/// cascades emulate unclipped depth on adapters without depth clip control.
#[test]
fn depth_shader_validates_in_every_shadow_configuration() {
    for extras in [
        &[][..],
        &["WORLD_ALPHA_CUTOFF"],
        &["UNCLIPPED_DEPTH_ORTHO_EMULATION"],
        &["WORLD_ALPHA_CUTOFF", "UNCLIPPED_DEPTH_ORTHO_EMULATION"],
    ] {
        validate(include_str!("retail_depth.wgsl"), extras);
    }
}

/// Metal translation regression test: WGSL that validates can still fail
/// Naga's MSL backend (cube-array explicit gradients have no `gradient2d`
/// translation, which broke macOS pipeline creation while Windows Vulkan
/// stayed green). Every shipped fragment shader must survive translation on
/// the backend Macs actually run; this runs on all CI hosts (pure CPU).
#[test]
fn shipped_shaders_translate_to_metal() {
    for (name, source, extras) in [
        ("world", include_str!("retail_world.wgsl"), &[][..]),
        ("character", include_str!("retail_character.wgsl"), &[][..]),
        ("sky", include_str!("retail_sky.wgsl"), &[][..]),
        ("depth", include_str!("retail_depth.wgsl"), &[][..]),
        (
            "depth-cutoff",
            include_str!("retail_depth.wgsl"),
            &["WORLD_ALPHA_CUTOFF"][..],
        ),
        // retail_tone.wgsl omitted: it needs the fullscreen-vertex import
        // fixture and contains no gradient/binding constructs at risk.
    ] {
        let module = validate(source, extras);
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: revalidate failed: {e:?}"));
        // Match what wgpu requests from modern Metal devices; the default
        // 1.0 rejects even builtin instance indexing.
        let options = naga::back::msl::Options { lang_version: (2, 4), ..Default::default() };
        if let Err(error) = naga::back::msl::write_string(
            &module,
            &info,
            &options,
            &naga::back::msl::PipelineOptions::default(),
        ) {
            panic!("{name}: Metal translation failed: {error:?}");
        }
    }
}

/// Each vertex input location of a shader's `vertex` entry point, against a
/// description of the type it expects to receive there.
fn vertex_locations(module: &naga::Module) -> HashMap<u32, String> {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == "vertex")
        .expect("shader must own its vertex entry point");
    let mut found = HashMap::new();
    for argument in &entry.function.arguments {
        if let Some(naga::Binding::Location { location, .. }) = argument.binding {
            found.insert(location, format!("{:?}", module.types[argument.ty].inner));
        } else if let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner {
            for member in members {
                if let Some(naga::Binding::Location { location, .. }) = member.binding {
                    found.insert(location, format!("{:?}", module.types[member.ty].inner));
                }
            }
        }
    }
    found
}

/// The shadow pass reads the same vertex buffer as the main pass, so the two
/// shaders have to agree with `WorldMaterial::specialize` about which attribute
/// each location carries. Disagreeing is what made `prepass_pipeline` fail to
/// build: Bevy's own prepass shaders put `COLOR` on location 7.
#[test]
fn depth_shader_vertex_locations_match_the_world_shader() {
    let world = vertex_locations(&validate(include_str!("retail_world.wgsl"), &[]));
    let depth = vertex_locations(&validate(include_str!("retail_depth.wgsl"), &[]));
    for (location, ty) in &depth {
        assert_eq!(
            world.get(location),
            Some(ty),
            "location {location} differs from the world shader's vertex layout"
        );
    }
}

#[test]
#[ignore = "Explicit headless GPU compiler probe; no game or window"]
fn vulkan_material_pipeline_probe() {
    gpu_probe(false);
}

#[test]
#[ignore = "Explicit headless GPU shadow compiler probe; no game or window"]
fn vulkan_shadow_pipeline_probe() {
    gpu_probe(true);
}

fn gpu_probe(prepass: bool) {
    bevy::tasks::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(target_os = "macos")]
            backends: wgpu::Backends::METAL,
            #[cfg(not(target_os = "macos"))]
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        eprintln!("PROBE adapter={:?}", adapter.get_info());
        let features = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::BUFFER_BINDING_ARRAY
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: features,
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .unwrap();
        allocation_reuses_resident_textures(&device);
        // Probe the same shader the runtime compiles: the depth prepass or
        // the full world material pipeline.
        let module = if prepass {
            validate(include_str!("retail_depth.wgsl"), &[])
        } else {
            validate(include_str!("retail_world.wgsl"), &[])
        };
        let mut groups: Vec<Vec<wgpu::BindGroupLayoutEntry>> = vec![vec![]; 4];
        for (_, variable) in module.global_variables.iter() {
            let Some(binding) = variable.binding else {
                continue;
            };
            let (ty, count) = match module.types[variable.ty].inner {
                naga::TypeInner::BindingArray { base, .. } => (base, std::num::NonZeroU32::new(64)),
                _ => (variable.ty, None),
            };
            let resource = match module.types[ty].inner {
                naga::TypeInner::Sampler { comparison: false } => {
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                }
                naga::TypeInner::Image {
                    dim,
                    arrayed: false,
                    ..
                } => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: match dim {
                        naga::ImageDimension::D2 => wgpu::TextureViewDimension::D2,
                        naga::ImageDimension::Cube => wgpu::TextureViewDimension::Cube,
                        _ => panic!("image"),
                    },
                    multisampled: false,
                },
                _ => wgpu::BindingType::Buffer {
                    ty: if variable.space == naga::AddressSpace::Uniform {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: true }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            };
            groups[binding.group as usize].push(wgpu::BindGroupLayoutEntry {
                binding: binding.binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: resource,
                count,
            });
        }
        let layouts: Vec<_> = groups
            .iter()
            .map(|entries| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries,
                })
            })
            .collect();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &layouts.iter().collect::<Vec<_>>(),
            push_constant_ranges: &[],
        });
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("probe vertex"),source:wgpu::ShaderSource::Wgsl((if prepass { r#"
struct Out { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) uv_b:vec2<f32>, @location(4) world:vec4<f32>, @location(7) @interpolate(flat) instance:u32, @location(8) color:vec4<f32> }
@vertex fn vertex()->Out {var o:Out; o.position=vec4<f32>(0,0,0,1); return o;}
"# } else { r#"
struct Out { @builtin(position) position:vec4<f32>, @location(0) world:vec4<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) uv_b:vec2<f32>, @location(4) tangent:vec4<f32>, @location(5) color:vec4<f32>, @location(6) @interpolate(flat) instance:u32 }
@vertex fn vertex(@builtin(vertex_index) index:u32,@builtin(instance_index) instance:u32)->Out {var o:Out; let xy=vec2<f32>(f32((index<<1u)&2u),f32(index&2u))*2.0-1.0; o.position=vec4<f32>(xy,0,1); o.world=vec4<f32>(xy,0,1); o.normal=vec3<f32>(0,1,0); o.tangent=vec4<f32>(1,0,0,1); o.uv=(xy+1.0)*0.5; o.uv_b=o.uv; o.instance=instance; return o;}
"# }).into())});
        eprintln!("PROBE create shader");
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retail probe"),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
        });
        eprintln!("PROBE create pipeline");
        let color_targets = [Some(wgpu::TextureFormat::Rgba8Unorm.into())];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retail compiler probe"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: prepass.then_some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: if prepass { 1 } else { 8 },
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: if prepass { &[] } else { &color_targets },
            }),
            multiview: None,
            cache: None,
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        eprintln!("PROBE pipeline compiled");
        if prepass {
            return;
        }
        // Read back two distinct material slots. This exercises indexed buffer
        // and texture resources, queue submission and 8x-MSAA resolve on Vulkan.
        let bindless = std::env::var_os("SKATE_SHADER_PROBE_FALLBACK").is_none();
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16384,
                usage: wgpu::BufferUsages::UNIFORM
                    | wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let view_buffer = buffer("view");
        let zero_buffer = buffer("zero");
        let mesh_buffer = buffer("mesh");
        let table_buffer = buffer("table");
        let params_buffer = buffer("params");
        let bytes = |values: Vec<f32>| {
            values
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
        };
        let mut view = vec![0.0; 24];
        view[19] = 640.0;
        view[22] = 2.0;
        queue.write_buffer(&view_buffer, 0, &bytes(view));
        let mut params = vec![0.0; 52 * 64];
        for slot in 0..64 {
            let row = &mut params[slot * 52..(slot + 1) * 52];
            row[0] = 14.0;
            row[2] = -1.0;
            row[3] = if slot == 0 { 0.25 } else { 0.75 };
            row[18] = 1.0;
            row[37] = 1.0;
        }
        queue.write_buffer(&params_buffer, 0, &bytes(params));
        queue.write_buffer(
            &mesh_buffer,
            0,
            &(0u32..64).flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
        );
        let mut table = vec![0u32; 17 * 64];
        for slot in 0..64 {
            table[slot * 17] = slot as u32;
        }
        queue.write_buffer(
            &table_buffer,
            0,
            &table
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let make_texture = |layers| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let texture = make_texture(1);
        let cube = make_texture(6);
        for (tex, layers) in [(&texture, 1), (&cube, 6)] {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &vec![255u8; 4 * layers as usize],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: layers,
                },
            );
        }
        let texture_view = texture.create_view(&Default::default());
        let cube_view = cube.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let sampler = device.create_sampler(&Default::default());
        let samplers = vec![&sampler; 64];
        let textures = vec![&texture_view; 64];
        let cubes = vec![&cube_view; 64];
        let frame_buffers = vec![zero_buffer.as_entire_buffer_binding(); 64];
        let mut bind_groups = vec![];
        for (group, entries) in groups.iter().enumerate() {
            let entries: Vec<_> = entries
                .iter()
                .map(|entry| {
                    let resource = match entry.ty {
                        wgpu::BindingType::Sampler(_) if entry.count.is_some() => {
                            wgpu::BindingResource::SamplerArray(&samplers)
                        }
                        wgpu::BindingType::Sampler(_) => wgpu::BindingResource::Sampler(&sampler),
                        wgpu::BindingType::Texture {
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            ..
                        } if entry.count.is_some() => {
                            wgpu::BindingResource::TextureViewArray(&cubes)
                        }
                        wgpu::BindingType::Texture {
                            view_dimension: wgpu::TextureViewDimension::Cube,
                            ..
                        } => wgpu::BindingResource::TextureView(&cube_view),
                        wgpu::BindingType::Texture { .. } if entry.count.is_some() => {
                            wgpu::BindingResource::TextureViewArray(&textures)
                        }
                        wgpu::BindingType::Texture { .. } => {
                            wgpu::BindingResource::TextureView(&texture_view)
                        }
                        _ if entry.count.is_some() => {
                            wgpu::BindingResource::BufferArray(&frame_buffers)
                        }
                        _ => match (group, entry.binding) {
                            (0, 0) => &view_buffer,
                            (2, 0) => &mesh_buffer,
                            (3, 0) if bindless => &table_buffer,
                            (3, 0) | (3, 17) => &params_buffer,
                            _ => &zero_buffer,
                        }
                        .as_entire_binding(),
                    };
                    wgpu::BindGroupEntry {
                        binding: entry.binding,
                        resource,
                    }
                })
                .collect();
            bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layouts[group],
                entries: &entries,
            }));
        }
        let target = |samples| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | if samples == 1 {
                        wgpu::TextureUsages::COPY_SRC
                    } else {
                        wgpu::TextureUsages::empty()
                    },
                view_formats: &[],
            })
        };
        let msaa = target(8);
        let output = target(1);
        let msaa_view = msaa.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4096,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        for slot in 0..if bindless { 2 } else { 1 } {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &msaa_view,
                        depth_slice: None,
                        resolve_target: Some(&output_view),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Discard,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                for (index, group) in bind_groups.iter().enumerate() {
                    pass.set_bind_group(index as u32, group, &[]);
                }
                pass.draw(0..3, slot..slot + 1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &output,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(16),
                    },
                },
                wgpu::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            let slice = readback.slice(..);
            let (send, recv) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            recv.recv().unwrap().unwrap();
            let data = slice.get_mapped_range();
            let pixel = &data[8 * 256 + 8 * 4..8 * 256 + 8 * 4 + 4];
            let expected = if slot == 0 { 64i16 } else { 191 };
            assert!(
                (i16::from(pixel[0]) - expected).abs() <= 1,
                "slot {slot}: {pixel:?}"
            );
            assert_eq!(pixel[3], 255);
            eprintln!("PROBE slot={slot} pixel={pixel:?}");
            drop(data);
            readback.unmap();
        }
    });
}

// Exercise the real allocator against GPU resource identities and retirement.
fn allocation_reuses_resident_textures(device: &wgpu::Device) {
    use bevy::render::render_resource::{
        BindGroupLayoutDescriptor, BindingNumber, BindingResources, BindlessDescriptor,
        BindlessIndex, BindlessIndexTableDescriptor, BindlessResourceType,
        BindlessSlabResourceLimit, OwnedBindingResource, UnpreparedBindGroup,
    };
    let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
    let layout = BindGroupLayoutDescriptor::new("allocation regression", &[]);
    let descriptor = BindlessDescriptor {
        resources: vec![BindlessResourceType::Texture2d; 2].into(),
        buffers: vec![].into(),
        index_tables: vec![BindlessIndexTableDescriptor {
            indices: BindlessIndex(0)..BindlessIndex(2),
            binding_number: BindingNumber(0),
        }]
        .into(),
    };
    let mut allocator = bevy::pbr::MaterialBindGroupAllocator::new(
        &render_device,
        "allocation regression",
        Some(descriptor),
        layout.clone(),
        Some(BindlessSlabResourceLimit::Custom(2)),
    );
    let views: Vec<bevy::render::render_resource::TextureView> = (0..4)
        .map(|_| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
                .into()
        })
        .collect();
    let group = |a: usize, b: usize| UnpreparedBindGroup {
        bindings: BindingResources(vec![
            (
                0,
                OwnedBindingResource::TextureView(wgpu::TextureViewDimension::D2, views[a].clone()),
            ),
            (
                1,
                OwnedBindingResource::TextureView(wgpu::TextureViewDimension::D2, views[b].clone()),
            ),
        ]),
    };
    let first = allocator.allocate_unprepared(group(0, 1), &layout);
    let second = allocator.allocate_unprepared(group(2, 3), &layout);
    assert_ne!(first.group, second.group);
    allocator.free(first);
    let shared = allocator.allocate_unprepared(group(2, 3), &layout);
    assert_eq!(
        shared.group, second.group,
        "reuse resident textures instead of duplicating into an empty earlier slab"
    );
    allocator.free(second);
    allocator.free(shared);
    let fresh = allocator.allocate_unprepared(group(0, 3), &layout);
    assert_eq!(fresh.group, first.group);
    eprintln!("PROBE allocator resource reuse and retirement passed");
}
#[test]
fn character_shaders_validate() {
    validate(include_str!("retail_character.wgsl"), &[]);
    validate(include_str!("retail_character_depth.wgsl"), &[]);
    validate(
        include_str!("retail_character_depth.wgsl"),
        &[
            "PREPASS_FRAGMENT",
            "NORMAL_PREPASS",
            "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
            "MOTION_VECTOR_PREPASS",
            "UNCLIPPED_DEPTH_ORTHO_EMULATION",
        ],
    );
}

/// The vertex entry point declares locations 0..5, and `specialize` pins the
/// mesh attributes to those same locations. A mismatch is silent corruption
/// rather than an error, so assert the shader's own view of its inputs.
#[test]
fn world_vertex_inputs_match_the_pinned_attribute_locations() {
    let module = validate(include_str!("retail_world.wgsl"), &[]);
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == "vertex")
        .expect("world shader must own its vertex entry point");
    let mut locations: Vec<u32> = entry
        .function
        .arguments
        .iter()
        .filter_map(|argument| match argument.binding {
            Some(naga::Binding::Location { location, .. }) => Some(location),
            _ => None,
        })
        .collect();
    // A single struct argument carries the locations on its members instead.
    if locations.is_empty() {
        for argument in &entry.function.arguments {
            if let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner {
                for member in members {
                    if let Some(naga::Binding::Location { location, .. }) = member.binding {
                        locations.push(location);
                    }
                }
            }
        }
    }
    locations.sort_unstable();
    assert_eq!(locations, [0, 1, 2, 3, 4, 5, 6]);
}
