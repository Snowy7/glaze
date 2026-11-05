use super::*;
use serde_json::{Map, Value, json};
#[test]
fn faces_match_the_view_golden() {
    let discovery = Discovery {
        installed: true,
        running: true,
        signed_in: true,
    };
    let routines: Vec<_> = ["Morning brief", "Review my calendar", "Research watchlist"]
        .into_iter()
        .enumerate()
        .map(|(i, title)| Routine {
            id: format!("routine-{i}"),
            title: title.into(),
            url: "https://example.test/routine".into(),
            input: json!({}),
        })
        .collect();
    let runs: Vec<_> = routines
        .iter()
        .zip([
            RoutineState::Accepted,
            RoutineState::Sending,
            RoutineState::Failed,
        ])
        .map(|(r, state)| RoutineRun {
            id: r.id.clone(),
            title: r.title.clone(),
            state,
        })
        .collect();
    let mut out = Map::new();
    for id in ["routines", "recent", "prompt"] {
        let sizes: Map<String, Value> = [
            ("small", SizeClass::Small),
            ("medium", SizeClass::Medium),
            ("large", SizeClass::Large),
        ]
        .into_iter()
        .map(|(name, size)| {
            (
                name.into(),
                json!(face(id, size, &discovery, &routines, &runs)),
            )
        })
        .collect();
        out.insert(id.into(), sizes.into());
    }
    for (name, discovery) in [("setup", Discovery::default()), ("signed-in", discovery)] {
        out.insert(
            name.into(),
            json!({"medium":face("routines",SizeClass::Medium,&discovery,&[],&[])}),
        );
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
    let actual = serde_json::to_string_pretty(&out).unwrap() + "\n";
    if std::env::var_os("MYPAPER_BLESS").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        actual
    );
}
