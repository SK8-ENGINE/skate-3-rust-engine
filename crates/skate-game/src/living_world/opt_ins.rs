//! Living world features still in test, off by default: zombie mode, NPC skater physics, grab visualizer.
//! Switches and mod reach: docs/hails-additions/26a-living-world-overview.md#living-world-opt-ins
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptIn {
    Zombie,
    NpcPhysics,
    /// Grab cues in the world and grab availability log lines (`grab_view`); a viewer aid, nothing changes in play.
    VisualizeGrabs,
    /// Peds flee a car moving along the line to them, either way (retail's `82E2C0C8` test, W8; still in test).
    VehicleFlee,
}

impl OptIn {
    pub(crate) const ALL: [OptIn; 4] = [OptIn::Zombie, OptIn::NpcPhysics, OptIn::VisualizeGrabs, OptIn::VehicleFlee];

    /// The variable that turns it on for a run.
    pub(crate) fn env(self) -> &'static str {
        match self {
            OptIn::Zombie => "SKATE_ZOMBIE",
            OptIn::NpcPhysics => "SKATE_NPC_SIM",
            OptIn::VisualizeGrabs => "SKATE_VISUALIZE_GRABS",
            OptIn::VehicleFlee => "SKATE_PED_VEHICLE_FLEE",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie Mode",
            OptIn::NpcPhysics => "NPC Skater Physics",
            OptIn::VisualizeGrabs => "Visualize Grabs",
            OptIn::VehicleFlee => "Ped Vehicle Flee",
        }
    }
    /// The overlay's status line for the row (what it does and that it is still being tested).
    pub(crate) fn hint(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie mode (the retail cheat, until the cheat screen exists): yellow screen, no traffic, every ped comes after you.",
            OptIn::NpcPhysics => "NPC skaters near you run full skater physics and bail (still being tested).",
            OptIn::VisualizeGrabs => "Shows the props and car spots in grab range (green = grab now) and logs grab availability.",
            OptIn::VehicleFlee => "Peds run from a car within 10 m that drives toward or away from them (still being tested).",
        }
    }
    fn env_on(self) -> bool {
        std::env::var(self.env()).is_ok_and(|v| v == "1")
    }
    /// Session log key.
    fn log_key(self) -> &'static str {
        match self {
            OptIn::Zombie => "zombie",
            OptIn::NpcPhysics => "npc_physics",
            OptIn::VisualizeGrabs => "visualize_grabs",
            OptIn::VehicleFlee => "ped_vehicle_flee",
        }
    }
}

/// The player's choices. NPC Skater Physics is on by default (user 2026-10-25, USER-ASKS 338: "Lets turn on by default
/// with the ability to toggle it off"); the others are off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct OptIns {
    pub zombie: bool,
    pub npc_physics: bool,
    pub visualize_grabs: bool,
    pub vehicle_flee: bool,
}

impl Default for OptIns {
    fn default() -> Self {
        Self { zombie: false, npc_physics: true, visualize_grabs: false, vehicle_flee: false }
    }
}

impl OptIns {
    /// The `SKATE_*=1` variables of this run.
    pub(crate) fn from_env() -> Self {
        let mut s = Self { zombie: false, npc_physics: false, visualize_grabs: false, vehicle_flee: false };
        for o in OptIn::ALL {
            s.set(o, o.env_on());
        }
        s
    }
    pub(crate) fn get(&self, o: OptIn) -> bool {
        match o {
            OptIn::Zombie => self.zombie,
            OptIn::NpcPhysics => self.npc_physics,
            OptIn::VisualizeGrabs => self.visualize_grabs,
            OptIn::VehicleFlee => self.vehicle_flee,
        }
    }
    pub(crate) fn set(&mut self, o: OptIn, on: bool) {
        match o {
            OptIn::Zombie => self.zombie = on,
            OptIn::NpcPhysics => self.npc_physics = on,
            OptIn::VisualizeGrabs => self.visualize_grabs = on,
            OptIn::VehicleFlee => self.vehicle_flee = on,
        }
    }
    /// Either one on (saved choice or this run's variable).
    pub(crate) fn or(self, other: Self) -> Self {
        let mut s = self;
        for o in OptIn::ALL {
            s.set(o, self.get(o) || other.get(o));
        }
        s
    }
}

/// What is on right now (the player's choice, or a mod's value while it runs).
pub(crate) fn in_effect(settings: &super::LivingWorldSettings) -> OptIns {
    OptIns {
        zombie: settings.zombie,
        npc_physics: settings.npc_simulated.enabled,
        // The player's own choice only: a viewer aid, not a mod tuning.
        visualize_grabs: settings.user_opt_ins.visualize_grabs,
        vehicle_flee: settings.ped_brain.vehicle_flee.enabled,
    }
}

/// Session log line for one opt-in switching: `OPT_IN_CHANGED <name> off -> on by player`. `by` is `player` (the
/// overlay, or the player's choice back in effect) or `mod`. Every opt-in uses it, also the overlay's audio and
/// water ones, so one search finds every toggle of a session.
pub(crate) fn opt_in_changed_line(name: &str, old: bool, new: bool, by: &str) -> String {
    let word = |on: bool| if on { "on" } else { "off" };
    format!("OPT_IN_CHANGED {name} {} -> {} by {by}", word(old), word(new))
}
pub(crate) fn log_opt_in_changed(name: &str, old: bool, new: bool, by: &str) {
    if old != new {
        info!("{}", opt_in_changed_line(name, old, new, by));
    }
}

/// The OPT_IN_CHANGED lines from `old` to `now`; a value that differs from the player's choice is a mod's.
fn changes(old: OptIns, now: OptIns, user: OptIns) -> Vec<String> {
    OptIn::ALL.iter().filter(|o| old.get(**o) != now.get(**o))
        .map(|o| opt_in_changed_line(o.log_key(), old.get(*o), now.get(*o), if now.get(*o) == user.get(*o) { "player" } else { "mod" }))
        .collect()
}

/// Session log: an opt-in switching (start, overlay or mod), so a report shows which ones were on and when each one
/// was toggled.
pub(crate) fn log_opt_ins(settings: Res<super::LivingWorldSettings>, mut last: Local<Option<OptIns>>) {
    let now = in_effect(&settings);
    if *last != Some(now) {
        if let Some(old) = *last {
            for line in changes(old, now, settings.user_opt_ins) {
                info!("{line}");
            }
        }
        let on: Vec<&str> = OptIn::ALL.iter().filter(|o| now.get(**o)).map(|o| o.log_key()).collect();
        info!("OPT_IN {}", if on.is_empty() { "none".to_owned() } else { on.join(" ") });
        *last = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_and_default_npc_physics_on() {
        assert_eq!(serde_json::from_str::<OptIns>("{}").unwrap(), OptIns::default());
        assert_eq!(OptIns::default(), OptIns { zombie: false, npc_physics: true, visualize_grabs: false, vehicle_flee: false }, "USER-ASKS 338");
        let mut s = OptIns { npc_physics: false, ..OptIns::default() };
        for o in OptIn::ALL {
            assert!(!s.get(o));
            s.set(o, true);
            assert!(s.get(o));
        }
        let back: OptIns = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(OptIns::default().or(OptIns { zombie: true, ..Default::default() }).zombie, true);
    }

    /// The player's choice is in effect at once, a mod may change it while it runs, and undoing the mod brings the
    /// choice back.
    #[test]
    fn mod_reset_restores_the_players_choice() {
        let mut s = super::super::LivingWorldSettings::default();
        s.set_user_opt_ins(OptIns { zombie: true, npc_physics: true, ..Default::default() });
        assert!(s.zombie && s.npc_simulated.enabled);
        s.zombie = false;
        s.npc_simulated.enabled = false;
        s.reset_mod_overrides();
        assert!(s.zombie && s.npc_simulated.enabled);
        assert_eq!(in_effect(&s).zombie, true);
    }

    /// One OPT_IN_CHANGED line per opt-in that switched, naming who switched it.
    #[test]
    fn changes_name_each_toggle_and_its_source() {
        let off = OptIns { npc_physics: false, ..OptIns::default() };
        let zombie = OptIns { zombie: true, ..off };
        assert_eq!(changes(off, zombie, zombie), vec!["OPT_IN_CHANGED zombie off -> on by player"]);
        assert_eq!(changes(zombie, off, zombie), vec!["OPT_IN_CHANGED zombie on -> off by mod"]);
        assert!(changes(zombie, zombie, off).is_empty());
        let both = OptIns { npc_physics: true, visualize_grabs: true, ..off };
        assert_eq!(changes(off, both, both), vec![
            "OPT_IN_CHANGED npc_physics off -> on by player",
            "OPT_IN_CHANGED visualize_grabs off -> on by player",
        ]);
    }
}
