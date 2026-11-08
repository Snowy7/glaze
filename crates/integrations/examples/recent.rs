//! Bounded live Shell read. Counts only: personal destination names are not printed or persisted.
fn main() -> anyhow::Result<()> {
    let app = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Microsoft.Windows.Explorer".into());
    for frequent in [false, true] {
        let began = std::time::Instant::now();
        match glaze_integrations::launcher::recent(&app, frequent) {
            Ok(items) => println!(
                "{}",
                serde_json::json!({"kind":if frequent {"Frequent"} else {"Recent"},"count":items.len(),"milliseconds":began.elapsed().as_secs_f64()*1000.0})
            ),
            Err(error) => println!(
                "{}",
                serde_json::json!({"kind":if frequent {"Frequent"} else {"Recent"},"error":error.to_string()})
            ),
        }
    }
    Ok(())
}
