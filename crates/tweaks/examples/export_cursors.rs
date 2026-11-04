//! Writes the built-in cursor sets (`mypaper-dark`, `mypaper-light`) to a
//! folder, to look at them or copy them as a starting point for your own:
//!
//! ```text
//! cargo run -p mypaper-tweaks --example export_cursors -- <folder>
//! ```

fn main() -> anyhow::Result<()> {
    let Some(folder) = std::env::args_os().nth(1) else {
        anyhow::bail!("usage: export_cursors <folder>");
    };
    for set in mypaper_tweaks::cursors::export_builtin(folder.as_ref())? {
        println!("{}", set.display());
    }
    Ok(())
}
