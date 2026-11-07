//! The race tracker (research b973, b975): course, estimated time to finish, rank and gap per racer.
//! Details: docs/hails-additions/42-challenges.md#change-the-race-tracker-cratesskate-coresrcchallengesracers

pub type RacerId = u32;

/// Tunables with the retail values as defaults; a mod can replace them per race.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaceTuning {
    /// The reference speed the remaining distance is divided by (0x822F9118 = 1/9).
    pub reference_speed: f32,
    /// A segment not longer than this has no direction (0x830BD350).
    pub segment_epsilon: f32,
    /// The leaderboard refresh period (30 ticks at the console's 30 fps; RaceRefreshTime).
    pub hud_refresh_seconds: f32,
}

impl Default for RaceTuning {
    fn default() -> Self {
        Self { reference_speed: 9.0, segment_epsilon: 1.0e-6, hud_refresh_seconds: 1.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Leg {
    /// Point 0 = the leg start, then one point per split trigger (the volume's centre), the last = the finish.
    pub points: Vec<[f32; 3]>,
    pub length: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Course {
    pub legs: Vec<Leg>,
}

impl Course {
    /// Leg 1 starts at `start`, each later leg at the previous leg's last point (`sub_82560BC8`).
    pub fn new(start: [f32; 3], splits_per_leg: &[Vec<[f32; 3]>]) -> Self {
        let mut legs = Vec::with_capacity(splits_per_leg.len());
        let mut from = start;
        for splits in splits_per_leg {
            let mut points = Vec::with_capacity(splits.len() + 1);
            points.push(from);
            points.extend_from_slice(splits);
            from = *points.last().unwrap_or(&from);
            let length = points.windows(2).map(|w| distance(w[0], w[1])).sum();
            legs.push(Leg { points, length });
        }
        Self { legs }
    }

    /// Distance to the finish along the split polyline for a racer on `leg` past `split` split triggers
    /// (`sub_82560790`): split points already passed (by projection) are skipped.
    pub fn remaining(&self, leg: usize, split: usize, position: [f32; 3], epsilon: f32) -> f32 {
        let Some(current) = self.legs.get(leg) else { return 0.0 };
        let mut t = split + 1;
        let d = loop {
            let d = projection(&current.points, position, t, epsilon);
            if d >= 0.0 {
                break d;
            }
            t += 1;
        };
        d.abs() + length_from(&current.points, t) + self.legs[leg + 1..].iter().map(|l| l.length).sum::<f32>()
    }
}

/// `sub_82560A88`: the signed distance from `position` to point `t` along the incoming segment's direction.
fn projection(points: &[[f32; 3]], position: [f32; 3], t: usize, epsilon: f32) -> f32 {
    if t >= points.len() {
        return 0.0;
    }
    if t == 0 {
        return distance(points[0], position);
    }
    let dir = sub(points[t], points[t - 1]);
    let len = dot(dir, dir).sqrt();
    if len <= epsilon {
        return 0.0;
    }
    dot(sub(points[t], position), dir) / len
}

/// `sub_825608B8`: the polyline length from point `from` to the last point.
fn length_from(points: &[[f32; 3]], from: usize) -> f32 {
    points.get(from..).map_or(0.0, |rest| rest.windows(2).map(|w| distance(w[0], w[1])).sum())
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

/// One racer's node (AddPlayerToRace 82856330).
#[derive(Clone, Debug, PartialEq)]
pub struct Racer {
    pub id: RacerId,
    pub team: Option<u32>,
    pub leg: usize,
    pub split: usize,
    /// Estimated time to finish in seconds; 0 once finished.
    pub eta: f32,
    /// Seconds behind the leader.
    pub gap: f32,
    pub finish_time: Option<f32>,
    /// 1-based; -1 until the first update.
    pub rank: i32,
    pub disconnected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceTracker {
    pub course: Course,
    pub tuning: RaceTuning,
    pub racers: Vec<Racer>,
}

impl RaceTracker {
    /// StartRaceStats: a new heat on `course`, no racers.
    pub fn new(course: Course, tuning: RaceTuning) -> Self {
        Self { course, tuning, racers: Vec::new() }
    }

    pub fn add_racer(&mut self, id: RacerId, team: Option<u32>) {
        if self.racer(id).is_none() {
            self.racers.push(Racer { id, team, leg: 0, split: 0, eta: 0.0, gap: 0.0, finish_time: None, rank: -1, disconnected: false });
        }
    }

    pub fn racer(&self, id: RacerId) -> Option<&Racer> {
        self.racers.iter().find(|r| r.id == id)
    }

    fn racer_mut(&mut self, id: RacerId) -> Option<&mut Racer> {
        self.racers.iter_mut().find(|r| r.id == id)
    }

    /// SetPlayerProgress(id, leg, split): `split` = the number of split triggers entered on `leg` (0-based leg).
    pub fn set_progress(&mut self, id: RacerId, leg: usize, split: usize) {
        if let Some(r) = self.racer_mut(id) {
            r.leg = leg;
            r.split = split;
        }
    }

    pub fn finish(&mut self, id: RacerId, time: f32) {
        if let Some(r) = self.racer_mut(id) {
            r.finish_time = Some(time);
        }
    }

    pub fn disconnect(&mut self, id: RacerId) {
        if let Some(r) = self.racer_mut(id) {
            r.disconnected = true;
        }
    }

    /// GetEstimatedTimeToFinish: 0 for an unknown racer.
    pub fn eta(&self, id: RacerId) -> f32 {
        self.racer(id).map_or(0.0, |r| r.eta)
    }

    /// GetRank: -1 for an unknown racer.
    pub fn rank(&self, id: RacerId) -> i32 {
        self.racer(id).map_or(-1, |r| r.rank)
    }

    /// One tick (`sub_82560170`): ETA per racer from its position, then ranks and gaps. `position` gives a racer's
    /// position (None skips it, as a failed actor lookup does); `timer` its count-up race time.
    pub fn update(&mut self, position: impl Fn(RacerId) -> Option<[f32; 3]>, timer: impl Fn(RacerId) -> f32) {
        let speed = self.tuning.reference_speed.max(f32::EPSILON);
        for r in &mut self.racers {
            if r.disconnected {
                continue;
            }
            if r.finish_time.is_some() {
                r.eta = 0.0;
                continue;
            }
            if let Some(p) = position(r.id) {
                r.eta = self.course.remaining(r.leg, r.split, p, self.tuning.segment_epsilon) / speed;
            }
        }
        // Finished racers by their previous rank, then active ones by ETA, then disconnected ones by previous rank
        // (`sub_8255FB38`). Retail's sort is not stable; ties keep the join order here so every peer agrees.
        let mut order: Vec<usize> = (0..self.racers.len()).collect();
        order.sort_by(|&a, &b| {
            let (ra, rb) = (&self.racers[a], &self.racers[b]);
            let class = |r: &Racer| if r.disconnected { 2 } else if r.finish_time.is_some() { 0 } else { 1 };
            class(ra).cmp(&class(rb)).then_with(|| match class(ra) {
                1 => ra.eta.total_cmp(&rb.eta),
                _ => ra.rank.cmp(&rb.rank),
            })
        });
        for (rank, &i) in order.iter().enumerate() {
            self.racers[i].rank = rank as i32 + 1;
        }
        let Some(&leader) = order.first() else { return };
        let (leader_eta, leader_finish) = (self.racers[leader].eta, self.racers[leader].finish_time);
        for r in &mut self.racers {
            r.gap = (r.eta - leader_eta) + leader_finish.map_or(0.0, |f| timer(r.id) - f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight() -> RaceTracker {
        let course = Course::new([0.0; 3], &[vec![[9.0, 0.0, 0.0], [18.0, 0.0, 0.0], [27.0, 0.0, 0.0]]]);
        RaceTracker::new(course, RaceTuning::default())
    }

    #[test]
    fn eta_is_the_remaining_split_distance_at_nine_metres_a_second() {
        let mut race = straight();
        race.add_racer(1, None);
        race.update(|_| Some([0.0; 3]), |_| 0.0);
        assert!((race.eta(1) - 3.0).abs() < 1e-5);
        race.update(|_| Some([4.5, 0.0, 0.0]), |_| 0.0);
        assert!((race.eta(1) - 2.5).abs() < 1e-5);
    }

    #[test]
    fn passed_split_points_are_skipped_by_projection_without_a_trigger_event() {
        let race = straight();
        let ahead = race.course.remaining(0, 0, [20.0, 3.0, 0.0], 1e-6);
        assert!((ahead - 7.0).abs() < 1e-5, "{ahead}");
        assert_eq!(race.course.remaining(0, 0, [40.0, 0.0, 0.0], 1e-6), 0.0);
    }

    #[test]
    fn later_legs_add_their_whole_length() {
        let course = Course::new([0.0; 3], &[vec![[10.0, 0.0, 0.0]], vec![[10.0, 5.0, 0.0]]]);
        assert_eq!(course.legs[1].points[0], [10.0, 0.0, 0.0]);
        assert!((course.remaining(0, 0, [0.0; 3], 1e-6) - 15.0).abs() < 1e-5);
        assert!((course.remaining(1, 0, [10.0, 0.0, 0.0], 1e-6) - 5.0).abs() < 1e-5);
        assert_eq!(course.remaining(2, 0, [0.0; 3], 1e-6), 0.0);
    }

    #[test]
    fn a_zero_length_segment_has_no_direction() {
        let course = Course::new([0.0; 3], &[vec![[0.0; 3], [5.0, 0.0, 0.0]]]);
        assert!((course.remaining(0, 0, [0.0; 3], 1e-6) - 5.0).abs() < 1e-5);
    }

    #[test]
    fn ranks_put_finished_first_in_finish_order_then_eta_then_disconnected() {
        let mut race = straight();
        for id in 1..=4 {
            race.add_racer(id, None);
        }
        let at = |id: RacerId| Some([id as f32, 0.0, 0.0]);
        race.update(at, |_| 0.0);
        assert_eq!([1, 2, 3, 4].map(|id| race.rank(id)), [4, 3, 2, 1]);
        race.finish(3, 30.0);
        race.update(at, |_| 0.0);
        race.finish(4, 31.0);
        race.disconnect(1);
        race.update(at, |_| 0.0);
        assert_eq!([1, 2, 3, 4].map(|id| race.rank(id)), [4, 3, 1, 2]);
        assert_eq!(race.eta(3), 0.0);
        assert_eq!(race.rank(9), -1);
    }

    #[test]
    fn gap_adds_the_racers_time_past_the_leaders_finish() {
        let mut race = straight();
        race.add_racer(1, None);
        race.add_racer(2, None);
        race.finish(1, 20.0);
        race.update(|_| Some([9.0, 0.0, 0.0]), |_| 25.0);
        let gap = race.racer(2).unwrap().gap;
        assert!((gap - (2.0 + 5.0)).abs() < 1e-5, "{gap}");
    }
}
