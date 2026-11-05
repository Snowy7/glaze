fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("mypaper-shell.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    // Studio links the shell library; only the shell executable owns this application identity.
    println!("cargo:rustc-link-arg-bin=mypaper-shell=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=mypaper-shell=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
