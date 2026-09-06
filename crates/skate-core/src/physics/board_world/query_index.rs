//! Static bounds hierarchy over authored query meshes. Results are sorted back
//! into source order before narrow-phase, preserving equal-hit/contact order.
use super::query_metadata::{Bounds, QueryMesh};
use std::ops::Range;

#[derive(Default)]
pub(super) struct QueryIndex {
    nodes: Vec<Node>,
    order: Vec<usize>,
}
struct Node {
    bounds: Bounds,
    children: Option<(usize, usize)>,
    range: Range<usize>,
}
impl QueryIndex {
    pub(super) fn new(meshes: &[QueryMesh]) -> Self {
        let mut index = Self { nodes: Vec::new(), order: (0..meshes.len()).collect() };
        if !meshes.is_empty() { index.build(meshes, 0..meshes.len()); }
        index
    }
    fn build(&mut self, meshes: &[QueryMesh], range: Range<usize>) -> usize {
        let bounds = Bounds::from_points(self.order[range.clone()].iter()
            .flat_map(|&i| [meshes[i].local_bounds.min, meshes[i].local_bounds.max])).unwrap();
        let id = self.nodes.len();
        self.nodes.push(Node { bounds, children: None, range: range.clone() });
        if range.len() > 8 {
            let extents = [bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y, bounds.max.z - bounds.min.z];
            let axis = (0..3).max_by(|&a, &b| extents[a].total_cmp(&extents[b])).unwrap();
            let center = |i: usize| {
                let b = meshes[i].local_bounds;
                let low = [b.min.x, b.min.y, b.min.z][axis];
                let high = [b.max.x, b.max.y, b.max.z][axis];
                low * 0.5 + high * 0.5
            };
            let mid = range.start + range.len() / 2;
            self.order[range.clone()].select_nth_unstable_by(range.len() / 2,
                |&a, &b| center(a).total_cmp(&center(b)).then(a.cmp(&b)));
            let left = self.build(meshes, range.start..mid);
            let right = self.build(meshes, mid..range.end);
            self.nodes[id].children = Some((left, right));
        }
        id
    }
    pub(super) fn query(&self, bounds: Bounds, meshes: &[QueryMesh]) -> Vec<usize> {
        let mut result = Vec::new();
        if self.nodes.is_empty() { return result; }
        let mut stack = vec![0];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            if !node.bounds.overlaps(bounds) { continue; }
            if let Some((left, right)) = node.children {
                stack.push(right); stack.push(left);
            } else {
                result.extend(self.order[node.range.clone()].iter().copied()
                    .filter(|&i| meshes[i].local_bounds.overlaps(bounds)));
            }
        }
        result.sort_unstable();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{math::Vector3, physics::{drive_frames::RetailAffineTransform, board_world::query_metadata::QueryPool}};
    #[test]
    fn hierarchy_matches_linear_scan_including_boundaries_and_source_order() {
        let meshes: Vec<_> = (0..1024).map(|i| {
            let x = ((i * 37) % 32) as f32 * 10.;
            let z = (i / 32) as f32 * 10.;
            QueryMesh { triangle_range: i..i+1,
                local_to_world: RetailAffineTransform::IDENTITY, world_to_local: RetailAffineTransform::IDENTITY,
                local_bounds: Bounds { min: Vector3::new(x, 0., z), max: Vector3::new(x+5., 5., z+5.) },
                matching_group: -1, pool: QueryPool::Ground }
        }).collect();
        let index = QueryIndex::new(&meshes);
        for x in [-100., 0., 5., 10., 47., 155., 310., 320.] {
            for radius in [0., 5., 31., 1000.] {
                let bounds = Bounds::from_points([Vector3::new(x, 0., x)]).unwrap().expanded(radius);
                let expected: Vec<_> = meshes.iter().enumerate().filter(|(_, m)| m.local_bounds.overlaps(bounds)).map(|(i, _)| i).collect();
                assert_eq!(index.query(bounds, &meshes), expected);
            }
        }
    }
}
