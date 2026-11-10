//! The end dialog's results table (a race's RACE_TIMES): rows, the table natives' data block and the race standings.
//! Research: `.local/research/race-results-rows.md` (race_shared build_results / end_heat, natives 8265E950 /
//! 8265DDF8, b925); docs/hails-additions/42-challenges.md "Race results".

use super::dpo::time_text;

/// eResultsLBType.DeathRace (82063B08 = 3.0): the layout a race's results use (rank, name, time).
pub const LAYOUT_DEATH_RACE: u32 = 3;

/// The race's texts and rules (retail values; data, so a mod can change them).
#[derive(Clone, Debug, PartialEq)]
pub struct ResultsRules {
    pub title: String,
    pub layout: u32,
    /// Column titles of the layout (single player).
    pub columns: Vec<String>,
    pub dnf: String,
    pub status_pass: String,
    pub status_won: String,
    pub status_lose: String,
}

impl Default for ResultsRules {
    fn default() -> Self {
        Self {
            title: "ID_CHALLENGE_COMMON_RACE_TIMES".into(),
            layout: LAYOUT_DEATH_RACE,
            columns: vec!["ID_LEADERBOARD_HEADER_RANK".into(), "ID_LEADERBOARD_NAME_HEADER".into(), "ID_LEADERBOARD_HEADER_TIME".into()],
            dnf: "ID_RACE_DNF".into(),
            status_pass: "ID_CHALLENG_RESULT_STATUS_PASS".into(),
            status_won: "ID_CHALLENG_RESULT_STATUS_WON".into(),
            status_lose: "ID_CHALLENG_RESULT_STATUS_LOSE".into(),
        }
    }
}

/// One row (a results record: place, time, status, name; keyed by the racer's stable id).
#[derive(Clone, Debug, PartialEq)]
pub struct ResultRow {
    pub id: u32,
    pub place: u32,
    pub name: String,
    /// Seconds; below 0 = did not finish (the DNF text).
    pub time: f32,
    pub status: String,
    pub local: bool,
}

/// What the end dialog's results table shows.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ResultsView {
    pub title: String,
    pub layout: u32,
    pub columns: Vec<String>,
    pub rows: Vec<ResultRow>,
    pub dnf: String,
}

/// One item of the table's data block.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Number(f32),
    Text(String),
    Bool(bool),
}

impl ResultsView {
    /// The rows in place order (8266BA50 with comparator 8265E930: place ascending).
    pub fn sorted(&self) -> Vec<&ResultRow> {
        let mut rows: Vec<&ResultRow> = self.rows.iter().collect();
        rows.sort_by_key(|r| r.place);
        rows
    }

    /// The local racer's place (Results_GetSelfRank), 0 when none.
    pub fn self_rank(&self) -> u32 {
        self.rows.iter().find(|r| r.local).map_or(0, |r| r.place)
    }

    /// The time cell's text: h:m:s.hh, or the DNF text (8265DDF8 path of 8265E950 item 4).
    pub fn time_text(&self, time: f32) -> String {
        if time >= 0.0 { time_text(time) } else { self.dnf.clone() }
    }

    /// Results_GetLeaderboardDataBlock (8265E950): 8 items per row in place order: place, score text, score value,
    /// status, time text, is local, row index, name. Races write score 0 (not shown by the DeathRace layout).
    pub fn data_block(&self) -> Vec<Cell> {
        self.sorted()
            .into_iter()
            .enumerate()
            .flat_map(|(i, r)| {
                [
                    Cell::Number(r.place as f32),
                    Cell::Text(String::new()),
                    Cell::Number(0.0),
                    Cell::Text(r.status.clone()),
                    Cell::Text(self.time_text(r.time)),
                    Cell::Bool(r.local),
                    Cell::Number(i as f32),
                    Cell::Text(r.name.clone()),
                ]
            })
            .collect()
    }
}

/// A racer at the end of the heat (end_heat's RoundStandings input).
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub id: u32,
    pub name: String,
    pub local: bool,
    /// Its finish time, when it finished.
    pub finish: Option<f32>,
    /// Estimated seconds still to go (Race.GetEstimatedTimeToFinish), for a racer still riding.
    pub eta: f32,
}

/// end_heat + build_results (single player): finishers in finish order, then the racers still riding; an unfinished AI
/// racer gets the race time so far plus its estimated time to finish, the unfinished local racer did not finish (DNF);
/// status PASS while heats remain, else WON within `place_to_win` places, else LOSE.
pub fn race_results(standings: &[Standing], elapsed: f32, more_heats: bool, place_to_win: u32, rules: &ResultsRules) -> ResultsView {
    let time = |s: &Standing| match (s.finish, s.local) {
        (Some(t), _) if t > 0.001 => t,
        (_, true) => -2.0,
        _ => elapsed + s.eta.max(0.0),
    };
    // Finish order first (CurrentRaceScore: earlier finishers score higher), then the estimated times.
    let mut order: Vec<&Standing> = standings.iter().collect();
    order.sort_by(|a, b| {
        let key = |s: &Standing| (s.finish.is_none(), if s.finish.is_some() { s.finish.unwrap_or(0.0) } else { time(s).max(0.0) + if s.local { 1e9 } else { 0.0 } });
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    let rows = order
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let place = i as u32 + 1;
            let status = if more_heats {
                &rules.status_pass
            } else if place <= place_to_win {
                &rules.status_won
            } else {
                &rules.status_lose
            };
            ResultRow { id: s.id, place, name: s.name.clone(), time: time(s), status: status.clone(), local: s.local }
        })
        .collect();
    ResultsView { title: rules.title.clone(), layout: rules.layout, columns: rules.columns.clone(), rows, dnf: rules.dnf.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: u32, local: bool, finish: Option<f32>, eta: f32) -> Standing {
        Standing { id, name: format!("r{id}"), local, finish, eta }
    }

    #[test]
    fn finishers_rank_by_time_then_the_riders_by_estimate_and_an_unfinished_player_is_dnf() {
        let rules = ResultsRules::default();
        let v = race_results(&[s(0, true, Some(90.3), 0.0), s(1, false, Some(71.2), 0.0), s(2, false, None, 12.0), s(3, false, None, 4.0)], 92.0, false, 1, &rules);
        let order: Vec<(u32, u32, f32)> = v.sorted().iter().map(|r| (r.id, r.place, r.time)).collect();
        assert_eq!(order, vec![(1, 1, 71.2), (0, 2, 90.3), (3, 3, 96.0), (2, 4, 104.0)]);
        assert_eq!(v.self_rank(), 2);
        assert_eq!(v.rows.iter().find(|r| r.id == 1).unwrap().status, rules.status_won);
        assert_eq!(v.rows.iter().find(|r| r.id == 0).unwrap().status, rules.status_lose);
        let dnf = race_results(&[s(0, true, None, 30.0), s(1, false, Some(80.0), 0.0)], 150.0, false, 1, &rules);
        assert_eq!(dnf.sorted().last().unwrap().id, 0);
        assert_eq!(dnf.time_text(dnf.sorted()[1].time), "ID_RACE_DNF");
    }

    #[test]
    fn the_data_block_has_eight_items_per_row_in_place_order() {
        let v = race_results(&[s(0, true, Some(70.5), 0.0), s(1, false, Some(75.5), 0.0)], 80.0, false, 1, &ResultsRules::default());
        let b = v.data_block();
        assert_eq!(b.len(), 16);
        assert_eq!(b[0], Cell::Number(1.0));
        assert_eq!(b[4], Cell::Text("1:10.50".into()));
        assert_eq!(b[5], Cell::Bool(true));
        assert_eq!(b[7], Cell::Text("r0".into()));
        assert_eq!(b[14], Cell::Number(1.0));
        assert_eq!(v.layout, LAYOUT_DEATH_RACE);
    }
}
