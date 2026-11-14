//! Retail car materials (`vehicle_defaultPS`, `vehicle_glass_defaultPS`) as one Bevy material.
//! Shader rules, engine lighting and mod rows: docs/hails-additions/26h-living-world-traffic.md#car-materials

use bevy::{
    asset::embedded_asset,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

/// The ambient literal of both car pixel shaders (scaled by `m_params[5].w`) [code, shader].
pub(crate) const AMBIENT: [f32; 3] = [0.1, 0.175, 0.3];

#[derive(Clone, Debug, Default, PartialEq, ShaderType)]
pub(crate) struct VehicleParams {
    pub light: Vec4,
    pub red: Vec4,
    pub blue: Vec4,
    /// Glass program, normal map present, `i_params.x`, environment cube present.
    pub options: Vec4,
    /// Ambient literal (rgb), mask literal (w).
    pub literals: Vec4,
    pub rows: [Vec4; 9],
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct VehicleMaterial {
    #[uniform(0)]
    pub params: VehicleParams,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub normal: Option<Handle<Image>>,
    #[texture(5, dimension = "cube")]
    #[sampler(6)]
    pub environment: Option<Handle<Image>>,
    pub alpha: AlphaMode,
}

impl Material for VehicleMaterial {
    fn fragment_shader() -> ShaderRef {
        bevy::asset::AssetPath::from(bevy::asset::embedded_path!("vehicle_material.wgsl")).with_source("embedded").into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
}

/// Which program a car material runs, from its export kind (`extras.kind`, the GLB material name
/// prefix): `vehicle_glass` -> glass, `vehicle_chassis` -> chassis, anything else keeps its glTF
/// material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Program {
    Chassis,
    Glass,
}

impl Program {
    pub(crate) fn of(kind: &str) -> Option<Self> {
        if kind.starts_with("vehicle_glass") {
            Some(Self::Glass)
        } else if kind.starts_with("vehicle_chassis") {
            Some(Self::Chassis)
        } else {
            None
        }
    }
    /// The export's material kind of this program.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Chassis => "vehicle_chassis",
            Self::Glass => "vehicle_glass",
        }
    }
}

/// The uniform of one car material: `rows` from the material's vault row, the palette pair
/// scaled by `gain`, `light` from the character lighting.
pub(crate) fn params(program: Program, rows: &[[f32; 4]], chassis: [f32; 4], secondary: [f32; 4], gain: f32, light: Vec4, normal_map: bool, environment: bool) -> VehicleParams {
    let scale = |c: [f32; 4]| Vec4::new(c[0] * gain, c[1] * gain, c[2] * gain, c[3]);
    VehicleParams {
        light,
        red: scale(secondary),
        blue: scale(chassis),
        options: Vec4::new(f32::from(program == Program::Glass), f32::from(normal_map), 1.0, f32::from(environment)),
        literals: Vec3::from_array(AMBIENT).extend(skate_core::living_world::peds::colorize::MASK_GREEN_SQ_MAX),
        rows: std::array::from_fn(|i| rows.get(i).map_or(Vec4::ZERO, |r| Vec4::from_array(*r))),
    }
}

/// Retail render state per program: the body is opaque, the glass alpha-blended without depth
/// write (`sub_82CF0B90`: blend[8], depth[14]).
pub(crate) fn alpha_mode(program: Program) -> AlphaMode {
    match program {
        Program::Chassis => AlphaMode::Opaque,
        Program::Glass => AlphaMode::Blend,
    }
}

/// The car material (installed with the retail render plugins, next to the character lighting
/// whose WGSL module it imports).
pub(crate) struct VehicleMaterialPlugin;
impl Plugin for VehicleMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "vehicle_material.wgsl");
        app.add_plugins(MaterialPlugin::<VehicleMaterial>::default());
    }
}
