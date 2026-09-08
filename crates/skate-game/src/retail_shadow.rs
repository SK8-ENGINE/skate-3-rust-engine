//! Shared, in-place shadow state: moving probes must not rebuild world materials.
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::BufferUsages,
        renderer::RenderQueue,
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
    },
};

pub(super) const BUFFER: Handle<ShaderStorageBuffer> =
    bevy::asset::uuid_handle!("cd736f89-4882-4a5d-8fcb-32273beeaaf4");

#[derive(Resource, Clone, Default, ExtractResource)]
pub(crate) struct ShadowState(pub Vec4);

impl ShadowState {
    pub(crate) fn approach(&mut self, target: Vec3, dt: f32) {
        let target = target.clamp(Vec3::ZERO, Vec3::ONE);
        let value = if self.0.w == 0. {
            target
        } else {
            // Adapter smoothing, not a recovered native constant. Cap a hitch's
            // contribution so one long frame cannot cause a darkness step.
            self.0
                .truncate()
                .lerp(target, 1. - (-dt.clamp(0., 0.05) / 0.35).exp())
        };
        self.0 = value.extend(1.);
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ShadowState>()
        .add_plugins(ExtractResourcePlugin::<ShadowState>::default())
        .add_systems(Startup, initialize);
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, upload.in_set(RenderSystems::PrepareResources));
    }
}

fn initialize(mut buffers: ResMut<Assets<ShaderStorageBuffer>>) {
    let mut buffer = ShaderStorageBuffer::from(Vec4::ZERO);
    buffer.buffer_description.usage |= BufferUsages::COPY_DST;
    buffers
        .insert(BUFFER.id(), buffer)
        .expect("reserved shadow state asset");
}

fn upload(
    state: Res<ShadowState>,
    buffers: Res<RenderAssets<GpuShaderStorageBuffer>>,
    queue: Res<RenderQueue>,
) {
    if let Some(buffer) = buffers.get(BUFFER.id()) {
        let bytes: Vec<u8> = state
            .0
            .to_array()
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        // Keep the buffer and every material bind group alive; only 16 bytes change.
        queue.write_buffer(&buffer.buffer, 0, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_transition_is_gradual_and_hitches_are_bounded() {
        let mut state = ShadowState::default();
        state.approach(Vec3::splat(0.1), 0.016);
        assert_eq!(state.0, Vec3::splat(0.1).extend(1.));
        state.approach(Vec3::splat(0.3), 0.016);
        assert!(state.0.x > 0.1 && state.0.x < 0.12);
        let mut hitch = state.clone();
        state.approach(Vec3::splat(0.3), 0.05);
        hitch.approach(Vec3::splat(0.3), 10.);
        assert_eq!(state.0, hitch.0);
        for _ in 0..240 {
            state.approach(Vec3::splat(0.3), 1. / 60.);
        }
        assert!((state.0.x - 0.3).abs() < 0.0001);
    }
}
