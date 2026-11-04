//! `Appearance` as files and the UI see it: unset fields are absent, never
//! `null`. The theme crate owns the fields and their ranges (docs/widgets.md,
//! "Appearance"); this wrapper only changes how they are written.

use anyhow::Result;
use mypaper_theme::Appearance;
use serde::ser::Error as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct Look(pub Appearance);

impl Look {
    /// Nothing is set: the layer below shows through.
    pub fn is_empty(&self) -> bool {
        self.0 == Appearance::default()
    }

    /// A JSON object holding only the set fields.
    pub fn to_map(&self) -> Result<Map<String, Value>> {
        // Through text: a `Value` built straight from an `f32` would print as 0.550000011920929.
        let Value::Object(map) = serde_json::from_str(&serde_json::to_string(&self.0)?)? else {
            unreachable!("Appearance serializes to an object")
        };
        Ok(map
            .into_iter()
            .filter(|(_, value)| !value.is_null())
            .collect())
    }

    /// Range checks (opacity, blur, radius, scale).
    pub fn validate(&self) -> Result<()> {
        self.0.validate()
    }
}

impl Serialize for Look {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_map()
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_set_fields_are_written_and_floats_stay_short() {
        let look: Look = serde_json::from_str(
            r##"{ "surface": "glass", "opacity": 0.55, "accent": "auto", "tint": "#101010" }"##,
        )
        .unwrap();
        let text = serde_json::to_string(&look).unwrap();
        assert!(
            text.contains("\"opacity\":0.55,") || text.contains("\"opacity\":0.55}"),
            "{text}"
        );
        assert!(!text.contains("null") && !text.contains("blur"), "{text}");
        assert_eq!(serde_json::from_str::<Look>(&text).unwrap(), look);
        assert!(Look::default().is_empty() && !look.is_empty());
        assert_eq!(serde_json::to_string(&Look::default()).unwrap(), "{}");
    }

    #[test]
    fn unknown_fields_and_out_of_range_values_are_refused() {
        assert!(serde_json::from_str::<Look>(r#"{ "glow": 1 }"#).is_err());
        let loud: Look = serde_json::from_str(r#"{ "opacity": 3 }"#).unwrap();
        assert!(loud.validate().is_err());
    }
}
