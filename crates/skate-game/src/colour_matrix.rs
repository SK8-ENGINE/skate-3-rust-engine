//! Retail post FX colour matrix records from the stock collections (class `post_fx`; research b100): the base
//! `colour_matrix` (identity) and `colour_matrix_zombiemode` (the zombie cheat's tint, selected by `827F4D28` /
//! `827F5450` while the zombie query is true). The matrices are built by `skate_core::colour_matrix`.

use bevy::prelude::*;
use skate_core::colour_matrix::{ColourBand, ColourMatrixRecord};
use skate_data::collections::Collections;

/// What the retail tone pass applies (`postfx_visualfxPS`, research b101): near rows (c7..c9), far rows (c10..c12),
/// the depth weights `(mult.x, mult.y, add.x, add.y)` (c13 / c14: `wA = sat(z * mult.x + add.x)`, `wB = sat(z *
/// mult.y + add.y)`, linear in view depth between the record's distances, `827F3D20`) and `(camera near, 0, 0, 0)`.
#[derive(Resource, Clone, Copy, Debug, PartialEq, bevy::render::extract_resource::ExtractResource)]
pub(crate) struct ColourGrade {
    pub rows: [Vec4; 8],
}

impl ColourGrade {
    /// The matrices and depth weights of a record (identity record: the picture is unchanged).
    pub fn of(record: &ColourMatrixRecord, camera_near: f32) -> Self {
        let near = skate_core::colour_matrix::build(&record.near);
        let far = skate_core::colour_matrix::build(&record.far);
        let d = (record.distance_far - record.distance_near).max(1e-3);
        let row = |r: [f32; 4]| Vec4::from_array(r);
        Self {
            rows: [
                row(near[0]),
                row(near[1]),
                row(near[2]),
                row(far[0]),
                row(far[1]),
                row(far[2]),
                Vec4::new(-1.0 / d, 1.0 / d, record.distance_far / d, -record.distance_near / d),
                Vec4::new(camera_near, 0.0, 0.0, 0.0),
            ],
        }
    }
}

impl Default for ColourGrade {
    fn default() -> Self {
        Self::of(&ColourMatrixRecord::default(), 0.1)
    }
}

/// The stock records (loaded once; a missing collection keeps the identity).
#[derive(Resource, Default)]
pub(crate) struct ColourRecords {
    pub base: ColourMatrixRecord,
    pub zombie: Option<ColourMatrixRecord>,
}

pub(crate) fn load_records(config: Option<Res<crate::config::Config>>, mut records: ResMut<ColourRecords>) {
    let Some(config) = config else { return };
    match Collections::load(&config.asset_root) {
        Ok(c) => {
            records.base = load(&c, BASE).unwrap_or_default();
            records.zombie = load(&c, ZOMBIE).map_err(|e| warn!("COLOUR_MATRIX {ZOMBIE}: {e}")).ok();
        }
        Err(e) => warn!("COLOUR_MATRIX: stock collections: {e}"),
    }
}

/// The active record (`827F4D28` sets the zombie bit while the zombie query is true; `827F5450` then builds the
/// zombie matrices, a full replace with no fade) and the gameplay camera's near plane.
pub(crate) fn update_grade(
    records: Res<ColourRecords>,
    settings: Option<Res<crate::living_world::LivingWorldSettings>>,
    cameras: Query<&Projection, With<crate::camera::GameplayCamera>>,
    mut grade: ResMut<ColourGrade>,
) {
    let zombie = settings.is_some_and(|s| s.zombie);
    let record = if zombie { records.zombie.as_ref().unwrap_or(&records.base) } else { &records.base };
    let near = cameras
        .iter()
        .find_map(|p| match p {
            Projection::Perspective(p) => Some(p.near),
            _ => None,
        })
        .unwrap_or(0.1);
    let next = ColourGrade::of(record, near);
    if *grade != next {
        *grade = next;
    }
}

pub(crate) const CLASS: &str = "post_fx";
pub(crate) const BASE: &str = "colour_matrix";
pub(crate) const ZOMBIE: &str = "colour_matrix_zombiemode";

fn vec3(c: &Collections, record: &str, name: &str) -> Result<[f32; 3], String> {
    let w = c.words::<3>(CLASS, record, name)?;
    let v = [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2])];
    if v.iter().all(|x| x.is_finite()) { Ok(v) } else { Err(format!("Non-finite {CLASS}/{record}/{name}")) }
}

fn band(c: &Collections, record: &str, side: &str) -> Result<ColourBand, String> {
    let f = |n: &str| c.float(CLASS, record, &format!("{side}_{n}"));
    Ok(ColourBand {
        multiply: vec3(c, record, &format!("{side}_multiply"))?,
        add: vec3(c, record, &format!("{side}_add"))?,
        contrast: f("contrast")?,
        contrast_midpoint: f("contrast_midpoint")?,
        saturation: f("saturation")?,
    })
}

/// One record (parents included).
pub(crate) fn load(c: &Collections, record: &str) -> Result<ColourMatrixRecord, String> {
    Ok(ColourMatrixRecord {
        near: band(c, record, "near")?,
        far: band(c, record, "far")?,
        distance_near: c.float(CLASS, record, "distance_near")?,
        distance_far: c.float(CLASS, record, "distance_far")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grade the tone pass gets: the near / far weights are linear in view depth between the record's distances
    /// (full near up to 50 m, full far from 100 m, summing to 1); the base record leaves every colour unchanged.
    #[test]
    fn the_grade_mixes_near_and_far_linearly_in_depth() {
        let g = ColourGrade::of(&ColourMatrixRecord::default(), 0.25);
        let w = |z: f32| ((z * g.rows[6].x + g.rows[6].z).clamp(0.0, 1.0), (z * g.rows[6].y + g.rows[6].w).clamp(0.0, 1.0));
        assert_eq!(w(10.0), (1.0, 0.0));
        assert_eq!(w(75.0), (0.5, 0.5));
        assert_eq!(w(500.0), (0.0, 1.0));
        assert_eq!(g.rows[7].x, 0.25);
        let apply = |base: usize, c: Vec3| Vec3::new(g.rows[base].dot(c.extend(1.0)), g.rows[base + 1].dot(c.extend(1.0)), g.rows[base + 2].dot(c.extend(1.0)));
        let c = Vec3::new(0.3, 0.6, 0.9);
        assert!((apply(0, c) - c).length() < 1e-6 && (apply(3, c) - c).length() < 1e-6, "identity");
    }

    /// Data-gated: the base record is the identity and the zombie record holds the values b100 read.
    #[test]
    fn stock_colour_matrix_records_load() {
        let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from) else {
            eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
            return;
        };
        let c = Collections::load(&root).expect("stock collections");
        let base = load(&c, BASE).expect("base record");
        assert_eq!(base, ColourMatrixRecord::default());
        let z = load(&c, ZOMBIE).expect("zombie record");
        eprintln!("{z:?}");
        assert_eq!((z.near.multiply, z.near.saturation, z.near.contrast), ([2.0, 1.4, 1.0], 0.5, 1.3));
        assert_eq!((z.far.contrast, z.far.add, z.distance_near, z.distance_far), (1.5, [-0.4; 3], 50.0, 100.0));
    }
}
