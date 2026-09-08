//! VLT fields identified through Scorable82DA2340 and ScoreHolder82DA5D18.
//! No fallback point values: a missing/invalid authored field is an error.
use crate::collections::Collections;
use skate_core::{
    animation::{output::attributes::AttributeName, skeleton_input::name::encode},
    point_graph::PointGraph,
    scoring::{Scorable, catalog::IDENTIFIERS},
};

const SCORABLE: &str = "Hash_6918469984A8C596";
const TUNING: &str = "Hash_349215E2E817703C";

#[derive(Clone, Debug)]
pub struct Definition {
    pub metadata: Scorable,
    pub identifier: &'static str,
    pub encoded_name: AttributeName,
    pub points: i32,
    pub label: String,
    pub trick_type: u32,
    pub completion_delay: f32,
    pub variant: i32,
    pub flags: u32,
}

#[derive(Clone, Debug)]
pub struct ScoringData {
    pub definitions: Vec<Definition>,
    pub repetition: PointGraph<8>,
    pub announcement: PointGraph<8>,
    pub line_drain: f32,
    pub line_capacity: f32,
    pub combo_drain: f32,
    pub combo_capacity: f32,
    /// Ascending point thresholds and their authored multiplier.
    pub combo_levels: [(f32, f32); 3],
    pub combo_refresh_threshold: f32,
    pub unannounced_factor: f32,
    pub bail_factor: f32,
}

impl ScoringData {
    pub fn session_rules(&self) -> skate_core::scoring::session::Rules {
        skate_core::scoring::session::Rules {
            combo_capacity: self.combo_capacity,
            combo_levels: self.combo_levels,
            combo_refresh_threshold: self.combo_refresh_threshold,
            line_capacity: self.line_capacity,
            bail_factor: self.bail_factor,
        }
    }
    pub fn load(data: &Collections) -> Result<Self, String> {
        let mut definitions = Vec::new();
        for (id, &(identifier, class, score_type)) in IDENTIFIERS.iter().enumerate() {
            // The executable enum includes unused entries without VLT records.
            // Only actual records are eligible for recognition.
            if !data.entries().iter().any(|r| {
                r.class_name == SCORABLE
                    && (r.key == identifier
                        || r.key == crate::attrib_hash::numeric_name(identifier))
            }) {
                continue;
            }
            let words = |name| data.words::<1>(SCORABLE, identifier, name).map(|v| v[0]);
            let actual_id = words("Hash_B2383F16252DFE8E")?;
            if actual_id != id as u32 {
                return Err(format!(
                    "Scorable {identifier}: enum {id} disagrees with VLT {actual_id}"
                ));
            }
            let label = data.field(SCORABLE, identifier, "Hash_843613E915014627")?;
            if label.type_name != "EA::Reflection::Text" {
                return Err(format!("Invalid scorable label {identifier}"));
            }
            definitions.push(Definition {
                metadata: Scorable {
                    id,
                    class,
                    score_type,
                },
                identifier,
                encoded_name: encode(identifier.as_bytes()),
                points: data.integer(SCORABLE, identifier, "Hash_937F62AE5C1ED284")? as i32,
                label: label.data.clone(),
                trick_type: words("TrickType")?,
                completion_delay: data.float(SCORABLE, identifier, "Hash_DC7F402E5680BA02")?,
                variant: words("Hash_2E90BC04042A0B5A")? as i32,
                flags: words("Hash_D34A84B044B60CE3")?,
            });
        }
        if definitions.is_empty() {
            return Err("No native scorable definitions in owned VLT data".into());
        }
        let graph = |field| -> Result<PointGraph<8>, String> {
            let w = data.words::<20>(TUNING, "default", field)?;
            let x = std::array::from_fn(|i| f32::from_bits(w[4 + i]));
            let y = std::array::from_fn(|i| f32::from_bits(w[12 + i]));
            if x.iter().chain(y.iter()).any(|v| !v.is_finite()) || x.windows(2).any(|v| v[0] > v[1])
            {
                return Err(format!("Invalid native scoring curve {field}"));
            }
            Ok(PointGraph { x, y })
        };
        let f = |field| data.float(TUNING, "default", field);
        Ok(Self {
            definitions,
            repetition: graph("Hash_59D91EAABF033A24")?,
            announcement: graph("Hash_263B277F8CA17126")?,
            line_drain: f("Hash_FE02A45231F7B06A")?,
            line_capacity: f("Hash_57478337ACCFFF18")?,
            combo_drain: f("Hash_88407506DC9780ED")?,
            combo_capacity: f("Hash_28767A5F961C7129")?,
            combo_levels: [
                (f("Hash_829887D8C24A5B8A")?, f("Hash_4752056364FF91FE")?),
                (f("Hash_FB8408A15C17A9D4")?, f("Hash_F4993A13ED7B44C1")?),
                (f("Hash_30AAE071A7868771")?, f("Hash_DC0C3853F6C5FC2E")?),
            ],
            combo_refresh_threshold: f("Hash_E36197BFC8EA1CAD")?,
            unannounced_factor: f("Hash_2577DF0FCE5251E4")?,
            bail_factor: f("Hash_B0B56FF046508506")?,
        })
    }
    pub fn by_name(&self, name: AttributeName) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.encoded_name == name)
    }
    pub fn by_id(&self, id: usize) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.metadata.id == id)
    }
}
