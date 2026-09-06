//! Cached physics_footplantmanager layout used by82D6FD60/82D6F5F0/82D70070.
use skate_data::collections::Collections;
pub(super) struct Settings {
    pub deck_bounds: [f32; 4],      //0
    pub max_leg_angle_error: f32,   //280 degrees
    pub leg_length_on_landing: f32, //288
    pub foot_volume_y_offset: f32,  //296
    pub deck_bounds_y_offset: f32,  //300
}
impl Settings {
    pub fn load(data: &Collections) -> Result<Self, String> {
        let float = |name| data.float("physics_footplantmanager", "default", name);
        Ok(Self {
            deck_bounds: data
                .words::<4>("physics_footplantmanager", "default", "DeckBB")?
                .map(f32::from_bits),
            max_leg_angle_error: float("MaxLegAngleError")?,
            leg_length_on_landing: float("LegLengthOnLanding")?,
            foot_volume_y_offset: float("FootVolumeYOffset")?,
            deck_bounds_y_offset: float("DeckBBYOffset")?,
        })
    }
}
