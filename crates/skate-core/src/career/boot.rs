//! The new career's first challenge (research b976, b1000, b1001): while the profile's "new career pending" flag is
//! set, the first world load queues one challenge by the world it loaded (827CF960) and clears the flag.
//! Details: docs/hails-additions/43-career.md#change-the-first-challenge-of-a-new-career-careerbootrs

use std::collections::BTreeMap;

/// The world -> first challenge rule as data; retail: the skate school starts the first tutorial, any other world
/// the university follow the leader. A mod can replace it.
#[derive(Clone, Debug, PartialEq)]
pub struct FirstChallenge {
    pub by_world: BTreeMap<String, String>,
    pub default: String,
}

impl Default for FirstChallenge {
    fn default() -> Self {
        Self { by_world: [("dist_skateschool".to_owned(), "tut_sksc_01".to_owned())].into(), default: "ftl_univ_02".into() }
    }
}

impl FirstChallenge {
    /// The challenge to queue for `world` (a district key such as `dist_skateschool`; None = unknown world).
    pub fn pick(&self, world: Option<&str>) -> &str {
        world.and_then(|w| self.by_world.get(w)).unwrap_or(&self.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skate_school_starts_the_first_tutorial_and_any_other_world_follow_the_leader() {
        let rule = FirstChallenge::default();
        assert_eq!(rule.pick(Some("dist_skateschool")), "tut_sksc_01");
        assert_eq!(rule.pick(Some("dist_university")), "ftl_univ_02");
        assert_eq!(rule.pick(None), "ftl_univ_02");
    }
}
