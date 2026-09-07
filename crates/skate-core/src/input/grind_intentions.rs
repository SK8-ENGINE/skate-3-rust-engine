//! TU3 Listener::Fill825999F0, grind emissions8259ACF0..8259AD94.
//! Publish controller-space values; the stock graphs apply stance/mirror filters.
use super::{controller::DerivedControllerInput, riding_intentions::RidingIntent};

pub fn produce(controller: &DerivedControllerInput) -> Vec<RidingIntent> {
    let words = controller.words();
    let left_x = f32::from_bits(words[7]);
    let right_x = f32::from_bits(words[9]);
    let right_y = f32::from_bits(words[10]);
    [
        ("GrindBalanceX", -left_x),
        ("PhysGrindTranslation", (left_x + right_x).clamp(-1.0, 1.0)),
        ("PhysGrindStabilityNudge", left_x),
        ("PhysGrindUpDown", right_y),
    ]
    .into_iter()
    // Native Fill omits neutral records. Rebuilding the AG map removes the
    // previous tick's input, letting the graph end the corresponding MG intent.
    .filter(|(_, value)| *value != 0.0)
    .map(|(name, value)| RidingIntent { name, value })
    .collect()
}
