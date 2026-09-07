//! Authored test-course grind paths. Collision/visuals share these dimensions.
//! This asset owner does not claim a nearby spline is an acquired grind.
use skate_core::math::Vector3;
use bevy::prelude::*;

/// Shared authored geometry. The physical grind owner runs native truck queries
/// against these same spline endpoints before state selection.
#[derive(Resource)]
pub(crate) struct GrindGeometry {
    pub rails: Vec<Rail>,
    pub native_blob: Vec<u8>,
}

pub(crate) struct GrindGeometryPlugin;
impl Plugin for GrindGeometryPlugin {
    fn build(&self, app: &mut App) {
        let test_world = app.world().resource::<crate::config::Config>().map.is_none();
        let geometry = if test_world {
            GrindGeometry { rails: rails().to_vec(), native_blob: spline_blob() }
        } else {
            GrindGeometry { rails: Vec::new(), native_blob: Vec::new() }
        };
        if test_world {
            info!("GRIND_GEOMETRY rails={} bytes={} native_acquisition=paired_trucks_50_50",
                geometry.rails.len(), geometry.native_blob.len());
            for rail in &geometry.rails {
                info!("GRIND_RAIL id={} name={} start={:?} end={:?}",
                    rail.id, rail.name, rail.start, rail.end);
            }
        }
        app.insert_resource(geometry);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rail {
    pub id: u64,
    pub name: &'static str,
    pub start: Vector3,
    pub end: Vector3,
    pub width: f32,
    pub thickness: f32,
}

pub(crate) fn rails() -> [Rail; 7] {
    let floor = crate::physics::ground::FLOOR_HEIGHT;
    let top = crate::physics::ground::HEIGHT;
    let v = Vector3::new;
    [
        Rail { id: 1, name: "low_flat_rail", start: v(-7., floor + 0.45, 7.),
            end: v(-7., floor + 0.45, 15.), width: 0.10, thickness: 0.10 },
        Rail { id: 2, name: "high_flat_rail", start: v(4.5, floor + 0.70, 8.),
            end: v(4.5, floor + 0.70, 16.), width: 0.10, thickness: 0.10 },
        Rail { id: 3, name: "halfpipe_left_coping", start: v(7.5, floor + 2.5, -6.),
            end: v(7.5, floor + 2.5, 6.), width: 0.08, thickness: 0.08 },
        Rail { id: 4, name: "halfpipe_right_coping", start: v(16.5, floor + 2.5, -6.),
            end: v(16.5, floor + 2.5, 6.), width: 0.08, thickness: 0.08 },
        Rail { id: 5, name: "platform_back_edge", start: v(-3., top, -4.),
            end: v(3., top, -4.), width: 0., thickness: 0. },
        Rail { id: 6, name: "platform_right_edge", start: v(3., top, -4.),
            end: v(3., top, 4.), width: 0., thickness: 0. },
        Rail { id: 7, name: "platform_left_edge", start: v(-3., top, 4.),
            end: v(-3., top, -4.), width: 0., thickness: 0. },
    ]
}

/// Box rails have their spline on the top centre, not the centre of the tube.
/// Coping is flush with the deck lip; the existing transition remains intact.
pub(crate) fn surfaces() -> Vec<[Vector3; 4]> {
    let mut out = Vec::new();
    let floor = crate::physics::ground::FLOOR_HEIGHT;
    for rail in rails() {
        // Ledge paths use the already authored platform collision.
        if rail.width == 0. { continue; }
        let x = rail.start.x;
        let y = rail.start.y;
        box_faces(&mut out, Vector3::new(x - rail.width * 0.5, y - rail.thickness, rail.start.z),
            Vector3::new(x + rail.width * 0.5, y, rail.end.z));
        if rail.id <= 2 {
            for z in [rail.start.z + 0.6, rail.end.z - 0.6] {
                box_faces(&mut out, Vector3::new(x - 0.04, floor, z - 0.04),
                    Vector3::new(x + 0.04, y - rail.thickness, z + 0.04));
            }
        }
    }
    out
}

fn box_faces(out: &mut Vec<[Vector3; 4]>, min: Vector3, max: Vector3) {
    let v = Vector3::new;
    let top = [v(min.x, max.y, min.z), v(max.x, max.y, min.z),
        v(max.x, max.y, max.z), v(min.x, max.y, max.z)];
    out.push(top);
    for i in 0..4 {
        let a = top[i]; let b = top[(i + 1) % 4];
        out.push([b, a, v(a.x, min.y, a.z), v(b.x, min.y, b.z)]);
    }
    out.push([v(min.x, min.y, max.z), v(max.x, min.y, max.z),
        v(max.x, min.y, min.z), v(min.x, min.y, min.z)]);
}

/// Relocatable Pegasus tSplineData, matching the project's established native
/// builder (owned/world/src/grind_spline.cpp). One straight segment per rail.
/// Guest pointers stay blob-relative; the Rust owner retains stable rail IDs.
pub(crate) fn spline_blob() -> Vec<u8> {
    let rails = rails();
    let segments = 16 + rails.len() * 32;
    let mut bytes = vec![0; segments + rails.len() * 144];
    word(&mut bytes, 0, rails.len() as u32);
    word(&mut bytes, 4, rails.len() as u32);
    word(&mut bytes, 8, 16);
    word(&mut bytes, 12, segments as u32);
    for (i, rail) in rails.iter().enumerate() {
        let r = 16 + i * 32;
        let s = segments + i * 144;
        bytes[r..r + 8].copy_from_slice(&rail.id.to_be_bytes());
        bytes[r + 8..r + 16].copy_from_slice(&0x2c7017070007004au64.to_be_bytes());
        word(&mut bytes, r + 20, s as u32);
        word(&mut bytes, r + 24, s as u32);
        let delta = [rail.end.x - rail.start.x, rail.end.y - rail.start.y,
            rail.end.z - rail.start.z];
        let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        vector(&mut bytes, s, [delta[0], delta[1], delta[2], 0.]);
        vector(&mut bytes, s + 48, [rail.start.x, rail.start.y, rail.start.z, 1.]);
        vector(&mut bytes, s + 64, [1. / length, 0., 0., 0.]);
        vector(&mut bytes, s + 80, [rail.start.x.min(rail.end.x),
            rail.start.y.min(rail.end.y), rail.start.z.min(rail.end.z), 0.]);
        vector(&mut bytes, s + 96, [rail.start.x.max(rail.end.x),
            rail.start.y.max(rail.end.y), rail.start.z.max(rail.end.z), 0.]);
        word(&mut bytes, s + 112, length.to_bits());
        word(&mut bytes, s + 120, r as u32);
    }
    bytes
}

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
fn vector(bytes: &mut [u8], at: usize, value: [f32; 4]) {
    for (i, v) in value.into_iter().enumerate() { word(bytes, at + i * 4, v.to_bits()); }
}
