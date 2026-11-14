//! Living-world traffic V1: road graph, lane cursor, signals, junction entry query. Pure.
//! Retail reference, multiplayer and mod seams: docs/hails-additions/26h-living-world-traffic.md#traffic-core

pub mod cursor;
pub mod follow;
pub mod graph;
pub mod horn;
pub mod junction;
pub mod manoeuvre;
pub mod passage;
pub mod obstacles;
pub mod signals;
pub mod skater_scan;
pub mod spots;
pub mod spawn;

pub use cursor::{choose_connector, Advance, ConnectorChoice, LaneCursor, Place};
pub use follow::{Car, FollowEvent, FollowParams};
pub use graph::{
    ConnectorId, ConnectorInput, Curve, EndInput, Frame, JunctionId, JunctionInput, LaneId, PieceInput, RoadInput, RoadNetwork, SegmentId,
    SegmentInput, TrafficError, Turn,
};
pub use junction::{junction_entry, query_due, Entry, EntryInfo, EntryQuery, Occupancy, VehicleKey, VehicleSnapshot, Vehicles};
pub use signals::{priority_end, Controller, Light, Phase, SignalChange, SignalClock, SignalTimings};
pub use spawn::{LaneCar, LaneSpot, PlacementRules};

#[cfg(test)]
mod tests;
