//! The stock ped AI graph (`Pedestrian.stategraph`) on the shared graph runtime, ops parsed into `PedOp`.
//! Details: docs/hails-additions/26g-living-world-ped-interactions.md#ped-ai-graph-loading

use crate::graph_runtime::{load_graph, LoadedGraph};
use skate_core::living_world::peds::brain::PedOp;
use skate_data::state_graph::attributes::Attributes;
use std::collections::BTreeMap;
use std::path::Path;

/// Asset-root relative paths of the stock ped graphs.
pub(crate) const AI_GRAPH: &str = "private/stock/data/state/livingworldentities/pedestrian/Pedestrian.stategraph";
pub(crate) const CONVERSATION_GRAPH: &str = "private/stock/data/state/livingworldentities/pedestrian/plugin/conversation.stategraph";
pub(crate) const MOTION_GRAPH: &str = "private/stock/data/state/livingworldentities/pedestrian/MotionGraph_Pedestrian.stategraph";

/// The ped AI graph, compiled, with its operations parsed per behaviour and condition id.
pub(crate) struct PedGraph {
    pub graph: LoadedGraph,
    pub behaviors: Vec<PedOp>,
    pub conditions: Vec<PedOp>,
}

impl PedGraph {
    pub fn load(root: &Path, relative: &str) -> Result<Self, String> {
        let graph = load_graph(root, relative)?;
        let op = |operation: usize| {
            let o = &graph.binding.operations[operation];
            let attributes = Attributes::new(&graph.source.elements[o.element].attributes);
            let text = |k: &str| attributes.text(k).map(str::to_string);
            let float = |k: &str| attributes.get(k).map(|a| f32::from_bits(a.float_bits));
            PedOp::parse(&o.name, &text, &float)
        };
        let behaviors = graph.runtime.operations.behaviors.iter().map(|&o| op(o)).collect();
        let conditions = graph.runtime.operations.conditions.iter().map(|&o| op(o)).collect();
        Ok(Self { graph, behaviors, conditions })
    }

    /// Operation names not ported yet, with how often the graph uses each.
    pub fn pending(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for op in self.behaviors.iter().chain(&self.conditions) {
            if let PedOp::Pending { name } = op {
                *out.entry(name.clone()).or_insert(0) += 1;
            }
        }
        out
    }
}
