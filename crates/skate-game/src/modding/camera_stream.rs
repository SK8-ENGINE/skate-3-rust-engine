//! Authenticated peer camera transforms for the generic camera.watch API.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(super) const KEY: &str = "s2:camera";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct View { p: [f32; 3], q: [f32; 4], pub fov: f32 }
impl View {
    fn valid(&self) -> bool {
        self.p.iter().all(|x| x.is_finite() && x.abs() <= 100_000.)
            && self.q.iter().all(|x| x.is_finite())
            && (0.9..1.1).contains(&self.q.iter().map(|x| x*x).sum::<f32>())
            && self.fov.is_finite() && (0.1..3.1).contains(&self.fov)
    }
    pub fn transform(&self) -> Transform {
        Transform::from_translation(Vec3::from_array(self.p)).with_rotation(Quat::from_array(self.q).normalize())
    }
}
pub(super) fn capture(world: &mut World) -> Option<Vec<u8>> {
    let mut cameras = world.query_filtered::<(&Transform, &Projection), With<crate::camera::GameplayCamera>>();
    let (t, p) = cameras.iter(world).next()?;
    let Projection::Perspective(p) = p else { return None; };
    let view = View { p: t.translation.to_array(), q: t.rotation.to_array(), fov: p.fov };
    view.valid().then(|| serde_json::to_vec(&view).ok()).flatten()
}
pub(super) fn ingest(views: &mut BTreeMap<u64, View>, records: &[(u64,String,u32,Vec<u8>)], players: &[u64]) {
    views.clear();
    for (peer,key,_,bytes) in records {
        if key != KEY || !players.contains(peer) {continue;}
        if let Ok(view) = serde_json::from_slice::<View>(bytes) {
            if view.valid() {views.insert(*peer,view);}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_packets_require_connected_peer_and_valid_pose() {
        let mut views = BTreeMap::new();
        let view = View {p:[1.,2.,3.],q:[0.,0.,0.,1.],fov:1.};
        let record = (7,KEY.into(),1,serde_json::to_vec(&view).unwrap());
        ingest(&mut views,&[record.clone()],&[]); assert!(views.is_empty());
        ingest(&mut views,&[record],&[7]); assert_eq!(views[&7].transform().translation,Vec3::new(1.,2.,3.));
        let invalid=View {q:[0.;4],..view};
        ingest(&mut views,&[(7,KEY.into(),2,serde_json::to_vec(&invalid).unwrap())],&[7]); assert!(views.is_empty());
    }
}
