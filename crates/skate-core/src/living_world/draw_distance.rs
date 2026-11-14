//! NPC draw distance (QoL, not retail): one multiplier on population distances, caps scaled by area.
//! What scales and what does not: docs/hails-additions/26b-living-world-population.md#npc-draw-distance

use super::census::{CensusCircle, CensusRange};
use super::config::{CensusKindConfig, PopulationConfig, SkaterConfig};

/// The draw distance multiplier with its scaling rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawDistance(f32);

impl DrawDistance {
    /// Retail ranges (the default).
    pub const RETAIL: f32 = 1.0;
    /// The menu's "None" step (QoL, not retail): no ambient NPCs at all. Not a multiplier: the
    /// engine turns the population off for it and keeps the ranges at retail.
    pub const NONE: f32 = 0.0;
    /// The steps the settings menu offers (None, Retail, 1.5x, 2x, 3x).
    pub const MENU_STEPS: [f32; 5] = [Self::NONE, 1.0, 1.5, 2.0, 3.0];
    /// Accepted range for settings and mods; anything else falls back to retail or is clamped.
    pub const MIN: f32 = 0.25;
    pub const MAX: f32 = 4.0;

    /// A usable multiplier: non-finite or non-positive values mean retail, the rest is clamped to
    /// [`MIN`, `MAX`].
    pub fn new(multiplier: f32) -> Self {
        if multiplier.is_finite() && multiplier > 0.0 {
            Self(multiplier.clamp(Self::MIN, Self::MAX))
        } else {
            Self(Self::RETAIL)
        }
    }

    pub fn multiplier(self) -> f32 {
        self.0
    }

    pub fn is_retail(self) -> bool {
        self.0 == Self::RETAIL
    }

    /// Area factor m^2.
    pub fn area(self) -> f32 {
        self.0 * self.0
    }

    /// A distance x m (unchanged at retail).
    pub fn distance(self, d: f32) -> f32 {
        if self.is_retail() {
            d
        } else {
            d * self.0
        }
    }

    /// A count x m^2, rounded (unchanged at retail).
    pub fn count(self, n: u32) -> u32 {
        if self.is_retail() {
            n
        } else {
            (n as f32 * self.area()).round().min(u32::MAX as f32) as u32
        }
    }

    pub fn circle(self, c: CensusCircle) -> CensusCircle {
        CensusCircle {
            spawn_inner: self.distance(c.spawn_inner),
            spawn_outer: self.distance(c.spawn_outer),
            cull: self.distance(c.cull),
            forward_offset: self.distance(c.forward_offset),
            speed_kmh: c.speed_kmh,
        }
    }

    pub fn range(self, r: CensusRange) -> CensusRange {
        CensusRange { slow: self.circle(r.slow), fast: self.circle(r.fast) }
    }

    pub fn census_kind(self, cfg: &CensusKindConfig) -> CensusKindConfig {
        CensusKindConfig {
            density: cfg.density * self.area(),
            range: cfg.range.map(|r| self.range(r)),
            attempts_per_pass: self.count(cfg.attempts_per_pass),
            spawns_per_pass: self.count(cfg.spawns_per_pass),
            initial_attempts: self.count(cfg.initial_attempts),
            initial_spawns: self.count(cfg.initial_spawns),
            initial_ring: (self.distance(cfg.initial_ring.0), self.distance(cfg.initial_ring.1)),
            pool: cfg.pool.map(|p| self.count(p)),
            initial_pool: cfg.initial_pool.map(|p| self.count(p)),
            ..cfg.clone()
        }
    }

    pub fn skaters(self, cfg: &SkaterConfig) -> SkaterConfig {
        SkaterConfig {
            desired: self.count(cfg.desired),
            ai_cap: self.count(cfg.ai_cap),
            // Slot 0 is the local player: the ambient slots scale, the player slot stays.
            slots: if self.is_retail() { cfg.slots } else { 1 + self.count(cfg.slots.saturating_sub(1)).min(u8::MAX as u32 - 1) },
            pool_size: self.count(cfg.pool_size),
            spawn_inner: self.distance(cfg.spawn_inner),
            spawn_outer: self.distance(cfg.spawn_outer),
            max_candidate_lines: if self.is_retail() { cfg.max_candidate_lines } else { (cfg.max_candidate_lines as f32 * self.area()).round() as usize },
            cull: self.distance(cfg.cull),
            ..cfg.clone()
        }
    }

    /// The config the population runs with: `None` at retail (run `cfg` itself, unchanged).
    pub fn scaled(self, cfg: &PopulationConfig) -> Option<PopulationConfig> {
        if self.is_retail() {
            return None;
        }
        Some(PopulationConfig {
            skaters: self.skaters(&cfg.skaters),
            pedestrians: self.census_kind(&cfg.pedestrians),
            vehicles: self.census_kind(&cfg.vehicles),
            draw_distance: cfg.draw_distance,
        })
    }
}

impl Default for DrawDistance {
    fn default() -> Self {
        Self(Self::RETAIL)
    }
}
