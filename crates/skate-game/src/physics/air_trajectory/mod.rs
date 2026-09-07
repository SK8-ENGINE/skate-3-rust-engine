//! Real stock-settings and BoardWorld adapter for the player trajectory selector.
mod grind;
mod settings;
mod world;
use skate_core::{
    air::trajectory::{
        query_trajectory, LaunchInfo, QueryRequest, QueryResult, SelectorInput, SelectorSettings,
        TrajectorySelector,
    },
    physics::board_world::BoardWorld,
};
use skate_data::collections::Collections;

pub struct AirTrajectoryRuntime {
    pub selector: TrajectorySelector,
    pub settings: SelectorSettings,
    pub grind_board_position: [f32; 4],
    pub grind_com_position: [f32; 4],
    pub edges: Vec<skate_core::physics::grind_contact::Primitive>,
    grind_candidates: Vec<skate_core::physics::grind_contact::Primitive>,
    grind_settings: grind::Settings,
    pending_results: Option<Vec<QueryResult>>,
}
impl AirTrajectoryRuntime {
    ///Full82E099A0 query shared by trajectory and Footplant callers.
    pub fn query(world: &BoardWorld, request: QueryRequest) -> Result<QueryResult, String> {
        query_trajectory(
            request,
            |start, end, radius| world::line(world, start, end, radius),
            |position, radius| world::nearby(world, position, radius),
        )
    }
    pub fn load(collections: &Collections) -> Result<Self, String> {
        Ok(Self {
            selector: TrajectorySelector::new(),
            settings: settings::load(collections)?,
            pending_results: None,
            edges: Vec::new(),
            grind_board_position: [0.; 4],
            grind_com_position: [0.; 4],
            grind_settings: grind::Settings::load(collections)?,
            grind_candidates: Vec::new(),
        })
    }
    pub fn launch(
        &mut self,
        info: LaunchInfo,
        input: SelectorInput,
        world: &BoardWorld,
    ) -> Result<bool, String> {
        let launched = self.selector.launch(info, input, &self.settings)?;
        if launched {
            self.submit(world)?;
        }
        Ok(launched)
    }
    pub fn update(&mut self, input: SelectorInput, world: &BoardWorld) -> Result<bool, String> {
        let Some(results) = self.pending_results.take() else {
            return Ok(self.selector.update_without_completion());
        };
        let valid = self.selector.complete_batch_with_grinds(
            &results,
            input,
            &self.settings,
            |prediction, acquire| {
                self.grind_settings.evaluate(
                    prediction,
                    acquire,
                    world,
                    &self.edges,
                    &mut self.grind_candidates,
                    input.grind_lock_distance,
                    self.grind_com_position,
                    self.grind_board_position,
                )
            },
            |start, end, radius| world::line(world, start, end, radius),
        )?;
        if self.selector.pending() {
            self.submit(world)?;
        }
        Ok(valid)
    }
    pub fn cancel_pending(&mut self) {
        self.pending_results = None;
        self.selector.cancel_pending();
    }
    fn submit(&mut self, world: &BoardWorld) -> Result<(), String> {
        let results = self
            .selector
            .requests()
            .iter()
            .map(|&request| Self::query(world, request))
            .collect::<Result<Vec<_>, _>>()?;
        self.pending_results = Some(results);
        Ok(())
    }
}
