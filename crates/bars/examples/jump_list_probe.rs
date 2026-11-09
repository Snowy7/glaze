//! Read-only smoke check: counts only, never destination paths, task arguments or titles.
fn main() {
    for (name, path) in std::env::args().skip(1).enumerate() {
        let aumid = path.strip_prefix("id:").map(str::to_owned);
        let item = glaze_bars::apps::Item {
            key: String::new(),
            name: String::new(),
            path,
            icon: None,
            pinned: true,
            windows: vec![],
            aumid,
            exe: None,
        };
        let sections = glaze_bars::jump_list::load(&item);
        println!(
            "app {name}: {} sections, {} entries",
            sections.len(),
            sections.iter().map(|s| s.items.len()).sum::<usize>()
        );
        for section in sections {
            let category = match section.name.as_str() {
                "Pinned" | "Tasks" | "Recent" | "Frequent" => section.name.as_str(),
                _ => "Custom",
            };
            println!("  {category}: {}", section.items.len());
        }
    }
}
