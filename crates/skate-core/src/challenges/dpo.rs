//! The DPO (score / target box) values: retail's `eDPOFormat` / `eDPOIcon` and the per-format text of the numerator
//! and denominator (formatter `8266AF68`, research b997 section 2). The host localizes the templates.
//! Details: docs/hails-additions/42-challenges.md#the-dpo-killed-it-time-and-position

/// `eDPOFormat` (registered in `8284EE90`): Ints 0, Points 1, Time 2, Distance 3, Players 4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DpoFormat {
    #[default]
    Ints,
    Points,
    Time,
    Distance,
    Players,
}

impl DpoFormat {
    /// The retail value (the Lua `eDPOFormat` number).
    pub fn value(self) -> u8 {
        self as u8
    }

    pub fn from_value(v: u8) -> Option<Self> {
        [Self::Ints, Self::Points, Self::Time, Self::Distance, Self::Players].get(v as usize).copied()
    }
}

/// `eDPOIcon` values (registered in `8284F5F8`); the dpo movie does not draw the icon (b997 section 1).
pub mod icon {
    pub const CHALLENGE: u8 = 0;
    pub const KILLED_IT: u8 = 1;
}

/// The text the movie gets for one half: plain text, or a language template filled with the value.
#[derive(Clone, Debug, PartialEq)]
pub enum DpoText {
    Plain(String),
    Template { id: &'static str, value: f64 },
}

/// The numerator (`+0x2C`): Ints / Points / Players a plain integer, Distance one decimal in ID_COMMON_DISTANCE, Time
/// the time text.
pub fn numerator(format: DpoFormat, value: f32) -> DpoText {
    match format {
        DpoFormat::Ints | DpoFormat::Points | DpoFormat::Players => DpoText::Plain(format!("{}", value as i64)),
        DpoFormat::Distance => DpoText::Template { id: "ID_COMMON_DISTANCE", value: value as f64 },
        DpoFormat::Time => DpoText::Plain(time_text(value)),
    }
}

/// The denominator (`+0xAC`): Ints ID_COMMON_NUMBER, Points ID_DPO_STR_POINTS, Players ID_DPO_STR_FINISHED (each
/// with the integer), Distance ID_COMMON_DISTANCE, Time the time text.
pub fn denominator(format: DpoFormat, value: f32) -> DpoText {
    let int = (value as i64) as f64;
    match format {
        DpoFormat::Ints => DpoText::Template { id: "ID_COMMON_NUMBER", value: int },
        DpoFormat::Points => DpoText::Template { id: "ID_DPO_STR_POINTS", value: int },
        DpoFormat::Players => DpoText::Template { id: "ID_DPO_STR_FINISHED", value: int },
        DpoFormat::Distance => DpoText::Template { id: "ID_COMMON_DISTANCE", value: value as f64 },
        DpoFormat::Time => DpoText::Plain(time_text(value)),
    }
}

/// The race time as `M:SS.hh` (8290EB38 with the ':' ':' '.' separators; hours only from one hour on). The digit
/// counts are filled at runtime and were not read (b997: medium confidence); this is the layout the separators give.
pub fn time_text(seconds: f32) -> String {
    let t = seconds.max(0.0);
    let hundredths = (t * 100.0).floor() as u64;
    let (h, m, s, c) = (hundredths / 360_000, hundredths / 6_000 % 60, hundredths / 100 % 60, hundredths % 100);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{c:02}")
    } else {
        format!("{m}:{s:02}.{c:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_position_dpo_reads_place_over_the_racer_count() {
        assert_eq!(numerator(DpoFormat::Ints, 3.0), DpoText::Plain("3".into()));
        assert_eq!(denominator(DpoFormat::Ints, 5.0), DpoText::Template { id: "ID_COMMON_NUMBER", value: 5.0 });
        assert_eq!(denominator(DpoFormat::Players, 2.0), DpoText::Template { id: "ID_DPO_STR_FINISHED", value: 2.0 });
    }

    #[test]
    fn the_time_text_is_minutes_seconds_hundredths() {
        assert_eq!(time_text(85.0), "1:25.00");
        assert_eq!(time_text(65.07), "1:05.07");
        assert_eq!(time_text(9.5), "0:09.50");
        assert_eq!(time_text(3725.25), "1:02:05.25");
        assert_eq!(numerator(DpoFormat::Time, 70.0), DpoText::Plain("1:10.00".into()));
    }
}
