//! Embeds Studio's manifest and the same application icon used by the installer.
fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let manifest = root.join("glaze-studio.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    println!("cargo:rerun-if-changed=glaze-studio.rc");
    println!("cargo:rerun-if-changed=../../assets/logo/glaze.ico");
    println!("cargo:rerun-if-env-changed=RC");
    let resource =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("studio.res");
    let compiler = std::env::var_os("RC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            // Cargo's MSVC linker discovery doesn't put the Windows SDK tools on PATH.
            let kits = std::path::PathBuf::from(std::env::var_os("ProgramFiles(x86)").unwrap())
                .join("Windows Kits/10/bin");
            let mut versions: Vec<_> = std::fs::read_dir(kits)
                .expect("Windows SDK bin directory")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect();
            versions.sort();
            versions
                .into_iter()
                .rev()
                .map(|version| version.join("x64/rc.exe"))
                .find(|path| path.is_file())
                .expect("Windows SDK resource compiler (rc.exe)")
        });
    let status = std::process::Command::new(compiler)
        .current_dir(&root)
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("glaze-studio.rc")
        .status()
        .expect("compile Studio icon resource");
    assert!(status.success(), "Studio resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=glaze-studio={}",
        resource.display()
    );
}
