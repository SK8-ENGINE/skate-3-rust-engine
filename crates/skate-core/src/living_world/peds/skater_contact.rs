//! The skater touching a ped: knock-down or standing stumble, hit direction and reaction (82E38FB8 kind 5).
//! Retail mechanism and data: docs/hails-additions/26g-living-world-ped-interactions.md#skater-touching-a-ped

/// Retail reaction kind (`ped+2496`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReactionKind {
    Knockdown,
    Standing,
}

impl ReactionKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Knockdown => "Knockdown",
            Self::Standing => "Standing",
        }
    }
    /// The inverse of `name` (mod data).
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Knockdown, Self::Standing].into_iter().find(|k| k.name() == name)
    }
}

/// A contact reaction stored for later (retail: the collision component's begin-colliding byte +8 with the reaction
/// `ped+0x9C0` / direction `ped+0x9C4`): a contact during a takedown attempt, or the failed takedown's trip (b326).
/// It plays when the ped's graph next enters `PedestrianColliding`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoredReaction {
    pub kind: ReactionKind,
    pub direction: ReactionDirection,
    /// The contact's closing speed (m/s; 0 for a trip).
    pub closing: f32,
    /// Who touched the ped (`None` for a trip).
    pub toucher: Option<u64>,
}

/// Retail hit direction (`ped+2500`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReactionDirection {
    FromFront,
    FromBack,
    FromLeft,
    FromRight,
}

impl ReactionDirection {
    pub fn name(self) -> &'static str {
        match self {
            Self::FromFront => "FromFront",
            Self::FromBack => "FromBack",
            Self::FromLeft => "FromLeft",
            Self::FromRight => "FromRight",
        }
    }
    /// The inverse of `name` (mod data).
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::FromFront, Self::FromBack, Self::FromLeft, Self::FromRight].into_iter().find(|d| d.name() == name)
    }
}

/// The animation set's collision values [data] (`livingworld_entity_animation`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionRules {
    /// Knock-down above these body speeds (linear, angular), skater contacts (3.0 / 3.0).
    pub knockdown_speed: [f32; 2],
    /// The same for a ped touching the ped (`081F978F8706D77B` / `5E566360FDC5FDD1`, 6.0 / 6.0).
    pub ped_knockdown_speed: [f32; 2],
    /// Byte 17: the set allows knock-downs (false for the marquee sets).
    pub can_knock_down: bool,
    /// Seconds on the ground before getting up, counted from the hit (1.5; security 0.5).
    pub ground_seconds: f32,
    /// Get-up clip times (lying down until, crouched until) that pick a re-hit's posture, by the stored
    /// knock-down direction (`sub_8269E460`, b372): FromFront `recovery_intervals_b`, FromBack
    /// `recovery_intervals`, the sides `recovery_intervals_left` (2.0 / 3.167, 2.0 / 3.167, 1.667 / 2.334).
    pub recovery_front: [f32; 2],
    pub recovery_back: [f32; 2],
    pub recovery_side: [f32; 2],
    /// Where a lying-down re-hit restarts the fall clip (`Wipeout{Back,Fwd,Left}GroundRestartTime`, 0.667 / 0.833 /
    /// 0.833 s, `template/knockdown.xml`).
    pub ground_restart: [f32; 3],
    /// Contacts on a downed ped are dropped once its ground timer has run out more than this many times
    /// (`sub_82E38FB8`: `c+4 > 5`; a code constant in retail, data here).
    pub block_after: u8,
}

impl Default for CollisionRules {
    /// The stock `default` set's values (used only when the export has no set).
    fn default() -> Self {
        Self {
            knockdown_speed: [3.0, 3.0],
            ped_knockdown_speed: [6.0, 6.0],
            can_knock_down: true,
            ground_seconds: 1.5,
            recovery_front: [2.0, 3.167],
            recovery_back: [2.0, 3.167],
            recovery_side: [1.667, 2.334],
            ground_restart: [0.667, 0.833, 0.833],
            block_after: 5,
        }
    }
}

/// The posture a re-hit while getting up starts from ([code] `sub_8269E460`, b372).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posture {
    LyingDown,
    Crouched,
    Standing,
}

impl Posture {
    pub fn name(self) -> &'static str {
        match self {
            Self::LyingDown => "LyingDown",
            Self::Crouched => "Crouched",
            Self::Standing => "Standing",
        }
    }
}

impl CollisionRules {
    /// [code] `sub_8269E460`: the get-up clip's time against the stored knock-down direction's interval pair.
    pub fn posture(&self, getup_time: f32, stored: ReactionDirection) -> Posture {
        let [lying, crouched] = match stored {
            ReactionDirection::FromFront => self.recovery_front,
            ReactionDirection::FromBack => self.recovery_back,
            _ => self.recovery_side,
        };
        if getup_time < lying {
            Posture::LyingDown
        } else if getup_time < crouched {
            Posture::Crouched
        } else {
            Posture::Standing
        }
    }
}

/// Re-hit of a ped getting up (`knockdown.xml` RecoveryCollision, b372): docs/hails-additions/26g-living-world-ped-interactions.md#ped-re-hit
pub fn rehit_steps(posture: Posture, direction: ReactionDirection, rules: &CollisionRules) -> (Vec<ReactionStep>, f32) {
    let mut steps = reaction_steps(ReactionKind::Knockdown, direction);
    let side = match direction {
        ReactionDirection::FromFront => 0,
        ReactionDirection::FromBack => 1,
        _ => 2,
    };
    let start = match posture {
        Posture::LyingDown => rules.ground_restart[side],
        Posture::Crouched => {
            steps[0].anim = ["WipeoutBackCrouch", "WipeoutFwdCrouch", "WipeoutLeftCrouch"][side];
            0.0
        }
        Posture::Standing => 0.0,
    };
    (steps, start)
}

/// [code] `sub_82E38FB8` direction buckets: `normal` is the contact normal and `forward` the ped's
/// forward, both in the ground plane (x, z). The normal is taken as the push on the ped (from the
/// skater into the ped) [inferred: Havok's normal convention for the ped's callback is not read].
pub fn reaction_direction(normal: [f32; 2], forward: [f32; 2]) -> ReactionDirection {
    let len = |v: [f32; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let (ln, lf) = (len(normal), len(forward));
    if ln <= 0.0 || lf <= 0.0 {
        return ReactionDirection::FromFront;
    }
    let (n, f) = ([normal[0] / ln, normal[1] / ln], [forward[0] / lf, forward[1] / lf]);
    let mut a = crate::trigonometry::acos((n[0] * f[0] + n[1] * f[1]).clamp(-1.0, 1.0)).to_degrees();
    // [code] the vpermwi cross product's y lane: cross(forward, normal).y = f.z * n.x - f.x * n.z.
    if f[1] * n[0] - f[0] * n[1] > 0.0 {
        a = -a;
    }
    if a > -35.0 && a < 35.0 {
        ReactionDirection::FromBack
    } else if (-145.0..=-35.0).contains(&a) {
        ReactionDirection::FromRight
    } else if (35.0..=145.0).contains(&a) {
        ReactionDirection::FromLeft
    } else {
        ReactionDirection::FromFront
    }
}

/// [code] `sub_82E38FB8` kind 5, the skater as toucher: `body_speed` = (linear, angular) speed of the
/// ped body after the contact; `gate` = the entity's thresholds (0, 0 stock); `immune` = the brain's
/// `+3277` bit 0x10. `None` = no reaction.
pub fn skater_reaction(body_speed: [f32; 2], gate: [f32; 2], rules: &CollisionRules, immune: bool) -> Option<ReactionKind> {
    if !(body_speed[0] > gate[0] || body_speed[1] > gate[1]) {
        return None;
    }
    if rules.can_knock_down && (body_speed[0] > rules.knockdown_speed[0] || body_speed[1] > rules.knockdown_speed[1]) {
        return Some(ReactionKind::Knockdown);
    }
    (!immune).then_some(ReactionKind::Standing)
}

/// 82E38FB8 kind 5 with a ped toucher: ped thresholds, zombie cheat, forced knock-down (NOT RETAIL YET source): docs/hails-additions/26g-living-world-ped-interactions.md#ped-touching-ped
pub fn ped_reaction(body_speed: [f32; 2], gate: [f32; 2], rules: &CollisionRules, immune: bool, zombie: bool, forced: bool) -> Option<ReactionKind> {
    if !(body_speed[0] > gate[0] || body_speed[1] > gate[1]) {
        return None;
    }
    let fast = body_speed[0] > rules.ped_knockdown_speed[0] || body_speed[1] > rules.ped_knockdown_speed[1];
    if rules.can_knock_down && (forced || (fast && !zombie)) {
        return Some(ReactionKind::Knockdown);
    }
    (!immune && !zombie).then_some(ReactionKind::Standing)
}

/// One animation step of a reaction: the logical (remapped) animation, mirrored or not, the blend in,
/// and whether it cycles until the ped recovers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReactionStep {
    pub anim: &'static str,
    pub mirror: bool,
    pub blend: f32,
    pub cycle: bool,
}

/// The motion graph's reaction [data]: a standing stumble is one clip (0.3 s blend); a knock-down is
/// the fall (0.4 s, `FromStand`), the ground cycle (0.1 s, until `Recover`) and the get-up (0.1 s).
pub fn reaction_steps(kind: ReactionKind, direction: ReactionDirection) -> Vec<ReactionStep> {
    use ReactionDirection::*;
    let step = |anim, mirror, blend, cycle| ReactionStep { anim, mirror, blend, cycle };
    match kind {
        ReactionKind::Standing => {
            let (anim, mirror) = match direction {
                FromFront => ("CollisionBackStanding", false),
                FromBack => ("CollisionFwdStanding", false),
                FromLeft => ("CollisionLeftStanding", true),
                FromRight => ("CollisionLeftStanding", false),
            };
            vec![step(anim, mirror, 0.3, false)]
        }
        ReactionKind::Knockdown => {
            let (side, mirror) = match direction {
                FromFront => ("Back", false),
                FromBack => ("Fwd", false),
                FromLeft => ("Left", true),
                FromRight => ("Left", false),
            };
            let name = |part: &str| -> &'static str {
                match (side, part) {
                    ("Back", "Fall") => "WipeoutBackFall",
                    ("Back", "Ground") => "WipeoutBackGroundCyc",
                    ("Back", _) => "WipeoutBackGetUp",
                    ("Fwd", "Fall") => "WipeoutFwdFall",
                    ("Fwd", "Ground") => "WipeoutFwdGroundCyc",
                    ("Fwd", _) => "WipeoutFwdGetUp",
                    (_, "Fall") => "WipeoutLeftFall",
                    (_, "Ground") => "WipeoutLeftGroundCyc",
                    _ => "WipeoutLeftGetUp",
                }
            };
            vec![step(name("Fall"), mirror, 0.4, false), step(name("Ground"), mirror, 0.1, true), step(name("GetUp"), mirror, 0.1, false)]
        }
    }
}

/// Every logical animation the reactions name (the ped animation loader resolves these).
pub const REACTION_ANIMS: &[&str] = &[
    "CollisionBackStanding", "CollisionFwdStanding", "CollisionLeftStanding",
    "WipeoutBackFall", "WipeoutBackGroundCyc", "WipeoutBackGetUp",
    "WipeoutFwdFall", "WipeoutFwdGroundCyc", "WipeoutFwdGetUp",
    "WipeoutLeftFall", "WipeoutLeftGroundCyc", "WipeoutLeftGetUp",
    "WipeoutBackCrouch", "WipeoutFwdCrouch", "WipeoutLeftCrouch",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_direction_buckets() {
        let f = [0.0, 1.0];
        assert_eq!(reaction_direction([0.0, 1.0], f), ReactionDirection::FromBack, "pushed along its forward");
        assert_eq!(reaction_direction([0.0, -1.0], f), ReactionDirection::FromFront);
        let at = |deg: f32| [deg.to_radians().sin(), deg.to_radians().cos()];
        let (l, r) = (reaction_direction(at(90.0), f), reaction_direction(at(-90.0), f));
        assert!(matches!((l, r), (ReactionDirection::FromLeft, ReactionDirection::FromRight) | (ReactionDirection::FromRight, ReactionDirection::FromLeft)) && l != r);
        assert_eq!(reaction_direction(at(30.0), f), ReactionDirection::FromBack);
        assert_eq!(reaction_direction(at(150.0), f), ReactionDirection::FromFront);
        assert_ne!(reaction_direction(at(40.0), f), ReactionDirection::FromBack);
    }

    #[test]
    fn knock_down_above_three_stumble_below_none_when_still() {
        let r = CollisionRules::default();
        assert_eq!(skater_reaction([3.5, 0.0], [0.0, 0.0], &r, false), Some(ReactionKind::Knockdown));
        assert_eq!(skater_reaction([0.0, 3.1], [0.0, 0.0], &r, false), Some(ReactionKind::Knockdown));
        assert_eq!(skater_reaction([3.0, 0.0], [0.0, 0.0], &r, false), Some(ReactionKind::Standing), "not above 3.0");
        assert_eq!(skater_reaction([1.0, 0.0], [0.0, 0.0], &r, true), None, "immune");
        assert_eq!(skater_reaction([0.0, 0.0], [0.0, 0.0], &r, false), None, "the contact did not move it");
        let marquee = CollisionRules { can_knock_down: false, ..r };
        assert_eq!(skater_reaction([9.0, 0.0], [0.0, 0.0], &marquee, false), Some(ReactionKind::Standing));
    }

    #[test]
    fn reaction_steps_follow_the_motion_graph() {
        let s = reaction_steps(ReactionKind::Standing, ReactionDirection::FromLeft);
        assert_eq!((s.len(), s[0].anim, s[0].mirror, s[0].blend), (1, "CollisionLeftStanding", true, 0.3));
        let k = reaction_steps(ReactionKind::Knockdown, ReactionDirection::FromFront);
        assert_eq!(k.iter().map(|s| s.anim).collect::<Vec<_>>(), ["WipeoutBackFall", "WipeoutBackGroundCyc", "WipeoutBackGetUp"]);
        assert!(k[1].cycle && !k[0].cycle && !k[2].cycle);
        assert!(reaction_steps(ReactionKind::Knockdown, ReactionDirection::FromRight).iter().all(|s| !s.mirror && s.anim.starts_with("WipeoutLeft")));
        for kind in [ReactionKind::Knockdown, ReactionKind::Standing] {
            for d in [ReactionDirection::FromFront, ReactionDirection::FromBack, ReactionDirection::FromLeft, ReactionDirection::FromRight] {
                assert!(reaction_steps(kind, d).iter().all(|s| REACTION_ANIMS.contains(&s.anim)));
            }
        }
    }

    #[test]
    fn a_ped_touching_a_ped_uses_the_ped_thresholds_and_zombie_mode_drops_it() {
        let r = CollisionRules::default();
        let gate = [0.0, 0.0];
        assert_eq!(ped_reaction([4.0, 0.0], gate, &r, false, false, false), Some(ReactionKind::Standing), "below 6.0");
        assert_eq!(skater_reaction([4.0, 0.0], gate, &r, false), Some(ReactionKind::Knockdown), "a skater at 4 m/s knocks down");
        assert_eq!(ped_reaction([6.5, 0.0], gate, &r, false, false, false), Some(ReactionKind::Knockdown));
        assert_eq!(ped_reaction([6.5, 0.0], gate, &r, false, true, false), None, "zombie mode");
        assert_eq!(ped_reaction([1.0, 0.0], gate, &r, false, true, true), Some(ReactionKind::Knockdown), "a forcing toucher");
        assert_eq!(ped_reaction([1.0, 0.0], gate, &r, true, false, false), None, "IgnoreStandingCollisions");
        let marquee = CollisionRules { can_knock_down: false, ..r };
        assert_eq!(ped_reaction([6.5, 0.0], gate, &marquee, false, false, true), Some(ReactionKind::Standing));
    }
}
