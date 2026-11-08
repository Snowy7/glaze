fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("glaze-core.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bin=glaze-core=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=glaze-core=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
