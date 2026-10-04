use crate::renderer::WgpuWrapper;
use crate::{
    render_resource::{SurfaceTexture, TextureView},
    renderer::{RenderAdapter, RenderDevice, RenderInstance},
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
};
use bevy_app::{App, Plugin};
use bevy_ecs::{entity::EntityHashMap, prelude::*};
use bevy_platform::collections::HashSet;
use bevy_utils::default;
use bevy_window::{
    CompositeAlphaMode, PresentMode, PrimaryWindow, RawHandleWrapper, Window, WindowClosing,
};
use core::{
    num::NonZero,
    ops::{Deref, DerefMut},
};
use tracing::{debug, info, warn};
use wgpu::{
    SurfaceConfiguration, SurfaceTargetUnsafe, TextureFormat, TextureUsages, TextureViewDescriptor,
};

pub mod screenshot;

use screenshot::ScreenshotPlugin;

pub struct WindowRenderPlugin;

impl Plugin for WindowRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ScreenshotPlugin);

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<ExtractedWindows>()
                .init_resource::<WindowSurfaces>()
                .add_systems(ExtractSchedule, extract_windows)
                .add_systems(
                    Render,
                    create_surfaces
                        .run_if(need_surface_configuration)
                        .before(prepare_windows),
                )
                .add_systems(Render, prepare_windows.in_set(RenderSystems::ManageViews));
        }
    }
}

pub struct ExtractedWindow {
    /// An entity that contains the components in [`Window`].
    pub entity: Entity,
    pub handle: RawHandleWrapper,
    pub physical_width: u32,
    pub physical_height: u32,
    pub present_mode: PresentMode,
    pub desired_maximum_frame_latency: Option<NonZero<u32>>,
    /// Note: this will not always be the swap chain texture view. When taking a screenshot,
    /// this will point to an alternative texture instead to allow for copying the render result
    /// to CPU memory.
    pub swap_chain_texture_view: Option<TextureView>,
    pub swap_chain_texture: Option<SurfaceTexture>,
    pub swap_chain_texture_format: Option<TextureFormat>,
    pub swap_chain_texture_view_format: Option<TextureFormat>,
    pub size_changed: bool,
    pub present_mode_changed: bool,
    pub alpha_mode: CompositeAlphaMode,
    /// Whether this window needs an initial buffer commit.
    ///
    /// On Wayland, windows must present at least once before they are shown.
    /// See <https://wayland.app/protocols/xdg-shell#xdg_surface>
    pub needs_initial_present: bool,
}

impl ExtractedWindow {
    fn set_swapchain_texture(&mut self, frame: wgpu::SurfaceTexture) {
        self.swap_chain_texture_view_format = Some(frame.texture.format().add_srgb_suffix());
        let texture_view_descriptor = TextureViewDescriptor {
            format: self.swap_chain_texture_view_format,
            ..default()
        };
        self.swap_chain_texture_view = Some(TextureView::from(
            frame.texture.create_view(&texture_view_descriptor),
        ));
        self.swap_chain_texture = Some(SurfaceTexture::from(frame));
    }

    fn has_swapchain_texture(&self) -> bool {
        self.swap_chain_texture_view.is_some() && self.swap_chain_texture.is_some()
    }

    pub fn present(&mut self) {
        if let Some(surface_texture) = self.swap_chain_texture.take() {
            // TODO(clean): winit docs recommends calling pre_present_notify before this.
            // though `present()` doesn't present the frame, it schedules it to be presented
            // by wgpu.
            // https://docs.rs/winit/0.29.9/wasm32-unknown-unknown/winit/window/struct.Window.html#method.pre_present_notify
            surface_texture.present();
        }
    }
}

#[derive(Default, Resource)]
pub struct ExtractedWindows {
    pub primary: Option<Entity>,
    pub windows: EntityHashMap<ExtractedWindow>,
}

impl Deref for ExtractedWindows {
    type Target = EntityHashMap<ExtractedWindow>;

    fn deref(&self) -> &Self::Target {
        &self.windows
    }
}

impl DerefMut for ExtractedWindows {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.windows
    }
}

fn extract_windows(
    mut extracted_windows: ResMut<ExtractedWindows>,
    mut closing: Extract<MessageReader<WindowClosing>>,
    windows: Extract<Query<(Entity, &Window, &RawHandleWrapper, Option<&PrimaryWindow>)>>,
    mut removed: Extract<RemovedComponents<RawHandleWrapper>>,
    mut window_surfaces: ResMut<WindowSurfaces>,
) {
    for (entity, window, handle, primary) in windows.iter() {
        if primary.is_some() {
            extracted_windows.primary = Some(entity);
        }

        let (new_width, new_height) = (
            window.resolution.physical_width().max(1),
            window.resolution.physical_height().max(1),
        );

        let extracted_window = extracted_windows.entry(entity).or_insert(ExtractedWindow {
            entity,
            handle: handle.clone(),
            physical_width: new_width,
            physical_height: new_height,
            present_mode: window.present_mode,
            desired_maximum_frame_latency: window.desired_maximum_frame_latency,
            swap_chain_texture: None,
            swap_chain_texture_view: None,
            size_changed: false,
            swap_chain_texture_format: None,
            swap_chain_texture_view_format: None,
            present_mode_changed: false,
            alpha_mode: window.composite_alpha_mode,
            needs_initial_present: true,
        });

        if extracted_window.swap_chain_texture.is_none() {
            // If we called present on the previous swap-chain texture last update,
            // then drop the swap chain frame here, otherwise we can keep it for the
            // next update as an optimization. `prepare_windows` will only acquire a new
            // swap chain texture if needed.
            extracted_window.swap_chain_texture_view = None;
        }
        extracted_window.size_changed = new_width != extracted_window.physical_width
            || new_height != extracted_window.physical_height;
        extracted_window.present_mode_changed =
            window.present_mode != extracted_window.present_mode;

        if extracted_window.size_changed {
            debug!(
                "Window size changed from {}x{} to {}x{}",
                extracted_window.physical_width,
                extracted_window.physical_height,
                new_width,
                new_height
            );
            extracted_window.physical_width = new_width;
            extracted_window.physical_height = new_height;
        }

        if extracted_window.present_mode_changed {
            debug!(
                "Window Present Mode changed from {:?} to {:?}",
                extracted_window.present_mode, window.present_mode
            );
            extracted_window.present_mode = window.present_mode;
        }
    }

    for closing_window in closing.read() {
        extracted_windows.remove(&closing_window.window);
        window_surfaces.remove(&closing_window.window);
    }
    for removed_window in removed.read() {
        extracted_windows.remove(&removed_window);
        window_surfaces.remove(&removed_window);
    }
}

struct SurfaceData {
    // TODO: what lifetime should this be?
    surface: WgpuWrapper<wgpu::Surface<'static>>,
    configuration: SurfaceConfiguration,
    texture_view_format: Option<TextureFormat>,
    recovery: SurfaceRecovery,
}

/// Do not turn a temporary acquisition failure into a process-wide panic.
/// Generic errors can also indicate device loss, so retries must be bounded.
#[derive(Default)]
struct SurfaceRecovery {
    skipped_frames: u32,
    consecutive_generic_errors: u32,
}

impl SurfaceRecovery {
    fn skip_frame(&mut self, error: &wgpu::SurfaceError) -> bool {
        self.skipped_frames = self.skipped_frames.saturating_add(1);
        self.consecutive_generic_errors = if matches!(error, wgpu::SurfaceError::Other) {
            self.consecutive_generic_errors + 1
        } else {
            0
        };
        !matches!(error, wgpu::SurfaceError::OutOfMemory)
            && self.consecutive_generic_errors < 8
    }
}

/// Retry once after rebuilding the swapchain. Never retry memory exhaustion,
/// and never reconfigure merely because another process is keeping the GPU busy.
fn acquire_surface_texture<T>(
    mut acquire: impl FnMut() -> Result<T, wgpu::SurfaceError>,
    mut reconfigure: impl FnMut(),
) -> Result<T, wgpu::SurfaceError> {
    match acquire() {
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Other) => {
            reconfigure();
            acquire()
        }
        result => result,
    }
}

#[derive(Resource, Default)]
pub struct WindowSurfaces {
    surfaces: EntityHashMap<SurfaceData>,
    /// List of windows that we have already called the initial `configure_surface` for
    configured_windows: HashSet<Entity>,
}

impl WindowSurfaces {
    fn remove(&mut self, window: &Entity) {
        self.surfaces.remove(window);
        self.configured_windows.remove(window);
    }
}

/// (re)configures window surfaces, and obtains a swapchain texture for rendering.
///
/// NOTE: `get_current_texture` in `prepare_windows` can take a long time if the GPU workload is
/// the performance bottleneck. This can be seen in profiles as multiple prepare-set systems all
/// taking an unusually long time to complete, and all finishing at about the same time as the
/// `prepare_windows` system. Improvements in bevy are planned to avoid this happening when it
/// should not but it will still happen as it is easy for a user to create a large GPU workload
/// relative to the GPU performance and/or CPU workload.
/// This can be caused by many reasons, but several of them are:
/// - GPU workload is more than your current GPU can manage
/// - Error / performance bug in your custom shaders
/// - wgpu was unable to detect a proper GPU hardware-accelerated device given the chosen
///   [`Backends`](crate::settings::Backends), [`WgpuLimits`](crate::settings::WgpuLimits),
///   and/or [`WgpuFeatures`](crate::settings::WgpuFeatures). For example, on Windows currently
///   `DirectX 11` is not supported by wgpu 0.12 and so if your GPU/drivers do not support Vulkan,
///   it may be that a software renderer called "Microsoft Basic Render Driver" using `DirectX 12`
///   will be chosen and performance will be very poor. This is visible in a log message that is
///   output during renderer initialization.
///   Another alternative is to try to use [`ANGLE`](https://github.com/gfx-rs/wgpu#angle) and
///   [`Backends::GL`](crate::settings::Backends::GL) with the `gles` feature enabled if your
///   GPU/drivers support `OpenGL 4.3` / `OpenGL ES 3.0` or later.
pub fn prepare_windows(
    mut windows: ResMut<ExtractedWindows>,
    mut window_surfaces: ResMut<WindowSurfaces>,
    render_device: Res<RenderDevice>,
) {
    for window in windows.windows.values_mut() {
        let window_surfaces = window_surfaces.deref_mut();
        let Some(surface_data) = window_surfaces.surfaces.get_mut(&window.entity) else {
            continue;
        };

        // We didn't present the previous frame, so we can keep using our existing swapchain texture.
        if window.has_swapchain_texture() && !window.size_changed && !window.present_mode_changed {
            continue;
        }

        // A retained frame/view must not outlive a surface reconfiguration.
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
        window.swap_chain_texture_format = Some(surface_data.configuration.format);
        let surface = &surface_data.surface;
        match acquire_surface_texture(
            || surface.get_current_texture(),
            || render_device.configure_surface(surface, &surface_data.configuration),
        ) {
            Ok(frame) => {
                window.set_swapchain_texture(frame);
                surface_data.recovery = SurfaceRecovery::default();
            }
            Err(err) => {
                if !surface_data.recovery.skip_frame(&err) {
                    panic!("Couldn't recover swap chain texture after bounded retries: {err}");
                }
                // Rate-limit warnings. Physics and networking continue without
                // a window texture; the next render frame attempts acquisition.
                let skipped = surface_data.recovery.skipped_frames;
                if skipped == 1 || skipped.is_multiple_of(60) {
                    warn!("Skipping window frame {skipped}; swapchain acquisition will retry: {err}");
                }
            }
        }
    }
}

pub fn need_surface_configuration(
    windows: Res<ExtractedWindows>,
    window_surfaces: Res<WindowSurfaces>,
) -> bool {
    for window in windows.windows.values() {
        if !window_surfaces.configured_windows.contains(&window.entity)
            || window.size_changed
            || window.present_mode_changed
        {
            return true;
        }
    }
    false
}

// 2 is wgpu's default/what we've been using so far.
// 1 is the minimum, but may cause lower framerates due to the cpu waiting for the gpu to finish
// all work for the previous frame before starting work on the next frame, which then means the gpu
// has to wait for the cpu to finish to start on the next frame.
const DEFAULT_DESIRED_MAXIMUM_FRAME_LATENCY: u32 = 2;

/// Creates window surfaces.
pub fn create_surfaces(
    // By accessing a NonSend resource, we tell the scheduler to put this system on the main thread,
    // which is necessary for some OS's
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy_ecs::system::NonSendMarker,
    mut windows: ResMut<ExtractedWindows>,
    mut window_surfaces: ResMut<WindowSurfaces>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
) {
    for window in windows.windows.values_mut() {
        let data = window_surfaces
            .surfaces
            .entry(window.entity)
            .or_insert_with(|| {
                let surface_target = SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: window.handle.get_display_handle(),
                    raw_window_handle: window.handle.get_window_handle(),
                };
                // SAFETY: The window handles in ExtractedWindows will always be valid objects to create surfaces on
                let surface = unsafe {
                    // NOTE: On some OSes this MUST be called from the main thread.
                    // As of wgpu 0.15, only fallible if the given window is a HTML canvas and obtaining a WebGPU or WebGL2 context fails.
                    render_instance
                        .create_surface_unsafe(surface_target)
                        .expect("Failed to create wgpu surface")
                };
                let caps = surface.get_capabilities(&render_adapter);
                let present_mode = present_mode(window, &caps);
                let formats = caps.formats;
                // For future HDR output support, we'll need to request a format that supports HDR,
                // but as of wgpu 0.15 that is not yet supported.
                // Prefer sRGB formats for surfaces, but fall back to first available format if no sRGB formats are available.
                let mut format = *formats.first().expect("No supported formats for surface");
                for available_format in formats {
                    // Rgba8UnormSrgb and Bgra8UnormSrgb and the only sRGB formats wgpu exposes that we can use for surfaces.
                    if available_format == TextureFormat::Rgba8UnormSrgb
                        || available_format == TextureFormat::Bgra8UnormSrgb
                    {
                        format = available_format;
                        break;
                    }
                }

                let texture_view_format = if !format.is_srgb() {
                    Some(format.add_srgb_suffix())
                } else {
                    None
                };
                let configuration = SurfaceConfiguration {
                    format,
                    width: window.physical_width,
                    height: window.physical_height,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    present_mode,
                    desired_maximum_frame_latency: window
                        .desired_maximum_frame_latency
                        .map(NonZero::<u32>::get)
                        .unwrap_or(DEFAULT_DESIRED_MAXIMUM_FRAME_LATENCY),
                    alpha_mode: match window.alpha_mode {
                        CompositeAlphaMode::Auto => wgpu::CompositeAlphaMode::Auto,
                        CompositeAlphaMode::Opaque => wgpu::CompositeAlphaMode::Opaque,
                        CompositeAlphaMode::PreMultiplied => {
                            wgpu::CompositeAlphaMode::PreMultiplied
                        }
                        CompositeAlphaMode::PostMultiplied => {
                            wgpu::CompositeAlphaMode::PostMultiplied
                        }
                        CompositeAlphaMode::Inherit => wgpu::CompositeAlphaMode::Inherit,
                    },
                    view_formats: match texture_view_format {
                        Some(format) => vec![format],
                        None => vec![],
                    },
                };

                render_device.configure_surface(&surface, &configuration);

                SurfaceData {
                    surface: WgpuWrapper::new(surface),
                    configuration,
                    texture_view_format,
                    recovery: SurfaceRecovery::default(),
                }
            });

        if window.size_changed || window.present_mode_changed {
            // normally this is dropped on present but we double check here to be safe as failure to
            // drop it will cause validation errors in wgpu
            drop(window.swap_chain_texture.take());
            #[cfg_attr(
                target_arch = "wasm32",
                expect(clippy::drop_non_drop, reason = "texture views are not drop on wasm")
            )]
            drop(window.swap_chain_texture_view.take());

            data.configuration.width = window.physical_width;
            data.configuration.height = window.physical_height;
            let caps = data.surface.get_capabilities(&render_adapter);
            data.configuration.present_mode = present_mode(window, &caps);
            render_device.configure_surface(&data.surface, &data.configuration);
        }

        window_surfaces.configured_windows.insert(window.entity);
    }
}

fn present_mode(
    window: &mut ExtractedWindow,
    caps: &wgpu::SurfaceCapabilities,
) -> wgpu::PresentMode {
    let present_mode = match window.present_mode {
        PresentMode::Fifo => wgpu::PresentMode::Fifo,
        PresentMode::FifoRelaxed => wgpu::PresentMode::FifoRelaxed,
        PresentMode::Mailbox => wgpu::PresentMode::Mailbox,
        PresentMode::Immediate => wgpu::PresentMode::Immediate,
        PresentMode::AutoVsync => wgpu::PresentMode::AutoVsync,
        PresentMode::AutoNoVsync => wgpu::PresentMode::AutoNoVsync,
    };
    let fallbacks = match present_mode {
        wgpu::PresentMode::AutoVsync => {
            &[wgpu::PresentMode::FifoRelaxed, wgpu::PresentMode::Fifo][..]
        }
        wgpu::PresentMode::AutoNoVsync => &[
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Fifo,
        ][..],
        wgpu::PresentMode::Mailbox => &[
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ][..],
        // Always end in FIFO to make sure it's always supported
        x => &[x, wgpu::PresentMode::Fifo][..],
    };
    let new_present_mode = fallbacks
        .iter()
        .copied()
        .find(|fallback| caps.present_modes.contains(fallback))
        .unwrap_or_else(|| {
            unreachable!(
                "Fallback system failed to choose present mode. \
                            This is a bug. Mode: {:?}, Options: {:?}",
                window.present_mode, &caps.present_modes
            );
        });
    if new_present_mode != present_mode && fallbacks.contains(&present_mode) {
        info!("PresentMode {present_mode:?} requested but not available. Falling back to {new_present_mode:?}");
    }
    new_present_mode
}

#[cfg(test)]
mod surface_recovery_tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn timeout_skips_without_reconfiguring_or_retrying() {
        let attempts = Cell::new(0);
        let result = acquire_surface_texture::<()>(
            || { attempts.set(attempts.get() + 1); Err(wgpu::SurfaceError::Timeout) },
            || panic!("timeout must not reconfigure"),
        );
        assert_eq!(result, Err(wgpu::SurfaceError::Timeout));
        assert_eq!(attempts.get(), 1);
        let mut recovery = SurfaceRecovery::default();
        for _ in 0..120 { assert!(recovery.skip_frame(&wgpu::SurfaceError::Timeout)); }
    }

    #[test]
    fn lost_outdated_and_generic_errors_reconfigure_then_retry_once() {
        for error in [wgpu::SurfaceError::Lost, wgpu::SurfaceError::Outdated, wgpu::SurfaceError::Other] {
            let attempts = Cell::new(0);
            let configurations = Cell::new(0);
            let result = acquire_surface_texture(
                || { attempts.set(attempts.get() + 1); if attempts.get() == 1 { Err(error.clone()) } else { Ok(42) } },
                || configurations.set(configurations.get() + 1),
            );
            assert_eq!(result, Ok(42));
            assert_eq!(attempts.get(), 2);
            assert_eq!(configurations.get(), 1);
        }
    }

    #[test]
    fn retry_failure_is_returned_and_memory_exhaustion_is_fatal() {
        let attempts = Cell::new(0);
        let result = acquire_surface_texture::<()>(
            || { attempts.set(attempts.get() + 1); Err(wgpu::SurfaceError::Lost) },
            || {},
        );
        assert_eq!(result, Err(wgpu::SurfaceError::Lost));
        assert_eq!(attempts.get(), 2);
        assert_eq!(acquire_surface_texture::<()>(|| Err(wgpu::SurfaceError::OutOfMemory), || panic!("must not retry")), Err(wgpu::SurfaceError::OutOfMemory));
        assert!(!SurfaceRecovery::default().skip_frame(&wgpu::SurfaceError::OutOfMemory));
    }

    #[test]
    fn persistent_generic_errors_are_bounded_and_success_resets_recovery() {
        let mut recovery = SurfaceRecovery::default();
        for _ in 0..7 { assert!(recovery.skip_frame(&wgpu::SurfaceError::Other)); }
        assert!(!recovery.skip_frame(&wgpu::SurfaceError::Other));
        recovery = SurfaceRecovery::default();
        assert!(recovery.skip_frame(&wgpu::SurfaceError::Other));
        assert_eq!(acquire_surface_texture(|| Ok::<_, wgpu::SurfaceError>(42), || panic!("must not reconfigure")), Ok(42));
    }
}
