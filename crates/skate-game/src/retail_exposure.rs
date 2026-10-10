//! GPU-only exposure adaptation and the retail tone curve.
//!
//! The world and character shaders emit linear radiance premultiplied by a
//! baseline exposure of 2.5. Nothing downstream of them is a no-op: the tone
//! pass divides that baseline back out, applies the retail curve, and inverts
//! sRGB so Bevy's output attachment does not encode gamma twice. Skipping it
//! clips every bright surface to white, which reads as chrome rather than as
//! an exposure fault.
//!
//! The meter reads what retail's CPU meter (`sub_827F0B78`) reads: the alpha
//! of the bloom downsample, `dp3_sat(sat(tm/2), (0.3, 0.4, 0.3))` (see
//! [`meter_luminance`]), 8-bit, centre-weighted, scaled by 2.515 and fed to the
//! native multiplicative evaluator (`sub_827F0D00`). It samples a 16x16 bilinear
//! grid of the HDR target instead of the console's downsample chain.
//! The weights and the scale are a mod tuning domain (`exposure`, [`ExposureMeter`]).
use bevy::{
    asset::embedded_asset,
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::ExtractComponentPlugin,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::{ViewDepthTexture, ViewTarget},
    },
};
use std::{collections::VecDeque, sync::Mutex};

use crate::retail_render::{RetailScene, RetailTone, world_changed};

#[derive(Resource, Clone, ExtractResource)]
struct Settings {
    tuning: Vec4,
    timing: Vec4,
    /// Meter weights (xyz) and the average scale (w), copied from [`ExposureMeter`].
    meter: Vec4,
}

/// Retail meter channel weights (R, G, B): the `L255` literal (0.3, 0.4) that
/// `bloom_dof_tap4_minusthresholdPS` dots its saturated tm/2 with (`r1.zxy . (0.3, 0.3, 0.4)`).
pub(crate) const RETAIL_METER_WEIGHTS: Vec3 = Vec3::new(0.3, 0.4, 0.3);
/// Retail meter average scale: `sub_827F0D00` multiplies the weighted sum by the
/// float at 0x821A01E8 before dividing by the pixel count.
pub(crate) const RETAIL_METER_SCALE: f32 = 2.515;

/// Engine-side exposure meter constants: retail by default, patched by mods through
/// `sdk.world.set_tuning('exposure', {meter_weights = {r, g, b}, meter_scale = s})` and
/// rebuilt from the default when the mod stops (`modding::world_tuning`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ExposureMeter {
    pub weights: Vec3,
    pub scale: f32,
}

impl Default for ExposureMeter {
    fn default() -> Self {
        Self { weights: RETAIL_METER_WEIGHTS, scale: RETAIL_METER_SCALE }
    }
}

impl ExposureMeter {
    fn packed(&self) -> Vec4 {
        self.weights.extend(self.scale)
    }
}

/// One meter sample as retail computes it, mirrored by `retail_exposure.wgsl`:
/// the scene value `rgb` (linear, baseline exposure 2.5 included) at exposure `e`
/// goes through the retail tone curve, tm/2 is saturated per channel, dotted with
/// the weights, saturated, and stored as an 8-bit value (byte / 255).
pub(crate) fn meter_luminance(rgb: Vec3, e: f32, weights: Vec3) -> f32 {
    let xe = (rgb * e / 2.5).max(Vec3::ZERO);
    let t = (Vec3::ONE - xe).clamp(Vec3::ZERO, Vec3::ONE);
    let tm = (xe * 0.25 + Vec3::splat(0.75)).max(Vec3::ONE) - t * t;
    let lum = (tm * 0.5).clamp(Vec3::ZERO, Vec3::ONE).dot(weights).clamp(0., 1.);
    (lum * 255.).round() / 255.
}

impl Default for Settings {
    fn default() -> Self {
        // Missing metadata retains the existing fixed exposure.
        Self {
            tuning: Vec4::new(0., 2.5, 2.5, 0.),
            timing: Vec4::ZERO,
            meter: ExposureMeter::default().packed(),
        }
    }
}

#[derive(serde::Deserialize)]
struct Authored {
    target_luminance: f32,
    min: f32,
    max: f32,
    damping: f32,
}

pub(crate) struct RetailExposurePlugin;

impl Plugin for RetailExposurePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_exposure.wgsl");
        embedded_asset!(app, "retail_tone.wgsl");
        app.init_resource::<Settings>()
            .init_resource::<ExposureMeter>()
            .init_resource::<crate::colour_matrix::ColourGrade>()
            .init_resource::<crate::colour_matrix::ColourRecords>()
            .add_plugins((
                ExtractResourcePlugin::<Settings>::default(),
                ExtractResourcePlugin::<crate::colour_matrix::ColourGrade>::default(),
                ExtractComponentPlugin::<RetailTone>::default(),
            ))
            .add_systems(Startup, (load, crate::colour_matrix::load_records))
            .add_systems(PostUpdate, crate::colour_matrix::update_grade)
            .add_systems(
                PreUpdate,
                load.after(crate::map_transition::MapTransitionSet)
                    .run_if(world_changed),
            )
            .add_systems(Update, advance);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .add_systems(RenderStartup, initialize)
                .add_systems(Render, upload.in_set(RenderSystems::PrepareResources))
                .add_systems(Render, prune_bindings.in_set(RenderSystems::PrepareBindGroups))
                .add_render_graph_node::<ViewNodeRunner<ExposureNode>>(Core3d, ExposureLabel)
                .add_render_graph_edges(
                    Core3d,
                    (
                        Node3d::Tonemapping,
                        ExposureLabel,
                        Node3d::EndMainPassPostProcessing,
                    ),
                );
        }
    }
}

fn load(
    config: Res<crate::config::Config>,
    retail: Res<RetailScene>,
    mut settings: ResMut<Settings>,
) {
    let generation = settings.timing.y + 1.;
    *settings = Settings::default();
    settings.timing.y = generation;
    if !retail.0 {
        return;
    }
    if std::env::var_os("SKATE_FIXED_EXPOSURE").is_some_and(|v| v == "1") {
        info!("RETAIL_EXPOSURE: fixed 2.5 comparison mode");
        return;
    }
    let profile = config
        .map_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|name| {
            let bytes =
                std::fs::read(config.asset_root.join("private/exposure-profiles.json")).ok()?;
            let mut profiles: std::collections::BTreeMap<String, Authored> =
                serde_json::from_slice(&bytes).ok()?;
            profiles.remove(&name.to_string_lossy().to_lowercase())
        });
    let fallback = || {
        std::fs::read(config.asset_root.join("private/exposure.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Authored>(&b).ok())
    };
    if let Some(a) = profile.or_else(fallback) {
        if [a.target_luminance, a.min, a.max, a.damping]
            .iter()
            .all(|x| x.is_finite())
            && a.min > 0.
            && a.max >= a.min
            && a.target_luminance > 0.
            && a.damping >= 0.
        {
            settings.tuning = Vec4::new(a.target_luminance, a.min, a.max, a.damping);
            info!(
                "RETAIL_EXPOSURE: authored target={} range={}..{} damping={}; retail meter input",
                a.target_luminance, a.min, a.max, a.damping
            );
        }
    }
}

fn advance(
    time: Res<Time>,
    meter: Res<ExposureMeter>,
    mut settings: ResMut<Settings>,
) {
    settings.timing.x = time.delta_secs().clamp(0., 0.05);
    settings.meter = meter.packed();
    if meter.is_changed() {
        info!(
            "RETAIL_EXPOSURE_METER weights=[{:.3}, {:.3}, {:.3}] scale={:.3} retail={}",
            meter.weights.x, meter.weights.y, meter.weights.z, meter.scale,
            *meter == ExposureMeter::default()
        );
    }
}

#[derive(Resource)]
struct Pipeline {
    compute_layout: BindGroupLayoutDescriptor,
    tone_layout: BindGroupLayoutDescriptor,
    compute: CachedComputePipelineId,
    tone: CachedRenderPipelineId,
    settings: Buffer,
    state: Buffer,
    /// The colour grade (`ColourGrade`, 8 vec4) and a 1x1 depth texture used when the view's depth cannot be sampled.
    grade: Buffer,
    fallback_depth: TextureView,
    sampler: Sampler,
    // The buffers, sampler and layouts are immutable for this Pipeline's life.
    // Source views can alternate, resize or belong to different cameras. Keep a
    // bounded cache so retired targets cannot accumulate across map/size changes.
    bindings: Mutex<VecDeque<((TextureViewId, TextureViewId), BindGroup, BindGroup)>>,
}

fn initialize(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
) {
    let compute_layout = BindGroupLayoutDescriptor::new(
        "retail exposure meter",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<[Vec4; 3]>(false),
                storage_buffer::<Vec4>(false),
            ),
        ),
    );
    let tone_layout = BindGroupLayoutDescriptor::new(
        "retail exposed tone",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                storage_buffer_read_only::<Vec4>(false),
                texture_depth_2d(),
                uniform_buffer::<[Vec4; 8]>(false),
            ),
        ),
    );
    let compute = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("retail exposure".into()),
        layout: vec![compute_layout.clone()],
        shader: assets.load(
            bevy::asset::AssetPath::from(bevy::asset::embedded_path!("retail_exposure.wgsl"))
                .with_source("embedded"),
        ),
        entry_point: Some("meter".into()),
        ..default()
    });
    let tone = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("retail exposed tone".into()),
        layout: vec![tone_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load(
                bevy::asset::AssetPath::from(bevy::asset::embedded_path!("retail_tone.wgsl"))
                    .with_source("embedded"),
            ),
            targets: vec![Some(ColorTargetState {
                format: ViewTarget::TEXTURE_FORMAT_HDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    let settings = device.create_buffer(&BufferDescriptor {
        label: Some("retail exposure settings"),
        size: 48,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let state = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("retail exposure state"),
        contents: &bytes([Vec4::new(2.5, 0., 0., 0.)]),
        usage: BufferUsages::STORAGE,
    });
    let grade = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("retail colour grade"),
        contents: &bytes(crate::colour_matrix::ColourGrade::default().rows),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let fallback_depth = device
        .create_texture(&TextureDescriptor {
            label: Some("retail colour grade fallback depth"),
            size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default());
    commands.insert_resource(Pipeline {
        compute_layout,
        tone_layout,
        compute,
        tone,
        settings,
        state,
        grade,
        fallback_depth,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        bindings: default(),
    });
}

fn bytes<const N: usize>(values: [Vec4; N]) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(|v| v.to_array())
        .flat_map(f32::to_le_bytes)
        .collect()
}

fn upload(settings: Res<Settings>, grade: Res<crate::colour_matrix::ColourGrade>, pipeline: Res<Pipeline>, queue: Res<RenderQueue>) {
    queue.write_buffer(&pipeline.grade, 0, &bytes(grade.rows));
    let mut data = [0u8; 48];
    for (chunk, value) in data.chunks_exact_mut(4).zip(
        [settings.tuning, settings.timing, settings.meter]
            .into_iter()
            .flat_map(|v| v.to_array()),
    ) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    queue.write_buffer(&pipeline.settings, 0, &data);
}

fn prune_bindings(pipeline: Res<Pipeline>, views: Query<&ViewTarget, With<RetailTone>>) {
    // Bind groups retain their textures. Release retired camera/resize targets
    // before rendering, including when there are no exposure views left.
    pipeline.bindings.lock().unwrap().retain(|((id, _), _, _)| {
        views.iter().any(|view| {
            *id == view.main_texture_view().id() || *id == view.main_texture_other_view().id()
        })
    });
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ExposureLabel;

#[derive(Default)]
struct ExposureNode;

impl ViewNode for ExposureNode {
    type ViewQuery = (&'static ViewTarget, &'static RetailTone, Option<&'static ViewDepthTexture>);

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext,
        (view, _, depth): QueryItem<Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let p = world.resource::<Pipeline>();
        let cache = world.resource::<PipelineCache>();
        let (Some(compute), Some(tone)) = (
            cache.get_compute_pipeline(p.compute),
            cache.get_render_pipeline(p.tone),
        ) else {
            return Ok(());
        };
        // The view depth for the grade's near / far mix; the fallback when it is multisampled or not sampleable.
        let depth = depth
            .filter(|d| d.texture.sample_count() == 1 && d.texture.usage().contains(TextureUsages::TEXTURE_BINDING))
            .map_or(&p.fallback_depth, |d| d.view());
        let post = view.post_process_write();
        let key = (post.source.id(), depth.id());
        use bevy::render::diagnostic::RecordDiagnostics;
        let diagnostics = context.diagnostic_recorder();
        let mut bindings = p.bindings.lock().unwrap();
        let index = if let Some(index) = bindings
            .iter()
            .position(|(id, _, _)| *id == key)
        {
            index
        } else {
            let meter = context.render_device().create_bind_group(
                "retail meter",
                &cache.get_bind_group_layout(&p.compute_layout),
                &BindGroupEntries::sequential((
                    post.source,
                    &p.sampler,
                    p.settings.as_entire_binding(),
                    p.state.as_entire_binding(),
                )),
            );
            let output = context.render_device().create_bind_group(
                "retail exposed tone",
                &cache.get_bind_group_layout(&p.tone_layout),
                &BindGroupEntries::sequential((
                    post.source,
                    &p.sampler,
                    p.state.as_entire_binding(),
                    depth,
                    p.grade.as_entire_binding(),
                )),
            );
            if bindings.len() == 8 {
                bindings.pop_front();
            }
            bindings.push_back((key, meter, output));
            bindings.len() - 1
        };
        let (_, meter, output) = &bindings[index];
        {
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("retail exposure meter"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(compute);
            let span = diagnostics.pass_span(&mut pass, "retail_exposure_meter");
            pass.set_bind_group(0, meter, &[]);
            pass.dispatch_workgroups(1, 1, 1);
            span.end(&mut pass);
        }
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("retail exposed tone"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(tone);
        let span = diagnostics.pass_span(&mut pass, "retail_exposed_tone");
        pass.set_bind_group(0, output, &[]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's formula written out from bloom_dof_tap4_minusthresholdPS (scene
    /// target value s = sqrt(tm/2); the shader squares s, saturates, dots
    /// r1.zxy with (0.3, 0.3, 0.4), saturates; the CPU reads byte / 255).
    fn retail(rgb: [f32; 3], e: f32) -> f32 {
        let s: Vec<f32> = rgb
            .iter()
            .map(|c| {
                let xe = (c * e / 2.5).max(0.);
                let t = (1. - xe).clamp(0., 1.);
                let tm = (xe * 0.25 + 0.75).max(1.) - t * t;
                (tm * 0.5).sqrt().min(1.)
            })
            .collect();
        let sq: Vec<f32> = s.iter().map(|v| (v * v).clamp(0., 1.)).collect();
        let a = (sq[2] * 0.3 + sq[0] * 0.3 + sq[1] * 0.4).clamp(0., 1.);
        (a * 255.).round() / 255.
    }

    #[test]
    fn meter_matches_retail_formula() {
        for rgb in [[0., 0., 0.], [0.1, 0.2, 0.3], [1., 1., 1.], [2.5, 2.5, 2.5], [0.9, 1.4, 3.0], [40., 2., 0.01]] {
            for e in [0.75, 1.0, 2.0, 2.5] {
                let ours = meter_luminance(Vec3::from_array(rgb), e, RETAIL_METER_WEIGHTS);
                // One 8-bit step: retail's sqrt/square round trip can land either side of a .5 boundary.
                assert!((ours - retail(rgb, e)).abs() <= 1.0 / 255.0 + 1e-6, "{rgb:?} e={e}: {ours} vs {}", retail(rgb, e));
            }
        }
        // xe = 1 is the knee: tm = 1, tm/2 = 0.5 on every channel.
        assert!((meter_luminance(Vec3::ONE, 2.5, RETAIL_METER_WEIGHTS) - 0.5).abs() <= 0.5 / 255.0 + 1e-6);
        // Black stays black; anything bright saturates at 1.
        assert_eq!(meter_luminance(Vec3::ZERO, 2.5, RETAIL_METER_WEIGHTS), 0.);
        assert_eq!(meter_luminance(Vec3::splat(100.), 2.5, RETAIL_METER_WEIGHTS), 1.);
    }

    #[test]
    fn bright_sky_reads_lower_than_the_old_hdr_meter() {
        // A sky pixel at xe ~ 2 read 1.4 through the old capped Rec.709 HDR meter;
        // retail's tone-compressed meter reads under 0.7, so exposure rises.
        let sky = Vec3::new(1.6, 2.1, 2.6) * 2.5 / 2.0;
        let old = (sky * 2.0 / 2.5).dot(Vec3::new(0.2126, 0.7152, 0.0722)).min(16.);
        let new = meter_luminance(sky, 2.0, RETAIL_METER_WEIGHTS);
        assert!(new < 0.7 && old > 1.9, "old {old} new {new}");
    }

    #[test]
    fn retail_defaults() {
        let m = ExposureMeter::default();
        assert_eq!(m.packed(), Vec4::new(0.3, 0.4, 0.3, 2.515));
        assert_eq!(Settings::default().meter, m.packed());
    }
}
