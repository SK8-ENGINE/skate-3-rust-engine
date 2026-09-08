//! Native noise pass, composited before the original HUD.
use super::noise::Noise;
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::visibility::RenderLayers,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};

#[derive(Clone, Debug, ShaderType)]
struct Params {
    scroll: Vec4,
    first: Vec4,
    second: Vec4,
    fade: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
struct Effect {
    #[uniform(0)]
    params: Params,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
}
impl Material2d for Effect {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/session_marker/effect.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}
#[derive(Resource)]
struct Runtime {
    noise: Noise,
    material: Handle<Effect>,
    time: f64,
}
#[derive(Component)]
struct Screen;

pub(super) fn install(app: &mut App) {
    embedded_asset!(app, "effect.wgsl");
    app.add_plugins(Material2dPlugin::<Effect>::default())
        .add_systems(Startup, load)
        .add_systems(Update, present);
}
fn load(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<Effect>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mut noise = Noise::default();
    let mut image = Image::new(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        noise.texture(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    let material = materials.add(Effect {
        texture: images.add(image),
        params: Params {
            scroll: Vec4::ZERO,
            first: Vec4::X,
            second: Vec4::X,
            fade: Vec4::ZERO,
        },
    });
    commands.spawn((
        Screen,
        Mesh2d(meshes.add(Rectangle::new(1., 1.))),
        MeshMaterial2d(material.clone()),
        Transform::from_xyz(0., 0., -1.),
        Visibility::Hidden,
        RenderLayers::layer(29),
    ));
    commands.insert_resource(Runtime {
        noise,
        material,
        time: 0.,
    });
}
fn present(
    mut runtime: ResMut<Runtime>,
    session: Res<super::SessionMarker>,
    time: Res<Time<Real>>,
    window: Single<&Window>,
    mut materials: ResMut<Assets<Effect>>,
    mut screen: Query<(&mut Transform, &mut Visibility), With<Screen>>,
) {
    runtime.time += time.delta_secs_f64();
    while runtime.time >= 1. / 60. {
        runtime.time -= 1. / 60.;
        runtime.noise.advance();
    }
    if let Some(material) = materials.get_mut(&runtime.material) {
        material.params = Params {
            scroll: Vec4::from_array(runtime.noise.scroll),
            first: Vec4::from_array(Noise::weights(runtime.noise.phases[0])),
            second: Vec4::from_array(Noise::weights(runtime.noise.phases[1])),
            fade: Vec4::new(session.progress, 0., 0., 0.),
        };
    }
    for (mut transform, mut visibility) in &mut screen {
        transform.scale = Vec3::new(window.width(), window.height(), 1.);
        *visibility = if session.progress > 0. {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
