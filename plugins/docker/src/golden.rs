//! View fixture shared with the native Docker size sheet. The host renders this JSON.
use crate::{
    Availability, Data, Snapshot,
    docker::{Container, Usage},
    faces,
};
use glaze_plugin_sdk::SizeClass;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub fn fixture() -> Snapshot {
    Snapshot {
        availability: Availability::Live,
        data: Some(Data::Docker(crate::docker::Snapshot {
            containers: [
                ("web", "glaze-web", "running", false),
                ("db", "development-postgres", "running", false),
                ("worker", "background-worker", "exited", true),
            ]
            .into_iter()
            .map(|(id, name, state, failed)| Container {
                id: id.into(),
                name: name.into(),
                state: state.into(),
                failed,
            })
            .collect(),
            usage: BTreeMap::from([
                (
                    "web".into(),
                    Usage {
                        cpu_hundredths: Some(1240),
                        memory_bytes: 192 * 1_048_576,
                        memory_limit: 2_147_483_648,
                    },
                ),
                (
                    "db".into(),
                    Usage {
                        cpu_hundredths: Some(640),
                        memory_bytes: 384 * 1_048_576,
                        memory_limit: 2_147_483_648,
                    },
                ),
            ]),
        })),
        action_error: None,
    }
}

pub fn views() -> Value {
    let live = fixture();
    let mut stale = live.clone();
    stale.availability = Availability::Stale("fixture disconnect".into());
    let empty = Snapshot {
        availability: Availability::Live,
        data: Some(Data::Docker(Default::default())),
        action_error: None,
    };
    let mut out = Map::new();
    let unavailable = Snapshot {
        availability: Availability::Unavailable("Docker isn't running".into()),
        ..Default::default()
    };
    for (name, state) in [
        ("containers", live),
        ("setup", unavailable),
        ("disconnected", stale),
        ("empty", empty),
    ] {
        let sizes: Map<String, Value> = [
            ("small", SizeClass::Small),
            ("medium", SizeClass::Medium),
            ("large", SizeClass::Large),
        ]
        .into_iter()
        .map(|(name, size)| (name.into(), json!(faces::face(size, &state))))
        .collect();
        out.insert(name.into(), Value::Object(sizes));
    }
    Value::Object(out)
}

#[test]
fn view_golden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
    let actual = serde_json::to_string_pretty(&views()).unwrap() + "\n";
    if std::env::var_os("GLAZE_BLESS").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n"),
        actual
    );
}
