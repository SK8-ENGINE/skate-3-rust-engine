//! TU3 chromosome producers82DEE7E0/82DEE918/82DEED98/82DEEEE0/82DEEFB0.
use skate_core::riding::ground_correction_math::dot_product as dot;
type V = [f32; 4];
#[path = "grind_names.rs"]
mod names;

#[derive(Clone, Copy, Default)]
pub(super) struct Pose {
    pub animated_board: [V; 4],
    pub board: [V; 4],
    pub foot_directions: [V; 2],
    pub fakie: bool,
}
#[derive(Default)]
pub(super) struct Chromosome {
    history: std::collections::VecDeque<Pose>,
    approach_pose: Pose,
    approach: usize,
    previous_kind: Option<u32>,
    away_frames: u32,
    reversed: bool,
    travel: Option<usize>,
    pending: [usize; 6],
    pending_frames: u32,
    animation: [usize; 6],
    scoring: [usize; 6],
}
impl Chromosome {
    pub fn observe(&mut self, pose: Pose, category: u32) {
        if category == 100 {
            self.history.push_back(pose);
            if self.history.len() > 30 {
                self.history.pop_front();
            }
            self.approach_pose = pose;
        }
        if category != 400 {
            self.away_frames = self.away_frames.saturating_add(1);
            self.previous_kind = None;
            self.travel = None;
        }
    }
    pub fn update(
        &mut self,
        pose: Pose,
        kind: u32,
        point: V,
        direction: V,
        normal: V,
    ) -> (&'static str, &'static str) {
        let new_grind = self.previous_kind.is_none();
        let reference = if self.history.len() >= 30 {
            self.history[0]
        } else {
            self.approach_pose
        };
        if new_grind && self.away_frames > 30 {
            let across = cross(direction, normal);
            let from = sub(reference.animated_board[3], point);
            let across = signed(across, dot(across, from) > 0.0);
            let right = signed(
                reference.animated_board[0],
                dot(
                    add(reference.foot_directions[0], reference.foot_directions[1]),
                    reference.animated_board[0],
                ) > 0.0,
            );
            self.approach = usize::from(dot(right, across) > 0.0);
        }
        let board_end = if matches!(kind, 2 | 3 | 4) {
            usize::from(dot(sub(point, pose.board[3]), pose.board[2]) <= 0.0)
        } else {
            0
        };
        let projected = sub(
            pose.board[2],
            normal.map(|v| v * dot(pose.board[2], normal)),
        );
        let length = dot(projected, projected).sqrt();
        let straight = length <= 0.01 || (dot(projected, direction) / length).abs() > 0.9397;
        let tip_axis = signed(
            pose.animated_board[2],
            dot(sub(pose.animated_board[3], point), pose.animated_board[2]) > 0.0,
        );
        let low = dot(normal, tip_axis) <= -0.1;
        let feet = add(pose.foot_directions[0], pose.foot_directions[1]);
        let right = signed(
            pose.animated_board[0],
            dot(feet, pose.animated_board[0]) > 0.0,
        );
        let forward = dot(right, direction) > 0.0;
        let travel = if matches!(kind, 0 | 3) {
            if self.travel.is_none() {
                self.reversed = dot(
                    feet,
                    add(reference.foot_directions[0], reference.foot_directions[1]),
                ) < 0.0;
                if reference.fakie {
                    self.reversed = !self.reversed;
                }
            }
            match (forward, self.reversed) {
                (true, false) => 0,
                (false, false) => 1,
                (true, true) => 2,
                (false, true) => 3,
            }
        } else {
            usize::from(!forward)
        };
        self.travel = Some(travel);
        let chromosome = [
            self.approach,
            board_end,
            usize::from(straight),
            usize::from(low),
            travel,
            kind as usize,
        ];
        //82DEF518 publishes animation after one stable repeat and scoring after
        //13; entering a grind publishes both immediately.
        if new_grind {
            self.pending = chromosome;
            self.pending_frames = 13;
        } else if self.pending == chromosome {
            self.pending_frames = self.pending_frames.saturating_add(1);
        } else {
            self.pending = chromosome;
            self.pending_frames = 0;
        }
        if self.pending_frames > 0 {
            self.animation = self.pending;
        }
        if self.pending_frames > 12 {
            self.scoring = self.pending;
        }
        self.previous_kind = Some(kind);
        self.away_frames = 0;
        let animation = index(self.animation);
        let scoring = index(self.scoring);
        (names::NAMES[animation], names::NAMES[scoring])
    }
}
fn index(v: [usize; 6]) -> usize {
    (((((v[0] * 2 + v[1]) * 2 + v[2]) * 2 + v[3]) * 4 + v[4]) * 6) + v[5]
}
fn sub(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] - b[i])
}
fn add(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] + b[i])
}
fn signed(v: V, positive: bool) -> V {
    v.map(|x| if positive { x } else { -x })
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
        0.0,
    ]
}
