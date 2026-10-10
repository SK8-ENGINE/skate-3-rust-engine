//! The simple lane follower of milestone V3: census cars drive their lanes, take connectors by
//! retail's least-loaded rule, stop at the stop line when the V1 junction query says so and keep
//! a gap to the car ahead. The full retail driver (look-ahead `sub_82C412D8`, speed planner
//! `sub_82C3FA08` with its lead / obstacle cases, horn and manoeuvre deciders, the state graph)
//! is milestone V4; this module is the part of it V3 needs to put moving, light-obeying traffic on
//! screen, with every simplification named below.
//!
//! Retail [code, TU3; addresses are evidence only, behaviour re-implemented, not copied]:
//! - integrator `sub_82C3FF38`: `speed (+3412) += accel (+3408) x dt`; the accel is forced to 0
//!   while the speed is above the cap `+3688` (= (1.0 + `+3680` (+ `+3684` while held)) x f2, b57); speed
//!   and accel are zeroed when both are tiny. [`integrate`] is that step.
//! - the cap: the lane's speed limit (14.167 / 13.889 m/s [data]); the recomp shows `+3688` =
//!   14.167 on a 51 km/h road and 17.0 while skitched (`+3684` = 0.2) [trace, `npc-livingworld-re`
//!   §7d]. V3 uses the segment's limit on a lane and the exit segment's limit (at most the
//!   junction speed) on a connector.
//! - stop line (`sub_82C3FA08`): `accel = -v^2 / (2 (d - f2) + 0.001)` with `d` the distance to
//!   the line and `f2` the stop distance [code]. [`stop_accel`].
//! - a skater coming up behind (`sub_82C3FA08`, `+4403` bit 0x10 from the skater scan, limiter
//!   kind free): the car brakes toward the skater's speed minus 20 km/h and never accelerates
//!   ([`super::skater_scan`], b63; this was misread as a lead-following rule before).
//! - the junction query (`sub_82E11E90`, V1 [`junction_entry`]) runs once the stop line is within
//!   `look_ahead + speed`; Go enters, Approach slows to the connector's entry speed, Signal /
//!   Yield / Blocked hold the car at the line [code].
//! - connector choice: least loaded exit lane per metre on every car (`sub_82C376E8`, V1
//!   [`choose_connector`]) [code]; the load is the number of cars on the exit lane (the retail
//!   per-lane vec4 is not read yet, V1 open item).
//! - acceleration ramps 0.2 m/s^2 per ~0.25 s up to 2.3-3.4 m/s^2 from a stop [trace 47CAEDA0,
//!   proof1]; hard stops reach about -7.3 m/s^2 [trace 47CAEDA0, 164620].
//!
//! V3 simplifications (until V4): no look-ahead obstacles other than cars (skater, NPCs and peds
//! are V4 / V5); the look-ahead distance is the comfortable stopping distance plus one second of
//! speed (retail `+3516` is not read); the following rule below 20 km/h and the minimum gap are a
//! plain "stop `min_gap` behind the car ahead"; a car that got Go and can no longer stop
//! comfortably commits to the junction (amber dilemma zone); a car at a dead end stops there (the
//! engine despawns it); no lane changes, overtakes, horns, parking or skids. One engine-side
//! safeguard that is not retail: [`lead`] treats a car inside the junction on another connector
//! into the same exit lane, nearer the exit, as the car ahead (two lanes of one approach merging
//! into one; retail's junction query does not cover that case, its look-ahead does, V4).
//!
//! Multiplayer seams: [`step`] is a pure function of the road network, the signal clock, the cars
//! (sorted by key) and `dt`; cars update in key order and see the cars before them already moved
//! (deterministic). A client that runs the same spawn records from the same tick with the same
//! signal tick count gets the same motion; see doc 26 V3.
//!
//! Moddability: every number is a field of [`FollowParams`] (defaults from the vehicle spec
//! records or the trace values named on each field; a mod overrides per car); the connector
//! choice is the V1 [`ConnectorChoice`].

use std::collections::BTreeMap;

use super::cursor::{choose_connector, ConnectorChoice, LaneCursor, Place};
use super::graph::RoadNetwork;
use super::junction::{junction_entry, query_due, Entry, EntryQuery, Occupancy, VehicleKey, VehicleSnapshot};
use super::signals::SignalClock;
use crate::living_world::rng::Rng;

/// The follower's numbers. Every field is data a mod may override per car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowParams {
    /// Largest pull-away acceleration, m/s^2. Default: the spec field `Hash_328B9F4685A14018`
    /// (2.0 default, 3.0 family, 3.1 minivan) [data; that it is the max accel is the V0 layout
    /// candidate, open], matching the 2.3-3.4 m/s^2 the recomp reaches [trace].
    pub accel_max: f32,
    /// Acceleration ramp, m/s^3: 0.2 m/s^2 per 0.25 s [trace proof1, 47CAEDA0].
    pub jerk: f32,
    /// Comfortable braking for the look-ahead and stop planning, m/s^2. Default: the spec field
    /// `Hash_758229215579C6D1` (2.5-3.0) [data; meaning a candidate, open].
    pub plan_decel: f32,
    /// Hardest braking, m/s^2: -7.25 / -7.31 seen [trace 47CAEDA0, 164620].
    pub hard_brake: f32,
    /// Distance kept to the car ahead when stopped (rear to front), m. Engine value (retail
    /// `+3520` not read): 2.0.
    pub min_gap: f32,
    /// Stop distance before the line (`f2` of the stop-line rule), m. Engine value: 0.5.
    pub stop_margin: f32,
    /// The skater-behind rule (scan range, speeds, braking distances, release grace; b63).
    pub skater: super::skater_scan::SkaterFollowParams,
    /// The manoeuvre decider's values (spec / driver records; b69 / b74) and the lane-change passage: the spec's
    /// passage factor (`Hash_328B9F4685A14018`, spec+40, `+3692`), the road network's curve values and the spot /
    /// gap-check constants.
    pub manoeuvre: super::manoeuvre::DeciderParams,
    pub passage_factor: f32,
    pub passage: super::passage::PassageParams,
    pub spots: super::spots::SpotParams,
    /// Pull-over (b73 / b77): approach length = ext x the spec's approach factor (spec+36 `Hash_758229215579C6D1`,
    /// `+3672`), the stop slack 0.1 (`0x820641A8`), the crawl speed that starts the pull-over (1.0 m/s,
    /// `0x8231A844`) and the parked time before pulling out (driver `Hash_988BB0F6F043EB3D`, 30 s, taxi 20).
    pub approach_factor: f32,
    pub pull_over_slack: f32,
    pub pull_over_speed: f32,
    pub parked_time: f32,
    /// Multiplier on the lane cap (`f2` of the integrator's cap; 1.0 = retail; a mod or the
    /// skitch milestone raises it).
    pub cap_scale: f32,
    /// Added to the cap scale while a skater holds the car (`+3684`, 0.2 in the recomp trace; b57).
    pub held_cap_add: f32,
    /// The driver record's horn values (`livingworld_vehicle_drivers`).
    pub horn: super::horn::HornParams,
}

impl Default for FollowParams {
    /// The `default` spec record's values (`livingworld_vehicle_characteristics/default`) and
    /// the trace values; the engine fills real cars from their spec record.
    fn default() -> Self {
        Self {
            accel_max: 2.0,
            jerk: 0.8,
            plan_decel: 3.0,
            hard_brake: 7.3,
            min_gap: 2.0,
            stop_margin: 0.5,
            skater: Default::default(),
            manoeuvre: Default::default(),
            passage_factor: 2.0,
            passage: Default::default(),
            spots: Default::default(),
            approach_factor: 3.0,
            pull_over_slack: 0.1,
            pull_over_speed: 1.0,
            parked_time: 30.0,
            cap_scale: 1.0,
            held_cap_add: 0.2,
            horn: super::horn::HornParams::default(),
        }
    }
}

/// One car of the follower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Car {
    pub key: VehicleKey,
    pub cursor: LaneCursor,
    /// m/s (retail `+3412`).
    pub speed: f32,
    /// m/s^2 (retail `+3408`): the commanded acceleration of the last step.
    pub accel: f32,
    /// Length along the lane (m); the cursor is the car's centre.
    pub length: f32,
    pub params: FollowParams,
    /// The last junction answer for the chosen connector (retail junction state `+4392`).
    pub entry: Option<Entry>,
    /// The car got Go and can no longer stop comfortably: it goes through.
    pub committed: bool,
    /// An actor (the skater) hit the car ahead of it (retail `+4401` bit 0x20, `sub_82C3C150`): the
    /// planner brakes hard (`sub_82C3FA08` at `0x82C3FE44`: accel = -speed) until the car stands,
    /// then the integrator clears it (`sub_82C3FF38`).
    pub hit_brake: bool,
    /// The nearest obstacle in the look-ahead (`obstacles::nearest`; set by the host each frame, retail
    /// `sub_82C40B70` -> `+3584..+3620`).
    pub obstacle: Option<super::horn::ObstacleHit>,
    /// The rolled driver bits (`+4401` 0x01 / 0x02; the host rolls them at spawn).
    pub driver: super::horn::DriverBits,
    /// The limiter kind of the last step (`+4392`, [`super::horn::limiter`]).
    pub limiter: u8,
    pub horn_timers: super::horn::HornTimers,
    /// The horn state of the last step (`+3420`, 0 = silent) and its honked-at target (kind 2, a ped).
    pub horn: u8,
    pub honk_target: Option<u64>,
    /// A skater holds the car this tick (`+4402` bit 0x02, set by `82C361E8` from the skater's state 104) and the
    /// holder is the player (`+4403` bit 0x80: the car skips lights, `82C344D0`; b57). Host-set each tick.
    pub held: bool,
    pub player_held: bool,
    /// Held on the previous step (`+4402` bit 0x01) and the release grace (`+3728`, s; -1 = none), stepped
    /// after every car ([`super::skater_scan::grace_step`]).
    pub held_last: bool,
    pub release_grace: f32,
    /// The skater scan of this tick (`sub_82C414A8`; set by the host before [`step`], like `obstacle`).
    pub skater: super::skater_scan::SkaterScan,
    /// The manoeuvre decider (`+3632` timer, `+4396` pending) and the running lane change (`+4353`, passage block).
    pub decider: super::manoeuvre::DeciderState,
    pub passage: Option<super::passage::Passage>,
    /// Pulling over, parked, pulling out (b73); lane changes stay `Following` with a passage.
    pub manoeuvre: super::manoeuvre::Manoeuvre,
    /// The car alarm sounds (`+3424` bit 0x10; set by a contact while parked, `sub_82C3C150`, cleared by
    /// StopAlarming `82C3A4D0`). Host-set each tick from the alarm rule. While it is on, StayingParked
    /// (`82C39138`) holds the parked time `+3712` at 0 and `IsRequiredToPullOut` (`82C3A3A8`) is false.
    pub alarming: bool,
}

impl Car {
    pub fn new(key: VehicleKey, cursor: LaneCursor, length: f32, params: FollowParams) -> Self {
        Car { key, cursor, speed: 0.0, accel: 0.0, length, params, entry: None, committed: false, hit_brake: false, obstacle: None, driver: super::horn::DriverBits { horn: true, blocked_long: true }, limiter: 0, horn_timers: Default::default(), horn: 0, honk_target: None, held: false, player_held: false, held_last: false, release_grace: -1.0, skater: super::skater_scan::SkaterScan::NONE, decider: Default::default(), passage: None, manoeuvre: Default::default(), alarming: false }
    }

    /// On `place` for the lane lists: its own place, or the target lane of a running lane change (retail registers
    /// the car on the target lane when the passage starts, `82E14FC0`).
    pub fn on_place(&self, place: Place) -> bool {
        // Parked, or past the middle of the pull-over curve (`82C38C70`: progress - ext / 2 > length / 2): off the
        // outer lane, on the kerb slot.
        let off_lane = match (self.manoeuvre, self.passage) {
            (super::manoeuvre::Manoeuvre::Parked { .. }, _) => true,
            (super::manoeuvre::Manoeuvre::PullingOver { .. }, Some(p)) => p.progress - 0.25 * self.length > 0.5 * p.length(),
            _ => false,
        };
        (self.cursor.place == place && !off_lane)
            || self.passage.is_some_and(|p| matches!((self.cursor.place, place), (Place::Lane { segment: a, .. }, Place::Lane { segment: b, lane }) if a == b && lane == p.to_lane))
    }

    /// Where the car is drawn: on the lane-change curve while it runs (`sub_82C3F0C8`), else the cursor's frame.
    pub fn pose(&self, net: &RoadNetwork) -> super::graph::Frame {
        match self.passage {
            Some(p) => {
                let (position, forward) = p.sample();
                super::graph::Frame { position, forward }
            }
            None => match (self.manoeuvre, self.cursor.place) {
                // Parked on the kerb slot (lane n, one lane past the outer lane, b73).
                (super::manoeuvre::Manoeuvre::Parked { .. }, Place::Lane { segment, .. }) => net.lane_frame(segment, net.segments[segment].lanes as f32, self.cursor.distance),
                _ => self.cursor.frame(net),
            },
        }
    }

    /// Look-ahead distance (m): comfortable stopping distance (V3 stand-in for `+3516`).
    pub fn look_ahead(&self) -> f32 {
        self.speed * self.speed / (2.0 * self.params.plan_decel.max(0.1))
    }

    pub fn snapshot(&self) -> VehicleSnapshot {
        VehicleSnapshot {
            id: self.key,
            length: self.length,
            speed: self.speed,
            place: self.cursor.place,
            distance: self.cursor.distance,
            look_ahead: self.look_ahead(),
            min_gap: self.params.min_gap,
            // Mover getter 82C34598: +3424 bit 0x04 or held (+4402 bit 0x02); the entered-on-red half is open.
            flagged: self.held,
        }
    }
}

/// What one step did to a car (engine events, mod events, logs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FollowEvent {
    /// The junction answer changed (`None` = left the approach).
    Junction { key: VehicleKey, connector: usize, entry: Entry },
    /// The car entered a connector (it is inside the junction).
    EnteredJunction { key: VehicleKey, connector: usize },
    /// The car entered a lane from a connector.
    EnteredLane { key: VehicleKey, segment: usize, lane: u8 },
    /// The car stands at the end of a lane with nowhere to go.
    DeadEnd { key: VehicleKey },
    /// A lane change started (`82C3A9E0`): from lane `from` to `to` of `segment`.
    LaneChange { key: VehicleKey, segment: usize, from: u8, to: u8 },
    /// The car pulls over to its spot (`82C3ACF8`) / pulls out again (`82C3AFB8`).
    PullingOver { key: VehicleKey, segment: usize, spot: f32 },
    PullingOut { key: VehicleKey, segment: usize },
}

/// `sub_82C3FF38`: one integrator step. Returns the new (speed, accel).
pub fn integrate(speed: f32, accel: f32, cap: f32, dt: f32) -> (f32, f32) {
    let accel = if speed > cap && accel > 0.0 { 0.0 } else { accel };
    let mut v = speed + accel * dt;
    if v < 0.0 {
        v = 0.0;
    }
    if accel.abs() < 1e-3 && v < 1e-3 {
        return (0.0, 0.0);
    }
    (v, accel)
}

/// Stop-line braking (`sub_82C3FA08`): `-v^2 / (2 (d - f2) + 0.001)`; `d - f2 <= 0` asks for an
/// instant stop (`-v / dt` in the integrator's terms; the caller clamps).
pub fn stop_accel(speed: f32, distance: f32, margin: f32) -> f32 {
    let d = distance - margin;
    if d <= 0.0 {
        return f32::NEG_INFINITY;
    }
    -(speed * speed) / (2.0 * d + 0.001)
}

/// Braking to reach `target` m/s at `distance` (the Approach answer and the following rule).
pub fn reach_accel(speed: f32, target: f32, distance: f32) -> f32 {
    if speed <= target {
        return f32::INFINITY;
    }
    if distance <= 0.0 {
        return f32::NEG_INFINITY;
    }
    (target * target - speed * speed) / (2.0 * distance + 0.001)
}

/// Following a lead `gap` metres ahead (rear of the lead to the front of this car): brake to the
/// lead's speed by `min_gap` behind it. NOT RETAIL YET: retail's lead handling lives in the
/// planner's standoff part (`sub_82C41120`); `+3756` / `+3760` / `+3728` belong to the skater
/// scan, not to a lead (b63).
pub fn follow_accel(p: &FollowParams, speed: f32, lead_speed: f32, gap: f32) -> f32 {
    let gap = gap - p.min_gap;
    if gap <= 0.0 {
        return f32::NEG_INFINITY;
    }
    reach_accel(speed, lead_speed, gap)
}

/// The speed cap of the car's current place.
pub fn cap(net: &RoadNetwork, place: Place) -> f32 {
    match place {
        Place::Lane { segment, .. } => net.segments[segment].speed_limit,
        Place::Connector { connector } => {
            let c = &net.connectors[connector];
            let j = net.junctions[c.junction].speed;
            match net.connector_exit(connector) {
                Some(s) => net.segments[s].speed_limit.min(if j > 0.0 { j } else { f32::MAX }),
                None => j,
            }
        }
    }
}

/// Cars per place, sorted by distance (rearmost first). Rebuilt each step so the occupancy lists
/// follow the cars' order on the lane (retail keeps them newest first, which is the same order
/// for cars that entered from the lane start).
fn places(cars: &[Car]) -> BTreeMap<Place, Vec<(f32, usize)>> {
    let mut m: BTreeMap<Place, Vec<(f32, usize)>> = BTreeMap::new();
    for (i, c) in cars.iter().enumerate() {
        m.entry(c.cursor.place).or_default().push((c.cursor.distance, i));
        if let (Some(p), Place::Lane { segment, .. }) = (c.passage, c.cursor.place) {
            m.entry(Place::Lane { segment, lane: p.to_lane }).or_default().push((c.cursor.distance, i));
        }
    }
    for v in m.values_mut() {
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(cars[a.1].key.cmp(&cars[b.1].key)));
    }
    m
}

/// The occupancy lists of the cars (lane and connector lists, rearmost first).
pub fn occupancy(cars: &[Car]) -> Occupancy {
    let mut occ = Occupancy::default();
    // enter_* insert at the front: feed front-most first so the rearmost ends up first.
    for (place, list) in places(cars) {
        for &(_, i) in list.iter().rev() {
            match place {
                Place::Lane { segment, lane } => occ.enter_lane(segment, lane, cars[i].key),
                Place::Connector { connector } => occ.enter_connector(connector, cars[i].key),
            }
        }
    }
    occ
}

/// The car ahead of car `i` along its path (same place, then its chosen connector, then that
/// connector's exit lane; within `range` m): (index, gap rear-to-front, lead speed).
pub fn lead(net: &RoadNetwork, cars: &[Car], i: usize, range: f32) -> Option<(usize, f32, f32)> {
    let me = &cars[i];
    let front = me.cursor.distance + me.length * 0.5;
    let scan = |place: Place, offset: f32, same: bool| -> Option<(usize, f32, f32)> {
        let mut best: Option<(usize, f32, f32)> = None;
        for (j, o) in cars.iter().enumerate() {
            if j == i || !o.on_place(place) {
                continue;
            }
            if same && !(o.cursor.distance > me.cursor.distance || (o.cursor.distance == me.cursor.distance && o.key > me.key)) {
                continue;
            }
            let gap = offset + o.cursor.distance - o.length * 0.5 - front;
            if gap <= range && best.is_none_or(|b| gap < b.1) {
                best = Some((j, gap, o.speed));
            }
        }
        best
    };
    let span = me.cursor.span(net);
    let ahead = scan(me.cursor.place, 0.0, true).or_else(|| match me.cursor.place {
        Place::Lane { .. } => {
            let c = me.cursor.next?;
            scan(Place::Connector { connector: c }, span, false).or_else(|| {
                let exit = net.connector_exit(c)?;
                scan(Place::Lane { segment: exit, lane: net.connectors[c].to_lane }, span + net.connectors[c].length(), false)
            })
        }
        Place::Connector { connector } => {
            let exit = net.connector_exit(connector)?;
            scan(Place::Lane { segment: exit, lane: net.connectors[connector].to_lane }, span, false)
        }
    });
    // Merging: a car inside the junction on another connector into the same exit lane that is
    // closer to the exit point goes first. ENGINE SAFEGUARD, NOT RETAIL: retail's junction query
    // never scans the car's own approach (`sub_82E11E90` passes only the ends from_end + 1, + 2
    // and + 3 (r28 / [r1+80] / r16) to the merge scan `sub_82E11C78` and the crossing scan
    // `sub_82E11980`; its sibling check covers connectors of the same lane only) [code], so two
    // lanes of one approach merging into one exit lane are spaced by the look-ahead / planner
    // (`sub_82C412D8`, `sub_82C3FA08`), which is V4. Replace this with the ported look-ahead then.
    let mine = match me.cursor.place {
        Place::Lane { .. } => me.cursor.next.map(|c| (c, span - me.cursor.distance + net.connectors[c].length())),
        Place::Connector { connector } => Some((connector, net.connectors[connector].length() - me.cursor.distance)),
    };
    let mut merge: Option<(usize, f32, f32)> = None;
    if let Some((c, my_left)) = mine {
        let k = &net.connectors[c];
        for (j, o) in cars.iter().enumerate() {
            let Place::Connector { connector: oc } = o.cursor.place else { continue };
            let ok = &net.connectors[oc];
            if j == i || oc == c || ok.junction != k.junction || ok.to_end != k.to_end || ok.to_lane != k.to_lane {
                continue;
            }
            let theirs = ok.length() - o.cursor.distance;
            if theirs < my_left || (theirs == my_left && o.key < me.key) {
                let gap = my_left - theirs - o.length * 0.5 - me.length * 0.5;
                if gap <= range && merge.is_none_or(|b| gap < b.1) {
                    merge = Some((j, gap, o.speed));
                }
            }
        }
    }
    match (ahead, merge) {
        (Some(a), Some(m)) => Some(if m.1 < a.1 { m } else { a }),
        (a, m) => a.or(m),
    }
}

/// One follower step of every car (`cars` sorted by key; the caller keeps them so). Cars update
/// in that order. Returns the events.
pub fn step(net: &RoadNetwork, signals: &SignalClock, cars: &mut [Car], dt: f32, choice: ConnectorChoice, rng: &mut Rng) -> Vec<FollowEvent> {
    use super::manoeuvre::{Manoeuvre, Pending};
    let mut events = Vec::new();
    if !(dt > 0.0) {
        return events;
    }
    for i in 0..cars.len() {
        let occ = occupancy(cars);
        let snaps: BTreeMap<VehicleKey, VehicleSnapshot> = cars.iter().map(|c| (c.key, c.snapshot())).collect();
        let me = cars[i];
        let p = me.params;
        let mut decider = me.decider;
        let mut manoeuvre = me.manoeuvre;
        let approach = me.length * 0.5 * p.approach_factor;
        // Parked (`82C39138`): standing, the parked time runs; `IsRequiredToPullOut` (`82C3A3A8`) after the parked
        // time when the end of the approach is still on this road: the passage back to the outer lane
        // (`82C3AFB8`: d1 = d + approach, from the kerb slot to lane n - 1; the spot is released).
        // A sounding alarm holds the parked time at 0 and blocks the pull-out (`82C39138`, `82C3A3A8`).
        if let (Manoeuvre::Parked { spot, time }, Place::Lane { segment, lane }) = (manoeuvre, me.cursor.place) {
            let time = if me.alarming { 0.0 } else { time + dt };
            let d = me.cursor.distance;
            let seg = &net.segments[segment];
            let mut passage = None;
            if !me.alarming && time > p.parked_time && d + approach < seg.length {
                let from = net.lane_frame(segment, seg.lanes as f32, d);
                let to = net.lane_frame(segment, lane as f32, d + approach);
                passage = super::passage::Passage::begin(seg.lanes, lane, d, d + approach, (from.position, from.forward), (to.position, to.forward), &p.passage);
            }
            let car = &mut cars[i];
            car.speed = 0.0;
            car.accel = 0.0;
            match passage {
                Some(pass) => {
                    car.passage = Some(pass);
                    car.manoeuvre = Manoeuvre::PullingOut;
                    events.push(FollowEvent::PullingOut { key: car.key, segment });
                }
                None => car.manoeuvre = Manoeuvre::Parked { spot, time },
            }
            continue;
        }
        // The manoeuvre decider (`sub_82C41CD0`), on a lane with no manoeuvre running.
        if let (Place::Lane { segment, lane }, None, Manoeuvre::Following) = (me.cursor.place, me.passage, manoeuvre) {
            let seg = &net.segments[segment];
            let load = |l: u8| cars.iter().filter(|c| c.on_place(Place::Lane { segment, lane: l })).map(|c| c.length).sum::<f32>();
            let gap = |l: u8| lane_change_gap(net, cars, i, segment, l);
            // Reservations: every other car on this road holding a spot (pending, pulling over or parked).
            let spot_search = || {
                let mut r = super::spots::Reservations::default();
                for (j, c) in cars.iter().enumerate() {
                    let held = match c.decider.pending {
                        Pending::PullOver { spot } => Some(spot),
                        _ => c.manoeuvre.spot(),
                    };
                    if let (true, Some(stop), Place::Lane { segment: s, .. }) = (j != i, held, c.cursor.place) {
                        if s == segment {
                            r.reserve(super::spots::Reservation { car: c.key, stop, length: c.length });
                        }
                    }
                }
                r.find_spot(seg.length, me.cursor.distance, me.length, me.speed, &p.spots)
            };
            let input = super::manoeuvre::DeciderInput {
                dt,
                lane,
                lane_count: seg.lanes,
                lane_changes_allowed: seg.manoeuvres & 0x02 != 0,
                pull_over_allowed: seg.manoeuvres & 0x01 != 0,
                held: me.held,
                at_junction: false,
                lane_load: &load,
                gap_free: &gap,
                find_spot: &spot_search,
                spot_min: me.cursor.distance + approach,
            };
            let mut unit = || rng.next_u32() as f32 * 2.328_306_4e-10_f32;
            super::manoeuvre::decide(&mut decider, &p.manoeuvre, &input, &mut unit);
        }
        let cap_now = cap(net, me.cursor.place) * (p.cap_scale + if me.held { p.held_cap_add } else { 0.0 });
        // Free road: ramp up to accel_max, never past the cap.
        let ramp = (me.accel.max(0.0) + p.jerk * dt).min(p.accel_max);
        let mut accel = ramp.min((cap_now - me.speed) / dt);
        // Stop line / junction.
        let mut hold_at: Option<f32> = None;
        // The limiter kind and the nearest limit (`+4392`, best distance; horn.rs).
        let mut kind = super::horn::limiter::FREE;
        let mut best = f32::INFINITY;
        let mut entry = me.entry;
        let mut committed = me.committed;
        if let (Place::Lane { segment, .. }, Some(c)) = (me.cursor.place, me.cursor.next) {
            let to_line = net.segments[segment].length - (me.cursor.distance + me.length * 0.5);
            let snap = me.snapshot();
            if query_due(&snap, to_line) || to_line <= p.min_gap {
                let info = junction_entry(&EntryQuery { net, signals, occupancy: &occ, vehicles: &snaps, me: &snap, connector: c, check_lights: !me.player_held });
                if entry != Some(info.entry) {
                    events.push(FollowEvent::Junction { key: me.key, connector: c, entry: info.entry });
                }
                entry = Some(info.entry);
                // FollowingLane stores the answer in the same field as the limiter kind (`+4392`: 1 signal,
                // 2 approach, 3 yield, 4 blocked, 5 a yield to a flagged car); a car behind one waiting at a light
                // (1) or a flagged yield (5) counts as waiting itself and does not get the blocked horn.
                if info.entry != Entry::Go {
                    kind = if info.blocker_flagged { super::horn::limiter::JUNCTION_WAIT } else { info.entry as u8 };
                    best = to_line;
                }
                let comfortable = me.speed * me.speed / (2.0 * p.plan_decel.max(0.1));
                let hard = me.speed * me.speed / (2.0 * p.hard_brake.max(0.1));
                match info.entry {
                    Entry::Go => {
                        if comfortable >= to_line - p.stop_margin {
                            committed = true;
                        }
                    }
                    Entry::Approach if !committed => {
                        accel = accel.min(reach_accel(me.speed, net.connectors[c].entry_speed, to_line - p.stop_margin));
                    }
                    _ if !committed => {
                        if hard > to_line - p.stop_margin + 0.25 && me.speed > 0.5 {
                            // Cannot stop any more: go through (dilemma zone).
                            committed = true;
                        } else {
                            accel = accel.min(stop_accel(me.speed, to_line, p.stop_margin));
                            hold_at = Some(net.segments[segment].length - me.length * 0.5 - p.stop_margin.min(to_line.max(0.0)));
                        }
                    }
                    _ => {}
                }
            }
        }
        // The car ahead.
        let range = me.look_ahead() + me.speed + p.min_gap + 10.0;
        let mut limit: Option<f32> = None; // max distance the centre may reach along the current place
        let mut lead_close = None;
        if let Some((l, gap, lead_speed)) = lead(net, cars, i, range) {
            accel = accel.min(follow_accel(&p, me.speed, lead_speed, gap));
            // `sub_82C41120`: inside one second of travel plus the standoff the lead limits; "close" = inside
            // the standoff (ours: our follower settles at `min_gap`, so the stop margin is the tolerance).
            lead_close = Some(gap <= p.min_gap + p.stop_margin);
            if gap <= me.speed + p.min_gap && gap < best {
                best = gap;
                use super::horn::limiter::{BEHIND_LEAD, BEHIND_WAITING_LEAD, JUNCTION_WAIT};
                kind = if matches!(cars[l].limiter, BEHIND_WAITING_LEAD | JUNCTION_WAIT) { BEHIND_WAITING_LEAD } else { BEHIND_LEAD };
            }
            // Never closer than half the minimum gap (no overlaps whatever the braking).
            limit = Some(me.cursor.distance + (gap - p.min_gap * 0.5).max(0.0));
        }
        // A pending pull-over (`82C376E8`, b77): the spot is a stop target (kind 2, standoff = the approach length).
        if let (Pending::PullOver { spot }, Manoeuvre::Following) = (decider.pending, manoeuvre) {
            if let Some(x) = super::manoeuvre::pull_over_stop(spot, me.cursor.distance, me.speed, approach, p.pull_over_slack) {
                if x < best {
                    best = x;
                    kind = super::horn::limiter::STOP_POINT;
                    accel = accel.min(stop_accel(me.speed, x, approach));
                }
            }
        }
        // Pulling over (`82C38C70`): once the car would pass the curve's end within a second, it brakes to stop there.
        if let (Manoeuvre::PullingOver { .. }, Some(pass)) = (manoeuvre, me.passage) {
            let rem = (pass.length() - pass.progress).max(0.0);
            if me.speed > rem {
                best = rem;
                kind = super::horn::limiter::STOP_POINT;
                accel = accel.min(stop_accel(me.speed, rem, 0.0));
            }
        }
        // A skater coming up behind (`sub_82C3FA08`, limiter kind still free; the skater scan ran
        // before the step). Engine choice: the result is a min with our lead / junction terms,
        // which our planner folds into the same pass (retail keeps them apart by the kind).
        if kind == super::horn::limiter::FREE {
            if let Some(a) = super::skater_scan::skater_follow(&p.skater, me.speed, &me.skater, me.release_grace, accel) {
                accel = accel.min(a);
            }
        }
        // The obstacle ahead (`sub_82C412D8` -> `sub_82C3FA08`, standoff = the car's min gap).
        let accel = match me.obstacle.and_then(|o| super::obstacles::obstacle_accel(me.speed, o.distance, p.min_gap)) {
            Some(a) => accel.min(a),
            None => accel,
        };
        if me.obstacle.is_some_and(|o| o.distance < best) {
            kind = super::horn::limiter::OBSTACLE;
        }
        // `IsRequiredToChangeLane` (`82C39F78`, main read): pass = ext x passage factor + speed, h = pass / 2; the car
        // is faster than h, the free distance ahead (`+3752`) exceeds h, d > ext, d + pass ends before the road end
        // minus ext and the target lane passes the gap check. Then the passage starts (`82C3A9E0` / `82C3F540`).
        let mut passage = me.passage;
        // `IsRequiredToPullOver` (`82C3A270`, b77): the passage to the kerb slot (lane n, `82C3ACF8`: d1 = the spot).
        if let (Pending::PullOver { spot }, Place::Lane { segment, lane }, Manoeuvre::Following) = (decider.pending, me.cursor.place, manoeuvre) {
            let seg = &net.segments[segment];
            let d = me.cursor.distance;
            if spot <= seg.length && super::manoeuvre::is_required_to_pull_over(spot, d, me.speed, approach, p.pull_over_speed) {
                let from = net.lane_frame(segment, lane as f32, d);
                let to = net.lane_frame(segment, seg.lanes as f32, spot);
                if let Some(pass) = super::passage::Passage::begin(lane, seg.lanes, d, spot, (from.position, from.forward), (to.position, to.forward), &p.passage) {
                    passage = Some(pass);
                    manoeuvre = Manoeuvre::PullingOver { spot };
                    decider.pending = Pending::None;
                    events.push(FollowEvent::PullingOver { key: me.key, segment, spot });
                }
            }
        }
        if let (super::manoeuvre::Pending::LaneChange { lane: target }, Place::Lane { segment, lane }) = (decider.pending, me.cursor.place) {
            let ext = me.length * 0.5;
            let pass = ext * p.passage_factor + me.speed;
            let h = 0.5 * pass;
            let d = me.cursor.distance;
            let d1 = d + pass;
            let seg_len = net.segments[segment].length;
            if me.speed > h && best > h && d > ext && d1 < seg_len - ext && lane_change_gap(net, cars, i, segment, target) {
                let from = net.lane_frame(segment, lane as f32, d);
                let to = net.lane_frame(segment, target as f32, d1);
                passage = super::passage::Passage::begin(lane, target, d, d1, (from.position, from.forward), (to.position, to.forward), &p.passage);
                if passage.is_some() {
                    decider.pending = super::manoeuvre::Pending::None;
                    events.push(FollowEvent::LaneChange { key: me.key, segment, from: lane, to: target });
                }
            }
        }
        // The horn timers and decider (`sub_82C41120`, `sub_82C412D8`, `sub_82C40660`).
        let mut timers = me.horn_timers;
        timers.update(&p.horn, kind, lead_close, me.obstacle.is_some(), me.speed, dt);
        let (horn, honk_target) = super::horn::decide(&p.horn, me.driver, kind, &timers, me.obstacle, me.speed);
        let accel = if me.hit_brake { accel.min(-me.speed) } else { accel };
        let accel = accel.max(-p.hard_brake * 4.0).max(-me.speed / dt);
        let (mut speed, accel) = integrate(me.speed, accel, cap_now, dt);
        let mut ds = speed * dt;
        let mut target = me.cursor.distance + ds;
        if let Some(h) = hold_at {
            if target > h {
                target = h.max(me.cursor.distance);
            }
        }
        if let Some(l) = limit {
            if target > l {
                target = l.max(me.cursor.distance);
            }
        }
        if target < me.cursor.distance + ds {
            ds = (target - me.cursor.distance).max(0.0);
            if ds <= speed * dt * 0.5 {
                speed = (ds / dt).min(speed);
            }
        }
        let car = &mut cars[i];
        car.speed = speed;
        car.accel = accel;
        car.decider = decider;
        car.manoeuvre = manoeuvre;
        if speed <= 0.0 {
            car.hit_brake = false;
        }
        car.entry = entry;
        car.committed = committed;
        car.limiter = kind;
        car.horn_timers = timers;
        car.horn = horn;
        car.honk_target = honk_target;
        let loads = &occ;
        let mut choose = |n: &RoadNetwork, s: usize, l: u8| choose_connector(n, s, l, choice, &|seg, lane| loads.lane_load(seg, lane), rng);
        // A running lane change moves the car along its passage (`82C3C3C0`, 60 Hz steps) instead of the cursor;
        // the passage ends before the road end, so no place change happens meanwhile.
        if let Some(mut pass) = passage {
            let done = pass.advance(speed, false, dt);
            car.cursor.distance = pass.distance();
            if done {
                car.passage = None;
                match car.manoeuvre {
                    // `ToStayingParked` (`82C3AE78`): standing at the kerb, the spot stays reserved.
                    Manoeuvre::PullingOver { spot } => {
                        car.manoeuvre = Manoeuvre::Parked { spot, time: 0.0 };
                        car.speed = 0.0;
                        car.accel = 0.0;
                    }
                    // Back on the outer lane (`FromChangingLaneToFollowingLane`).
                    Manoeuvre::PullingOut => car.manoeuvre = Manoeuvre::Following,
                    _ => {
                        car.cursor.set_lane(net, pass.to_lane, &mut choose);
                        if let Place::Lane { segment, lane } = car.cursor.place {
                            events.push(FollowEvent::EnteredLane { key: car.key, segment, lane });
                        }
                    }
                }
            } else {
                car.passage = Some(pass);
            }
            continue;
        }
        let adv = car.cursor.advance(net, ds, &mut choose);
        for place in adv.entered {
            match place {
                Place::Connector { connector } => {
                    car.entry = None;
                    car.committed = false;
                    events.push(FollowEvent::EnteredJunction { key: car.key, connector });
                }
                Place::Lane { segment, lane } => events.push(FollowEvent::EnteredLane { key: car.key, segment, lane }),
            }
        }
        if adv.at_dead_end {
            car.speed = 0.0;
            car.accel = 0.0;
            events.push(FollowEvent::DeadEnd { key: car.key });
        }
    }
    // `sub_82C34B30` after the car loop: the release grace, then the held edge for the next step.
    for car in cars.iter_mut() {
        car.release_grace = super::skater_scan::grace_step(car.release_grace, car.held_last, car.held, dt, &car.params.skater);
        car.held_last = car.held;
    }
    events
}

/// `sub_82E14928` for car `i` onto `lane` of `segment`: ext = half the car's length (inferred, b71), the cars on
/// that lane (incl. those changing into it) by distance.
fn lane_change_gap(net: &RoadNetwork, cars: &[Car], i: usize, segment: usize, lane: u8) -> bool {
    let me = &cars[i];
    let seg = &net.segments[segment];
    let mut list: Vec<super::spots::LaneCar> = cars
        .iter()
        .enumerate()
        .filter(|(j, c)| *j != i && c.on_place(Place::Lane { segment, lane }))
        .map(|(_, c)| super::spots::LaneCar { distance: c.cursor.distance, length: c.length, speed: c.speed })
        .collect();
    list.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    super::spots::gap_free(seg.length, seg.lanes >= 2 && lane < seg.lanes, me.cursor.distance, me.length * 0.5, me.speed, &list, &me.params.spots)
}

#[cfg(test)]
#[path = "follow_tests.rs"]
mod tests;
