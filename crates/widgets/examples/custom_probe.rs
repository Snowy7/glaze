//! Evaluate the shipped examples in an isolated APPDATA supplied by the caller. No user apps receive input.
use serde_json::json;
fn main() -> anyhow::Result<()> {
    let output = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("expected evidence JSON path; set an isolated APPDATA first")
    })?;
    anyhow::ensure!(
        std::env::var("MYPAPER_CUSTOM_FIXTURE").as_deref() == Ok("1"),
        "Set MYPAPER_CUSTOM_FIXTURE=1 and an isolated APPDATA"
    );
    let folder = mypaper_widgets::custom::install_examples()?;
    let registry = mypaper_widgets::registry::builtins();
    let mut results = Vec::new();
    for (name, _) in mypaper_widgets::custom::EXAMPLES {
        let id = format!("custom-{name}");
        let definition = mypaper_widgets::custom::load(&id)?;
        let manifest = &registry
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("Custom widget is absent from catalog"))?
            .manifest;
        anyhow::ensure!(
            manifest.instances.allows(4, 4),
            "Custom definition must support multiple instances"
        );
        let value = mypaper_widgets::custom::evaluate(&id, &definition);
        results.push(json!({"id":id,"layout":format!("{:?}",definition.layout),"multipleInstances":true,"value":value.as_ref().ok(),"error":value.as_ref().err().map(ToString::to_string)}));
    }
    let report = json!({"folder":folder,"examples":results});
    if let Some(parent) = std::path::Path::new(&output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    anyhow::ensure!(
        results.iter().all(|r| r["error"].is_null()),
        "A custom example failed; see evidence JSON"
    );
    Ok(())
}
