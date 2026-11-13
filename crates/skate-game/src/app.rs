use crate::{
    animation, assets, camera,
    config::Config,
    graph_runtime::StockGraphs,
    input,
    physics::{GamePhysics, PhysicsPlugin, SkaterRuntime},
    verification, world,
};
use bevy::{
    prelude::*,
    render::{
        RenderPlugin,
        settings::{Backends, InstanceFlags, RenderCreation, WgpuFeatures, WgpuSettings},
    },
};
use skate_data::GameAssets;

/// Bevy's defaults plus, on request, the query features that make
/// `RenderDiagnosticsPlugin` report per-pass GPU time.
///
/// Opt-in: a required feature the adapter lacks aborts device creation, and the
/// queries are not free. Without them a pass's cost can only be inferred from
/// invocation counts, which says nothing about how long the pass took. Set
/// `SKATE_GPU_TIMING=1` to get `render/**/elapsed_gpu`.
/// Trace-all: docs/hails-additions/25-perf-diagnostics.md#code-reference-apprs-performancers-frame_timingstatsrs
/// One fixed schedule on Bevy's single-threaded executor, unless Physics Step Threads is Multi
/// (`graphics_menu::physics_threads_multi`).
fn fixed_executor(multi: bool) -> impl Fn(&mut Schedule) + Copy {
    move |schedule: &mut Schedule| {
        if !multi {
            schedule.set_executor_kind(bevy::ecs::schedule::ExecutorKind::SingleThreaded);
        }
    }
}

fn wgpu_features() -> WgpuFeatures {
    let default = WgpuSettings::default().features;
    if crate::trace_all::on() {
        return default;
    }
    if std::env::var_os("SKATE_GPU_TIMING").is_some_and(|v| v != "0") {
        default
            | WgpuFeatures::TIMESTAMP_QUERY
            | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | WgpuFeatures::PIPELINE_STATISTICS_QUERY
    } else {
        default
    }
}

/// The GPU timing features the device got, logged once at startup (trace-all or `SKATE_GPU_TIMING`).
fn log_gpu_timing(device: Option<Res<bevy::render::renderer::RenderDevice>>) {
    let Some(device) = device else { return };
    let features = device.features();
    let has = |f: WgpuFeatures| if features.contains(f) { "yes" } else { "no" };
    info!(
        "GPU_TIMING timestamp_query={} inside_encoders={} pipeline_statistics={}{}",
        has(WgpuFeatures::TIMESTAMP_QUERY),
        has(WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS),
        has(WgpuFeatures::PIPELINE_STATISTICS_QUERY),
        if features.contains(WgpuFeatures::TIMESTAMP_QUERY) { "" } else { " (adapter lacks timestamps: no GPU pass times, the rest of the trace continues)" }
    );
}

/// Hybrid laptops: Windows can hand the game the integrated GPU although a discrete one is present (a much slower
/// game with no other sign). Only when the picked adapter is integrated, list the others and warn with the fix.
fn warn_integrated_gpu(info: Option<Res<bevy::render::renderer::RenderAdapterInfo>>, instance: Option<Res<bevy::render::renderer::RenderInstance>>) {
    // By name: `wgpu` is only a dev-dependency here (Bevy re-exports neither `DeviceType` nor `Instance`).
    let kind = |t: &dyn std::fmt::Debug| format!("{t:?}");
    let (Some(info), Some(instance)) = (info, instance) else { return };
    if kind(&info.device_type) != "IntegratedGpu" {
        return;
    }
    let discrete: Vec<String> = instance.enumerate_adapters(Backends::VULKAN).iter().map(|a| a.get_info()).filter(|a| kind(&a.device_type) == "DiscreteGpu").map(|a| a.name).collect();
    if discrete.is_empty() {
        info!("GPU_ADAPTER integrated {} (no discrete GPU found)", info.name);
    } else {
        warn!(
            "GPU_ADAPTER integrated {} picked although a discrete GPU is present ({}): set skate3rust.exe to \"High performance\" in Windows Settings > System > Display > Graphics",
            info.name,
            discrete.join(", ")
        );
    }
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) enum FrameSet {
    Assets,
    Physics,
    Animation,
    Verification,
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) enum SimulationSet {
    Input,
    Controls,
    Physics,
}

pub(crate) fn build(
    config: Config,
    manifest: GameAssets,
    graphs: StockGraphs,
    physics: GamePhysics,
    skater: SkaterRuntime,
) -> App {
    let retail_scene = config.map.as_ref().is_some_and(|map| crate::retail_render::RetailScene::for_map(map));
    let single_fixed_executor = fixed_executor(crate::graphics_menu::physics_threads_multi(&config.asset_root));
    let mut app = App::new();
    crate::custom_models::register_source(&mut app);
    crate::modding::register_source(&mut app);
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: config.asset_root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: config.multiplayer.title.clone().unwrap_or_else(||"Skate 3 Rust Engine".into()),
                    resolution: (1280, 800).into(),
                    // Hidden until the start is done (`startup_loading`); the splash shows meanwhile.
                    visible: false,
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    backends: Some(Backends::VULKAN),
                    // Existing machine's validation layer rejects wgpu atomic shaders.
                    // This workaround belongs only to the rendering adapter.
                    instance_flags: InstanceFlags::empty(),
                    features: wgpu_features(),
                    ..default()
                }),
                ..default()
            }).build().disable::<bevy::log::LogPlugin>()
            // Gameplay and menu navigation both use raw pad states from
            // input::platform (SDL3, XInput fallback). No game system consumes
            // Bevy gamepad events/rumble; its second device backend can stall
            // PreUpdate (70.68 ms in the University capture).
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .insert_resource(bevy::winit::WinitSettings {focused_mode:bevy::winit::UpdateMode::Continuous,unfocused_mode:bevy::winit::UpdateMode::Continuous})
    .insert_resource(config)
    .insert_resource(crate::retail_render::RetailScene(retail_scene))
    .insert_resource(assets::AssetManifest(manifest))
    .insert_resource(crate::living_world::npc_skaters::NpcScoreClips(crate::graph_host::motion::stock_score_clips(&graphs.motion.source)))
    .insert_resource(graphs)
    .insert_resource(physics)
    .insert_resource(skater)
    .configure_sets(
        FixedUpdate,
        (
            SimulationSet::Input,
            SimulationSet::Controls,
            SimulationSet::Physics,
        )
            .chain(),
    )
    // The fixed step on one thread, without the multi-threaded executor's hand-offs; Physics Step Threads = Multi (Opt-Ins,
    // the launcher's Tools page, or SKATE_FIXED_EXECUTOR=multi) gives Bevy's (docs/hails-additions/25-perf-diagnostics.md#fixed-step-executor).
    .edit_schedule(FixedFirst, single_fixed_executor)
    .edit_schedule(FixedPreUpdate, single_fixed_executor)
    .edit_schedule(FixedUpdate, single_fixed_executor)
    .edit_schedule(FixedPostUpdate, single_fixed_executor)
    .edit_schedule(FixedLast, single_fixed_executor)
    .configure_sets(
        Update,
        (
            FrameSet::Assets,
            FrameSet::Physics,
            FrameSet::Animation,
            FrameSet::Verification,
        )
            .chain(),
    )
    .add_plugins(crate::frame_timing::FrameTimingPlugin)
    .add_plugins(crate::system_timing::SystemTimingPlugin)
    .add_plugins(crate::retail_backdrop::BackdropPlugin)
    .add_plugins((
        crate::retail_render::RetailRenderPlugin,
        input::InputPlugin,
        PhysicsPlugin,
        crate::presentation::PresentationPlugin,
        crate::replay::ReplayPlugin,
        assets::GameAssetsPlugin,
        animation::AnimationPlugin,
        world::WorldPlugin,
        crate::grind_world::GrindGeometryPlugin,
        camera::CameraPlugin,
        crate::graphics_menu::GraphicsMenuPlugin,
        crate::map_transition::MapTransitionPlugin,
        crate::render_capacity::RenderCapacityPlugin,
        verification::VerificationPlugin,
        crate::performance::PerformancePlugin,
    ));
    app.add_plugins((crate::session_marker::SessionMarkerPlugin, crate::customiser::CustomiserPlugin));
    app.add_plugins(crate::custom_models::CustomModelsPlugin);
    app.add_plugins(crate::trigger_volumes::TriggerVolumesPlugin);
    app.add_plugins(crate::career_stats_host::CareerStatsPlugin);
    app.add_plugins(crate::retail_menus::RetailMenusPlugin);
    app.add_plugins(crate::modding::ModdingPlugin);
    crate::teleport_menu::install(&mut app);
    crate::teleporters::install(&mut app);
    crate::visual_indicators::install(&mut app);
    app.add_plugins(crate::updater::UpdaterPlugin);
    app.add_plugins(crate::multiplayer::MultiplayerPlugin);
    app.add_plugins(crate::scoring_hud::ScoringHudPlugin);
    app.add_plugins(crate::retail_menu_stage::RetailMenuStagePlugin);
    app.add_plugins(crate::fe_backdrop::FeBackdropPlugin);
    app.add_plugins(crate::trick_guide_scene::TrickGuideScenePlugin);
    app.add_plugins(crate::marquee_preview::MarqueePreviewPlugin);
    app.add_plugins(crate::rumble::RumblePlugin);
    app.add_plugins(crate::startup_loading::StartupLoadingPlugin);
    app.add_plugins(crate::asset_check::AssetCheckPlugin);
    app.add_plugins(crate::debug_cam::DebugCamPlugin);
    app.add_plugins(crate::water_splash::WaterSplashPlugin);
    app.add_plugins(crate::ui_audio::UiAudioPlugin);
    app.add_plugins(crate::game_audio::GameAudioPlugin);
    app.add_plugins(crate::living_world::LivingWorldPlugin);
    app.add_systems(Last, crate::crash_context::sample);
    app.add_systems(Startup, warn_integrated_gpu);
    if crate::trace_all::on() || std::env::var_os("SKATE_GPU_TIMING").is_some_and(|v| v != "0") {
        app.add_systems(Startup, log_gpu_timing);
    }
    crate::profiling::install(&mut app);
    app
}
