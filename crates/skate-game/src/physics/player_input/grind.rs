//! Input timing/gates from82D8A828/82D8ABD8. The geometry query/scorer runs
//! in physics::grind after input completion, before the state selector.
use skate_core::{player::input_phase::ProcessedPhysicsInput, point_graph::PointGraph};
use skate_data::collections::Collections;
#[derive(Clone, Copy, Debug)]
pub(crate) struct NoGrindEdges;
#[derive(Debug)]
pub(crate) struct GrindInputState {
    previous_state: u32,
    engagement_counter: u32,
    cooldown: u32,
    pub disabled: bool,
    pub suppressed: bool,
    pub elapsed: f32,
    pub friction_vs_time: f32,
    pub previous_velocity: [u32; 4],
    pub grind_history: u32,
    pub secondary_history: u32,
    pub grounded_frames: u32,
    pub air_frames: u32,
    pub low_wheel_frames: u32,
    previous_candidate: bool,
    curve: PointGraph<4>,
}
impl GrindInputState {
    pub fn load(data: &Collections) -> Result<Self, String> {
        //XML C7220-C70E0=320; native PointGraph X336/Y352 skips header16.
        let graph = data
            .words::<12>("physics_grinds", "default", "FrictionVsTime")?
            .map(f32::from_bits);
        //82D8A318 scorer storesC/10/14/18/1C/20 and48 tozero;82D8A638
        //resets main464/468/472/476/477/480/484 and all416outputbytes.
        Ok(Self {
            previous_state: 0,
            engagement_counter: 0,
            cooldown: 0,
            disabled: false,
            suppressed: false,
            elapsed: 0.,
            friction_vs_time: 0.,
            previous_velocity: [0; 4],
            grind_history: 0,
            secondary_history: 0,
            grounded_frames: 0,
            air_frames: 0,
            low_wheel_frames: 0,
            previous_candidate: false,
            curve: PointGraph {
                x: graph[4..8].try_into().unwrap(),
                y: graph[8..12].try_into().unwrap(),
            },
        })
    }
    pub fn update(
        &mut self,
        _world: NoGrindEdges,
        p: &ProcessedPhysicsInput,
        air_counter: i32,
    ) -> Result<(), String> {
        let state = p.state_2508;
        if state != self.previous_state {
            self.engagement_counter = self.engagement_counter.wrapping_add(match state {
                400 | 402 | 404 => 50,
                403 => 20,
                _ => 0,
            });
            self.previous_state = state;
        }
        self.engagement_counter = decrement(self.engagement_counter);
        self.suppressed = self.engagement_counter > 151;
        self.cooldown = if self.suppressed {
            90
        } else {
            decrement(self.cooldown)
        };
        self.disabled = p.flags_2468 & 0x1800_0000 != 0
            || p.flags_2472 & 0x8008 != 0
            || p.flags_2476 & 0x0040_0000 != 0
            || self.cooldown > 0
            || (p.category_2512 == 200
                && (p.state_timer_2664 <= f32::from_bits(0x3da3_d70a)
                    || air_counter <= 10))
            || p.category_2512 == 500
            || (p.flags_2472 & 4 != 0 && p.category_2512 != 400);
        self.elapsed = if p.category_2512 == 400 || state == 701 {
            self.elapsed + p.timestep_2604
        } else {
            0.
        };
        self.low_wheel_frames = if self.previous_candidate
            && state == 100
            && p.wheel_count_2556 < 2
            && p.scalar_2652 < 0.8
        {
            self.low_wheel_frames.wrapping_add(1)
        } else {
            0
        };
        self.grind_history = if p.grind_words_2532_2536[1] == 2 {
            70
        } else {
            decrement(self.grind_history)
        };
        self.secondary_history = decrement(self.secondary_history);
        self.grounded_frames = if p.category_2512 == 100 {
            self.grounded_frames.wrapping_add(1)
        } else {
            0
        };
        self.air_frames = if p.category_2512 == 100 {
            0
        } else {
            self.air_frames.wrapping_add(1)
        };
        //82D875A8 LABEL80 on edge count0 clears candidate384 and owner32.
        self.previous_candidate = false;
        self.previous_velocity = p.vectors_400_416[0];
        self.friction_vs_time = self.curve.evaluate(self.elapsed);
        Ok(())
    }
}
fn decrement(value: u32) -> u32 {
    let next = value.wrapping_sub(1);
    if next & 0x8000_0000 != 0 { 0 } else { next }
}
