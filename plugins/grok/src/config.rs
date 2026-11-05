use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Grok {
    pub routines: Vec<Routine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

impl Grok {
    /// Accepts migrated arrays and the Advanced settings JSON field.
    pub fn parse(settings: &serde_json::Value) -> Result<Self, String> {
        let routines = match &settings["routines"] {
            serde_json::Value::String(s) => {
                serde_json::from_str(s).map_err(|_| "Advanced routines must be a JSON array")?
            }
            serde_json::Value::Null => Vec::new(),
            value => serde_json::from_value(value.clone())
                .map_err(|_| "Invalid routine configuration")?,
        };
        let config = Self { routines };
        if config.routines.len() > 32 {
            return Err("At most 32 Grok routines".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for r in &config.routines {
            if r.id.is_empty()
                || r.id.len() > 64
                || !r
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            {
                return Err("Invalid routine id".into());
            }
            if !ids.insert(&r.id) {
                return Err("Duplicate routine id".into());
            }
            if r.title.is_empty() || r.title.len() > 512 {
                return Err("Use a routine title of 1–512 bytes".into());
            }
            crate::routines::routine_url(&r.url)?;
            if serde_json::to_vec(&r.input)
                .map_err(|e| e.to_string())?
                .len()
                > 64 * 1024
            {
                return Err("Routine input exceeds 64 KiB".into());
            }
        }
        Ok(config)
    }
}
