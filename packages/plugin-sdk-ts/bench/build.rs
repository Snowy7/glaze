use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let input = root.join(".source/runtime.rs");
    println!("cargo:rerun-if-changed={}", input.display());
    println!("cargo:rerun-if-env-changed=MYPAPER_PROBE_MEMORY_MB");
    let cap: usize = env::var("MYPAPER_PROBE_MEMORY_MB").unwrap_or("32".into()).parse().unwrap();
    let wit = root.join(".source/plugin-api/wit").to_string_lossy().replace('\\', "/");
    let mut source = fs::read_to_string(input).unwrap().replace("//!", "//")
        .replace("path: \"../plugin-api/wit\"", &format!("path: \"{wit}\""));
    // Only a separately compiled diagnostic may relax these limits. The production host stays
    // untouched. Keep the memory cap at 32 by default to expose the next production failure.
    source = source.replace("MEMORY_CAP: usize = 32 << 20", &format!("MEMORY_CAP: usize = {cap} << 20"))
        .replace("max_core_instance_size(128 << 10)", "max_core_instance_size(512 << 10)")
        .replace("max_component_instance_size(128 << 10)", "max_component_instance_size(512 << 10)");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("runtime.rs"), source).unwrap();
}
