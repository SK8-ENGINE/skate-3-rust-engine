//! Retail ped body material (`livingworld_stamp_defaultPS`) as one Bevy material.
//! Shader rules, rows and mod reach: docs/hails-additions/26f-living-world-peds.md#ped-material-the-retail-shadow-side-2026-10-25-user-asks-352

use bevy::{
    asset::embedded_asset,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

/// Retail material types that run the stamp program (the export's `extras.shader`).
const PROGRAM_SHADERS: [&str; 2] = ["pedestrian_high_stamp", "pedestrian_low"];

#[derive(Clone, Debug, Default, PartialEq, ShaderType)]
pub(crate) struct PedParams {
    pub light: Vec4,
    /// Normal map present, `i_params.x` (fade alpha).
    pub options: Vec4,
    /// Ambient literal (rgb).
    pub literals: Vec4,
    pub rows: [Vec4; 9],
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct PedMaterial {
    #[uniform(0)]
    pub params: PedParams,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub normal: Option<Handle<Image>>,
    pub alpha: AlphaMode,
}

impl Material for PedMaterial {
    fn fragment_shader() -> ShaderRef {
        bevy::asset::AssetPath::from(bevy::asset::embedded_path!("ped_material.wgsl")).with_source("embedded").into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
}

/// Whether a ped GLB material runs the stamp program: its retail material type, opaque (masked hair and accessories
/// keep their glTF material until their programs are ported).
pub(crate) fn runs_program(extras: Option<&serde_json::Value>, alpha: AlphaMode) -> bool {
    alpha == AlphaMode::Opaque
        && extras.and_then(|v| v.get("shader")).and_then(|s| s.as_str()).is_some_and(|s| PROGRAM_SHADERS.contains(&s))
}

/// The `m_params` rows of one ped GLB material: a mod's rows for its `attrib_material` name win over the rows the
/// export wrote into the material's extras.
pub(crate) fn material_rows(extras: Option<&serde_json::Value>, overrides: &std::collections::BTreeMap<String, Vec<[f32; 4]>>) -> Option<Vec<[f32; 4]>> {
    let extras = extras?;
    let named = extras.get("attrib_material").and_then(|a| a.as_str()).and_then(|n| overrides.get(n)).cloned();
    let rows = named.or_else(|| serde_json::from_value::<Vec<[f32; 4]>>(extras.get("params")?.clone()).ok())?;
    (!rows.is_empty()).then_some(rows)
}

/// The uniform of one ped material: `rows` from the material's vault row, `light` from the character lighting.
pub(crate) fn params(rows: &[[f32; 4]], light: Vec4, normal_map: bool) -> PedParams {
    PedParams {
        light,
        options: Vec4::new(f32::from(normal_map), 1.0, 0.0, 0.0),
        literals: Vec3::from_array(super::vehicle_material::AMBIENT).extend(0.0),
        rows: std::array::from_fn(|i| rows.get(i).map_or(Vec4::ZERO, |r| Vec4::from_array(*r))),
    }
}

/// The ped material (installed with the retail render plugins, next to the character lighting whose WGSL module it
/// imports).
pub(crate) struct PedMaterialPlugin;
impl Plugin for PedMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "ped_material.wgsl");
        app.add_plugins(MaterialPlugin::<PedMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// b614: the stamp row's ambient scale 0.275 times the shader literal is the whole shadow side; the export's rows
    /// ride in the material extras and a mod's rows for the same name win.
    #[test]
    fn stamp_rows_give_the_retail_shadow_side_and_mods_override_them() {
        let extras = serde_json::json!({"shader": "pedestrian_high_stamp", "attrib_material": "character.livingworld_pedestrians_stamp",
            "params": [[0.05, 1.0, 0.0, 3.0], [1.5, 1.4, 1.2, 1.6], [0.0, 0.0, 0.0, 9.0], [0.005, 0.005, 0.005, 1.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.275], [0.0, 0.0, 0.0, 0.3]]});
        assert!(runs_program(Some(&extras), AlphaMode::Opaque));
        assert!(!runs_program(Some(&extras), AlphaMode::Mask(0.5)), "masked parts keep their glTF material");
        assert!(!runs_program(Some(&serde_json::json!({"shader": "default_hair"})), AlphaMode::Opaque));
        let mut overrides = std::collections::BTreeMap::new();
        let rows = material_rows(Some(&extras), &overrides).unwrap();
        let p = params(&rows, Vec4::new(0.0, 1.0, 0.0, 1.0), true);
        let ambient = p.literals.truncate() * p.rows[5].w;
        assert!((ambient - Vec3::new(0.0275, 0.048125, 0.0825)).length() < 1e-6, "{ambient}");
        assert_eq!(p.rows[1].truncate(), Vec3::new(1.5, 1.4, 1.2));
        assert_eq!(p.options.y, 1.0, "drawn solid until the fade sets i_params.x");
        overrides.insert("character.livingworld_pedestrians_stamp".to_string(), vec![[0.0, 2.0, 0.0, 0.0]]);
        assert_eq!(material_rows(Some(&extras), &overrides).unwrap(), vec![[0.0, 2.0, 0.0, 0.0]]);
        assert!(material_rows(Some(&serde_json::json!({"shader": "pedestrian_high_stamp"})), &Default::default()).is_none());
    }
}
