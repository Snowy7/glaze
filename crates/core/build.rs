fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("mypaper-core.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bin=mypaper-core=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=mypaper-core=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
