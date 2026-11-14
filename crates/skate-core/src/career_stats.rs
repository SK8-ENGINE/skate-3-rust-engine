//! Career stats (retail's Stats object, `[0x83067094]`): one running total per row of the career stats table, the
//! loading screen's Stats panel pick and the value formats. Details: docs/hails-additions/31-retail-menus.md
//! "Career stats", research b589.

/// Rows in the retail table (TU3: 65).
pub const ROWS: usize = 65;

/// Row indices the engine writes (retail's index order).
pub mod row {
    pub const GENDER_SWITCHES: usize = 2;
    pub const WIPEOUTS: usize = 6;
    pub const DISTANCE: usize = 7;
    pub const TIME_PLAYED: usize = 8;
    pub const DISTANCE_OFFBOARD: usize = 9;
    pub const INVERT_TIME: usize = 10;
    pub const SKITCH_DISTANCE: usize = 11;
    pub const SKITCH_TIME: usize = 12;
    pub const GRIND_DISTANCE: usize = 13;
    pub const FLIP_TRICKS: usize = 15;
    pub const BODY_FLIPS: usize = 16;
    pub const BODY_FLIP_ATTEMPTS: usize = 17;
    pub const LATE_FLIPS: usize = 18;
    pub const PUSHES: usize = 19;
    pub const MONGO_PUSHES: usize = 20;
    pub const MANUAL_DISTANCE: usize = 21;
    pub const POWERSLIDE_DISTANCE: usize = 22;
    pub const UNSUCCESSFUL_TRICKS: usize = 23;
    pub const SUCCESSFUL_TRICKS: usize = 24;
    pub const BONES_BROKEN: usize = 3;
    pub const HEAD_HITS: usize = 4;
    pub const GROIN_HITS: usize = 5;
    pub const PEDS_HIT: usize = 26;
    pub const BAIL_GROUND_TIME: usize = 36;
    pub const BAIL_AIR_TIME: usize = 37;
    pub const BAIL_GROUND_DISTANCE: usize = 38;
    pub const BAIL_AIR_DISTANCE: usize = 39;
    pub const BODY_TWEAKS: usize = 34;
    pub const BARREL_ROLLS: usize = 41;
    pub const CANNONBALL_ROLLS: usize = 42;
    pub const JUDO_KICKS: usize = 43;
    pub const SPREAD_EAGLE_TIME: usize = 44;
    pub const VEHICLES_HIT: usize = 28;
    pub const BAIL_DISTANCE: usize = 29;
    pub const DISLOCATIONS: usize = 30;
    pub const SPRAINS: usize = 31;
    pub const BRUISES: usize = 32;
    pub const SCRAPES: usize = 33;
    pub const PUSH_OFFS: usize = 40;
    pub const TIME_IN_BAIL: usize = 45;
    pub const MOST_COLLIDED: usize = 46;
    /// Hits per ped type, in this order: bum, jock, teen girl, teen guy, man, woman, business man, business woman,
    /// male skater, female skater, male tourist, female tourist, security guard, granny.
    pub const PED_HITS: usize = 47;
    pub const PED_TYPES: usize = 14;
    pub const SPOT_TUTORIALS: usize = 61;
    pub const BOARDS_OWNED: usize = 64;
}

/// How a row's value shows (the formatter vt+0x14 = 0x8250D6D0 switches on the index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Seconds as `hh:mm:ss.ff` with leading zero fields left out, then " s".
    Time,
    /// Meters, cut to whole meters, through the 1-decimal distance template.
    DistanceWhole,
    /// Meters through the 1-decimal distance template.
    Distance,
    /// Row 17: the successes (row 16) over the attempts (this row), whole percent.
    Percent,
    /// Row 46: the ped type hit most (rows 47..60).
    PedType,
    Money,
    Integer,
}

pub fn kind(index: usize) -> Kind {
    match index {
        0 | 8 | 10 | 12 | 14 | 36 | 37 | 43 | 45 => Kind::Time,
        7 | 9 => Kind::DistanceWhole,
        11 | 13 | 21 | 22 | 29 | 38 | 39 => Kind::Distance,
        17 => Kind::Percent,
        46 => Kind::PedType,
        62 | 63 => Kind::Money,
        _ => Kind::Integer,
    }
}

/// The running totals (career, per player). Every write adds, except the rows retail sets (61, 64).
#[derive(Clone, Debug, PartialEq)]
pub struct CareerStats {
    pub values: Vec<f32>,
}

impl Default for CareerStats {
    fn default() -> Self {
        Self { values: vec![0.0; ROWS] }
    }
}

impl CareerStats {
    /// The setter vt+0x18 (0x8250DFF0): adds `value` to the row, sets rows 61 and 64.
    pub fn record(&mut self, index: usize, value: f32) {
        if !value.is_finite() {
            return;
        }
        if self.values.len() < ROWS {
            self.values.resize(ROWS, 0.0);
        }
        if let Some(v) = self.values.get_mut(index) {
            if matches!(index, row::SPOT_TUTORIALS | row::BOARDS_OWNED) {
                *v = value;
            } else {
                *v += value;
            }
        }
    }

    pub fn value(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The ped type hit most (row 46): ties keep the earlier type, the scan runs jock .. security guard, then bum,
    /// then granny (0x8250D6D0 case 0x2E). None when no ped was hit.
    pub fn most_collided(&self) -> Option<usize> {
        let order = (1..row::PED_TYPES - 1).chain([0, row::PED_TYPES - 1]);
        let mut best: Option<(usize, f32)> = None;
        for t in order {
            let v = self.value(row::PED_HITS + t);
            if v > 0.0 && best.is_none_or(|(_, b)| b < v) {
                best = Some((t, v));
            }
        }
        best.map(|(t, _)| t)
    }

    /// The offline fetch (sub_82607840): enabled rows with a value other than 0, shuffled, at most 16.
    pub fn fetch(&self, enabled: &[bool], rng: &mut dyn FnMut() -> u32) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..ROWS.min(enabled.len())).filter(|&i| enabled[i] && self.value(i) != 0.0).collect();
        shuffle(&mut rows, rng);
        rows.truncate(16);
        rows
    }
}

/// One bail's Hall of Meat record (the tracker 0x82DAF1D8, copied to P+0x3030 and added to the rows at bail end by
/// 0x8250F820). Retail counts 1/60 s ticks; ours adds game time, so the totals match at any fixed rate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BailRecord {
    /// Ticks with the body not yet at rest (wipeout over_599 clear): row 45, time in bail.
    pub active_seconds: f32,
    /// Ticks with the wipeout response strength (+580) above 0, at rest or not: row 40, push offs.
    pub push_off_ticks: f32,
    /// Distinct peds / vehicles the body touched (the scorers 0x82DAF948 / 0x82DAFBC0 refuse a 9th).
    pub peds: Vec<u64>,
    pub vehicles: Vec<u64>,
    /// Finished airs (no bone touching), s; an air still open at bail end is not added (0x82DAEBC0).
    pub air_seconds: f32,
    air_open: f32,
    /// Finished airs' fall height (apex less the height at the last air tick, 0x82DAECB8), m, and the open air's.
    pub air_distance: f32,
    air_open_distance: f32,
    air_apex: Option<f32>,
    /// The skeleton center of mass's path on ground ticks (|velocity| x time), m.
    pub ground_distance: f32,
    /// The bones' injury levels (0x82DAE9A8 / 0x82DAF1D8), when the tuning is loaded.
    pub injuries: crate::hall_of_meat::Injuries,
    pub rotation: crate::hall_of_meat::Rotation,
    pub poses: crate::hall_of_meat::Poses,
    /// New entity hits while the pose is the judo kick (M+0x878).
    pub judo_kicks: u32,
    /// Each finished air's (time s, fall m): the score applies its graphs per air (0x82DAECB8 / 0x82DAEBC0).
    pub airs: Vec<(f32, f32)>,
    /// |body velocity| on the bail's first tick (M+0x180, latched by 0x82DAD2C0).
    pub start_speed: Option<f32>,
    /// The most rotation the bail reached (M+0x820), degrees.
    pub max_rotation: f32,
    /// The response strength summed over push-off ticks (M+0x82C), per 1/60 s tick.
    pub push_off_strength: f32,
}

/// One tick of the bail for [`BailRecord::tick`].
pub struct BailTick<'a> {
    pub dt: f32,
    /// The wipeout's body at rest (over_599).
    pub over: bool,
    /// Retail cVar2, PhysOut_Air +452 (our `use_air_reckoning_452`): stops the counters but push offs.
    pub frozen: bool,
    pub response_strength: f32,
    pub airborne: bool,
    /// The skeleton center of mass height and speed (retail `[skater+0x24]`, the system reckoning).
    pub centre: (f32, f32),
    /// The hips (physical part 23) rows 0..2 (retail `[skater+0x14]` +0x30 / +0x50 / +0x60).
    pub hips: [[f32; 4]; 3],
    /// The wipeout gesture stick (PhysOut_Trick +116 / +120).
    pub stick: [f32; 2],
    pub poses: Option<&'a crate::hall_of_meat::PoseTable>,
}

impl BailRecord {
    pub const ENTITY_CAP: usize = 8;
    pub const TICK_RATE: f32 = 60.0;

    pub fn tick(&mut self, t: &BailTick, peds: impl IntoIterator<Item = u64>, vehicles: impl IntoIterator<Item = u64>) {
        let (dt, counting) = (t.dt, !t.frozen);
        let (height, speed) = t.centre;
        self.start_speed.get_or_insert(speed);
        let new_air = t.airborne && self.air_apex.is_none();
        if new_air {
            self.poses.air_start();
        }
        if t.airborne {
            if counting {
                self.air_open += dt;
            }
            let apex = self.air_apex.map_or(height, |a| a.max(height));
            self.air_apex = Some(apex);
            self.air_open_distance = apex - height;
            if let Some(table) = t.poses {
                self.poses.air_tick(table, t.stick, self.rotation.total, t.frozen, t.dt);
            }
        } else {
            if self.air_apex.is_some() {
                self.airs.push((self.air_open, self.air_open_distance));
            }
            self.air_seconds += std::mem::take(&mut self.air_open);
            self.air_distance += std::mem::take(&mut self.air_open_distance);
            self.air_apex = None;
            if counting {
                self.ground_distance += speed * dt;
            }
        }
        self.rotation.tick(&t.hips, counting && !t.over);
        self.max_rotation = self.max_rotation.max(self.rotation.total);
        if counting && !t.over {
            self.active_seconds += dt;
        }
        if t.response_strength > 0.0 {
            self.push_off_ticks += dt * Self::TICK_RATE;
            self.push_off_strength += t.response_strength * dt * Self::TICK_RATE;
        }
        let mut new_hits = 0;
        for (list, ids) in [(&mut self.peds, peds.into_iter().collect::<Vec<_>>()), (&mut self.vehicles, vehicles.into_iter().collect())] {
            for id in ids {
                if list.len() < Self::ENTITY_CAP && !list.contains(&id) {
                    list.push(id);
                    new_hits += 1;
                }
            }
        }
        if self.poses.judo() {
            self.judo_kicks += new_hits;
        }
    }

    /// The Hall of Meat score at bail close (0x82DADC48 = damage 0x82DAD448 + bonus 0x82DADA30, b880 §1.3 / §1.4).
    /// Each graph caps itself; parts truncate to integers, the body tweak graphs summed as floats first. The open
    /// air counts too. NOT RETAIL YET: the alternate rotation graph (skater mode 2 / 7, M+0x8B5) and the entity points.
    pub fn score(&self, tuning: Option<&crate::hall_of_meat::Tuning>, scoring: &crate::hall_of_meat::HomScoring) -> crate::hall_of_meat::BailScore {
        use crate::hall_of_meat::graph as k;
        let g = |slot: usize, x: f32| scoring.graphs[slot].eval(x);
        let damage = tuning.map_or(0, |t| {
            t.bones.iter().zip(self.injuries.level).filter_map(|(b, l)| usize::try_from(l).ok().and_then(|l| b.levels.get(l)))
                .map(|(points, _)| *points).sum()
        });
        let rolls = |pose: usize| ((self.poses.rotation[pose] + 90.0) / 360.0).floor().max(0.0);
        let body = (g(k::JUDO_KICKS, self.judo_kicks as f32) + g(k::SWAN_DIVE, self.poses.time[1]) + g(k::BARREL, rolls(3))
            + g(k::CANNONBALL, rolls(0))) as u32 + self.poses.points;
        let rotation = g(k::ROTATION, self.max_rotation) as u32;
        let start_speed = g(k::START_SPEED, self.start_speed.unwrap_or(0.0)) as u32;
        let open = self.air_apex.map(|_| (self.air_open, self.air_open_distance));
        let air = self.airs.iter().copied().chain(open).map(|(time, fall)| g(k::AIR_TIME, time) as u32 + g(k::AIR_FALL, fall) as u32).sum();
        let time = g(k::TIME_IN_BAIL, self.active_seconds) as u32;
        let push_off = g(k::PUSH_OFF, self.push_off_strength) as u32;
        let total = damage + body + rotation + start_speed + air + time + push_off;
        crate::hall_of_meat::BailScore { total, damage, body, rotation, start_speed, air, time, push_off, hits: 0 }
    }

    /// The rows 0x8250F820 adds at bail end; the injury counts come from 0x82510990.
    pub fn rows(&self) -> [(usize, f32); 20] {
        // Rolls: floor((degrees rotated in the pose + 90) / 360); barrel = pose 3, cannonball = pose 0 (0x8250F820).
        let rolls = |pose: usize| ((self.poses.rotation[pose] + 90.0) / 360.0).floor().max(0.0);
        let c = self.injuries.counts();
        [
            (row::BONES_BROKEN, c.broken as f32),
            (row::HEAD_HITS, f32::from(u8::from(self.injuries.head_hit()))),
            (row::GROIN_HITS, f32::from(u8::from(self.injuries.groin_hit()))),
            (row::DISLOCATIONS, c.dislocations as f32),
            (row::SPRAINS, c.sprains as f32),
            (row::BRUISES, c.bruises as f32),
            (row::SCRAPES, c.scrapes as f32),
            (row::TIME_IN_BAIL, self.active_seconds),
            (row::PUSH_OFFS, self.push_off_ticks.round()),
            (row::PEDS_HIT, self.peds.len() as f32),
            (row::VEHICLES_HIT, self.vehicles.len() as f32),
            // Row 36: the time in bail less the air time (0x8250F820).
            (row::BAIL_GROUND_TIME, (self.active_seconds - self.air_seconds).max(0.0)),
            (row::BAIL_AIR_TIME, self.air_seconds),
            (row::BAIL_GROUND_DISTANCE, self.ground_distance),
            (row::BAIL_AIR_DISTANCE, self.air_distance),
            (row::BODY_TWEAKS, self.poses.tweaks as f32),
            (row::BARREL_ROLLS, rolls(3)),
            (row::CANNONBALL_ROLLS, rolls(0)),
            (row::JUDO_KICKS, self.judo_kicks as f32),
            (row::SPREAD_EAGLE_TIME, self.poses.time[1]),
        ]
    }
}

/// Push starts (0x82DB6EC0 pulses Q+0xA4 / Q+0xA3, 0x82775328 adds rows 19 / 20). While the state word +2468 has the
/// pushing bit 25, the push is a mongo push when bit 26 equals (regular stance == riding switch), else a regular one;
/// each kind counts on its rising edge, a mongo start first. The last tick's kinds are retail's flags_1296 bits 22 / 23.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushLatch {
    mongo: bool,
    regular: bool,
}

impl PushLatch {
    /// The row to add 1 to this tick, if a push started.
    pub fn update(&mut self, flags_2468: u32, regular_stance: bool, riding_switch: bool) -> Option<usize> {
        let (mut mongo, mut regular) = (false, false);
        if flags_2468 & (1 << 25) != 0 {
            if (flags_2468 & (1 << 26) != 0) == (regular_stance == riding_switch) {
                mongo = true;
            } else {
                regular = true;
            }
        }
        let started = if !self.mongo && mongo {
            Some(row::MONGO_PUSHES)
        } else if !self.regular && regular {
            Some(row::PUSHES)
        } else {
            None
        };
        *self = Self { mongo, regular };
        started
    }
}

/// Fisher-Yates with the tips RNG (retail's own shuffle order is not decoded; the outcome, a uniform order, is).
pub fn shuffle(rows: &mut [usize], rng: &mut dyn FnMut() -> u32) {
    for i in (1..rows.len()).rev() {
        let j = rng() as usize % (i + 1);
        rows.swap(i, j);
    }
}

/// Seconds in the default time format `hh:mm:ss.ff` (0x8290EB38): a field shows when it or an earlier field is not
/// zero, padded to its width; the hundredths show only without hours.
pub fn clock(seconds: f32) -> String {
    let total = (seconds.max(0.0) * 100.0) as u64;
    let (h, m, s, f) = (total / 360_000, total / 6_000 % 60, total / 100 % 60, total % 100);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h:02}:"));
    }
    if m > 0 || !out.is_empty() {
        out.push_str(&format!("{m:02}:"));
    }
    if s > 0 || f > 0 || !out.is_empty() {
        out.push_str(&format!("{s:02}"));
        if h == 0 {
            out.push_str(&format!(".{f:02}"));
        }
    }
    out
}

/// The `distance` text type (0x824FE848): one decimal and the meter word, or under imperial units the value times 3.0
/// (retail's factor) and the feet word.
pub fn distance_text(metres: f64, imperial: bool, float: &dyn Fn(f64, usize) -> String, metres_word: &str, feet_word: &str) -> String {
    if imperial {
        format!("{} {}", float(metres * 3.0, 1), feet_word)
    } else {
        format!("{} {}", float(metres, 1), metres_word)
    }
}

/// What a row shows: the localized pieces the caller passes in (units setting, unit words, number grouping and the
/// ped type names).
pub struct Format<'a> {
    /// Game Settings > Units: imperial (the distance handler 0x824FE848 multiplies by 3.0 and shows feet).
    pub imperial: bool,
    /// `ID_COMMON_METRIC_DISTANCE`, `ID_COMMON_METRIC_NEW_FEET`, `ID_COMMON_METRIC_TIME`.
    pub metres: &'a str,
    pub feet: &'a str,
    pub seconds: &'a str,
    pub int: &'a dyn Fn(i64) -> String,
    pub float: &'a dyn Fn(f64, usize) -> String,
    pub money: &'a dyn Fn(f64) -> String,
    /// The ped type name by type (0 bum .. 13 granny).
    pub ped_type: &'a dyn Fn(usize) -> String,
}

impl CareerStats {
    pub fn format(&self, index: usize, f: &Format) -> String {
        let v = self.value(index);
        let distance = |metres: f64| distance_text(metres, f.imperial, f.float, f.metres, f.feet);
        match kind(index) {
            Kind::Time if v <= 0.0 => format!("#0 {}", f.seconds),
            Kind::Time => format!("{} {}", clock(v), f.seconds),
            Kind::DistanceWhole => distance(v.trunc() as f64),
            Kind::Distance => distance(v as f64),
            Kind::Percent => {
                let attempts = v;
                let pct = if attempts > 0.0 { (self.value(row::BODY_FLIPS) / attempts * 100.0).floor().clamp(0.0, 100.0) } else { 0.0 };
                format!("{}%", pct as i64)
            }
            Kind::PedType => self.most_collided().map(|t| (f.ped_type)(t)).unwrap_or_default(),
            Kind::Money => (f.money)(v as f64),
            Kind::Integer => (f.int)(v as i64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> Format<'static> {
        Format {
            imperial: false,
            metres: "m",
            feet: "ft",
            seconds: "s",
            int: &|n| n.to_string(),
            float: &|x, d| format!("{x:.d$}"),
            money: &|x| format!("${x:.0}"),
            ped_type: &|t| ["Bum", "Jock"].get(t).unwrap_or(&"?").to_string(),
        }
    }

    #[test]
    fn records_add_except_the_set_rows() {
        let mut s = CareerStats::default();
        s.record(row::PUSHES, 1.0);
        s.record(row::PUSHES, 1.0);
        s.record(row::BOARDS_OWNED, 3.0);
        s.record(row::BOARDS_OWNED, 2.0);
        s.record(row::DISTANCE, f32::NAN);
        assert_eq!((s.value(row::PUSHES), s.value(row::BOARDS_OWNED), s.value(row::DISTANCE)), (2.0, 2.0, 0.0));
    }

    #[test]
    fn the_clock_drops_leading_zero_fields_and_the_hundredths_with_hours() {
        assert_eq!(clock(83.45), "01:23.45");
        assert_eq!(clock(5.2), "05.20");
        assert_eq!(clock(3723.5), "01:02:03");
        assert_eq!(clock(0.0), "");
    }

    #[test]
    fn values_format_by_kind() {
        let mut s = CareerStats::default();
        s.record(row::DISTANCE, 1234.56);
        s.record(row::GRIND_DISTANCE, 12.34);
        s.record(row::TIME_PLAYED, 83.45);
        s.record(row::BODY_FLIPS, 2.0);
        s.record(row::BODY_FLIP_ATTEMPTS, 3.0);
        s.record(row::PED_HITS + 1, 2.0);
        s.record(row::PED_HITS, 2.0);
        let f = fmt();
        assert_eq!(s.format(row::DISTANCE, &f), "1234.0 m");
        assert_eq!(s.format(row::GRIND_DISTANCE, &f), "12.3 m");
        assert_eq!(s.format(row::TIME_PLAYED, &f), "01:23.45 s");
        assert_eq!(s.format(row::INVERT_TIME, &f), "#0 s");
        assert_eq!(s.format(row::BODY_FLIP_ATTEMPTS, &f), "66%");
        assert_eq!(s.format(row::MOST_COLLIDED, &f), "Jock", "a tie keeps the type scanned first (jock before bum)");
        assert_eq!(s.format(row::PUSHES, &f), "0");
        let imperial = Format { imperial: true, ..fmt() };
        assert_eq!(s.format(row::GRIND_DISTANCE, &imperial), "37.0 ft");
    }

    #[test]
    fn the_fetch_keeps_enabled_rows_with_values_at_most_16() {
        let mut s = CareerStats::default();
        let mut enabled = vec![true; ROWS];
        enabled[row::PUSHES] = false;
        s.record(row::PUSHES, 1.0);
        s.record(row::DISTANCE, 5.0);
        assert_eq!(s.fetch(&enabled, &mut || 0), [row::DISTANCE]);
        for i in 0..30 {
            s.record(i, 1.0);
        }
        assert_eq!(s.fetch(&enabled, &mut || 7).len(), 16);
    }

    #[test]
    fn pushes_count_on_the_rising_edge_by_kind() {
        let mut l = PushLatch::default();
        let push = 1 << 25;
        let right = 1 << 26;
        assert_eq!(l.update(0, true, true), None);
        // Bit 26 clear, stance == switch: not equal, a regular push.
        assert_eq!(l.update(push, true, true), Some(row::PUSHES));
        assert_eq!(l.update(push, true, true), None);
        assert_eq!(l.update(push | right, true, false), None);
        // The kind changes while pushing: the new kind starts.
        assert_eq!(l.update(push | right, true, true), Some(row::MONGO_PUSHES));
        assert_eq!(l.update(0, true, true), None);
        assert_eq!(l.update(push, true, false), Some(row::MONGO_PUSHES));
    }

    fn tick(dt: f32, over: bool, response_strength: f32, airborne: bool, height: f32) -> BailTick<'static> {
        let hips = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        BailTick { dt, over, frozen: false, response_strength, airborne, centre: (height, 3.0), hips, stick: [0.0; 2], poses: None }
    }

    /// Every graph y = 1000 x on 0..7, so each part reads back its input.
    fn linear() -> crate::hall_of_meat::HomScoring {
        let g = crate::hall_of_meat::Graph { xs: std::array::from_fn(|i| i as f32), ys: std::array::from_fn(|i| i as f32 * 1000.0) };
        crate::hall_of_meat::HomScoring { graphs: [g; 12] }
    }

    #[test]
    fn the_bail_score_is_damage_plus_the_graphed_bonus_parts() {
        let mut b = BailRecord { judo_kicks: 2, max_rotation: 3.0, start_speed: Some(2.0), active_seconds: 4.0,
            push_off_strength: 0.5, airs: vec![(1.0, 2.0)], ..Default::default() };
        b.poses.time[1] = 1.5;
        b.poses.points = 500;
        let s = b.score(None, &linear());
        assert_eq!((s.body, s.rotation, s.start_speed, s.air, s.time, s.push_off), (4000, 3000, 2000, 3000, 4000, 500),
            "body = judo 2000 + swan dive 1500 + 500 pose points");
        assert_eq!(s.total, 16500);
    }

    #[test]
    fn an_air_still_open_at_close_scores_and_each_air_is_graphed_on_its_own() {
        let mut b = BailRecord::default();
        b.tick(&tick(1.0, false, 0.0, true, 2.0), [], []);
        b.tick(&tick(1.0, false, 0.0, true, 1.0), [], []);
        b.tick(&tick(0.5, false, 0.0, false, 0.0), [], []);
        assert_eq!(b.airs, vec![(2.0, 1.0)], "a finished air keeps its own time and fall");
        b.tick(&tick(1.0, false, 0.0, true, 3.0), [], []);
        let s = b.score(None, &linear());
        assert_eq!(s.air, 2000 + 1000 + 1000, "the open air (1 s, no fall yet) counts at close");
        assert_eq!(s.start_speed, 3000, "the speed of the first tick");
    }

    #[test]
    fn a_bail_record_counts_time_push_offs_and_distinct_entities() {
        let mut b = BailRecord::default();
        let dt = 1.0 / 60.0;
        for i in 0..20u64 {
            // Air 4..8 rises to 2 m at tick 5 and falls to 0.5 m; air 19 at 1 m.
            let height = match i { 4 => 1.0, 5 => 2.0, 6 => 1.0, 7 => 0.5, 19 => 1.0, _ => 0.0 };
            let t = tick(dt, i >= 12, if i < 5 || i >= 18 { 0.5 } else { 0.0 }, (4..8).contains(&i) || i >= 19, height);
            b.tick(&t, [i % 3, 100 + i], []);
        }
        b.tick(&tick(dt, true, 0.0, false, 0.0), [], [7, 7]);
        let rows = &b.rows()[7..11];
        assert!((rows[0].1 - 12.0 * dt).abs() < 1e-6);
        assert_eq!(rows[1], (row::PUSH_OFFS, 7.0));
        assert_eq!(rows[2], (row::PEDS_HIT, BailRecord::ENTITY_CAP as f32));
        assert_eq!(rows[3], (row::VEHICLES_HIT, 1.0));
        assert!(b.rows()[..7].iter().all(|r| r.1 == 0.0));
        // Two airs: 4 ticks, then 1 tick closed by the last call.
        assert!((b.rows()[12].1 - 5.0 * dt).abs() < 1e-6);
        assert!((b.rows()[11].1 - 7.0 * dt).abs() < 1e-6);
        assert!((b.rows()[14].1 - 1.5).abs() < 1e-6, "{:?}", b.rows()[14]);
        // Ground ticks: 0..4, 8..19 and the last call = 16 at 3 m/s.
        assert!((b.rows()[13].1 - 16.0 * dt * 3.0).abs() < 1e-5);
    }
}
