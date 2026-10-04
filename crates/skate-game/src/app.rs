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
        settings::{
            Backends, InstanceFlags, MemoryHints, RenderCreation, WgpuFeatures, WgpuSettings,
        },
        RenderPlugin,
    },
};
use skate_data::GameAssets;

/// Window title: engine name, selected graphics backend and — once the session
/// server has assigned an actor — the client number. Two local instances are
/// otherwise indistinguishable in the taskbar and on alt-tab.
fn window_title(config: &Config, actor: Option<u64>) -> String {
    let backend = gpu_backend_name(config.gpu_backend.as_deref());
    let base = config
        .multiplayer
        .title
        .clone()
        .unwrap_or_else(|| "Skate 3 Rust Engine".into());
    match actor {
        Some(id) if id != 0 => format!("{base} — {backend} — cliente #{id}"),
        _ => format!("{base} — {backend} — cliente ..."),
    }
}

/// The engine renders only with Vulkan. Kept as a named function so the startup
/// report and window title stay explicit about the active backend.
pub(crate) fn gpu_backend_name(_choice: Option<&str>) -> &'static str {
    "Vulkan"
}

/// Refreshes the window title when the session identity is first assigned.
/// Cheap enough to run every frame: it only writes when the text changes.
fn update_window_title(
    config: Res<Config>,
    net: Res<crate::multiplayer::Multiplayer>,
    windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut last: Local<Option<u64>>,
) {
    let actor = net.local_actor();
    if *last == Some(actor) {
        return;
    }
    *last = Some(actor);
    let title = window_title(&config, Some(actor));
    for mut window in windows {
        if window.title != title {
            window.title = title.clone();
        }
    }
}

/// Vulkan is the only supported backend. It renders independent processes
/// safely, so local clients share it through `--multi-instance` rather than
/// switching to a different graphics API.
fn gpu_backends() -> Backends {
    Backends::VULKAN
}

/// Bevy's defaults plus, on request, the query features that make
/// `RenderDiagnosticsPlugin` report per-pass GPU time.
///
/// Opt-in: a required feature the adapter lacks aborts device creation, and the
/// queries are not free. Without them a pass's cost can only be inferred from
/// invocation counts, which says nothing about how long the pass took. Set
/// `SKATE_GPU_TIMING=1` to get `render/**/elapsed_gpu`.
fn wgpu_features() -> WgpuFeatures {
    let default = WgpuSettings::default().features;
    if std::env::var_os("SKATE_GPU_TIMING").is_some_and(|v| v != "0") {
        default
            | WgpuFeatures::TIMESTAMP_QUERY
            | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | WgpuFeatures::PIPELINE_STATISTICS_QUERY
    } else {
        default
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
    let retail_scene = config
        .map
        .as_ref()
        .is_some_and(|map| crate::retail_render::RetailScene::for_map(map));
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
                    title: window_title(&config, None),
                    resolution: (1280, 800).into(),
                    desired_maximum_frame_latency: config
                        .multi_instance
                        .then(|| std::num::NonZeroU32::new(1).unwrap()),
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    backends: Some(gpu_backends()),
                    // Existing machine's validation layer rejects wgpu atomic shaders.
                    // This workaround belongs only to the rendering adapter.
                    instance_flags: InstanceFlags::empty(),
                    features: wgpu_features(),
                    memory_hints: if config.multi_instance {
                        MemoryHints::MemoryUsage
                    } else {
                        MemoryHints::default()
                    },
                    ..default()
                }),
                // Avoid parallel driver compilation bursts from several clients.
                synchronous_pipeline_compilation: config.multi_instance,
                ..default()
            })
            .build()
            .disable::<bevy::log::LogPlugin>()
            // Gameplay and menu navigation both use raw XInput. No game system
            // consumes Bevy gamepad events/rumble; its second device backend can
            // stall PreUpdate (70.68 ms in the University capture).
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .insert_resource(bevy::winit::WinitSettings {
        focused_mode: bevy::winit::UpdateMode::Continuous,
        unfocused_mode: bevy::winit::UpdateMode::Continuous,
    })
    .insert_resource(config)
    .insert_resource(crate::retail_render::RetailScene(retail_scene))
    .insert_resource(assets::AssetManifest(manifest))
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
    .add_plugins(crate::fps_overlay::FpsOverlayPlugin)
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
    app.add_plugins((
        crate::session_marker::SessionMarkerPlugin,
        crate::customiser::CustomiserPlugin,
    ));
    app.add_plugins(crate::custom_models::CustomModelsPlugin);
    app.add_plugins(crate::modding::ModdingPlugin);
    crate::teleport_menu::install(&mut app);
    app.add_plugins(crate::updater::UpdaterPlugin);
    app.add_plugins(crate::multiplayer::MultiplayerPlugin);
    app.add_plugins(crate::scoring_hud::ScoringHudPlugin);
    app.add_plugins(crate::debug_cam::DebugCamPlugin);
    app.add_systems(Last, (crate::crash_context::sample, update_window_title));
    crate::profiling::install(&mut app);
    app
}
