//! Embed Files' own icon (the Glaze tile with a folder) as resource 101.
fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=glaze-files.rc");
    println!("cargo:rerun-if-changed=../../assets/logo/glaze-files.ico");
    println!("cargo:rerun-if-env-changed=RC");
    let resource = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("files.res");
    let compiler = std::env::var_os("RC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            // Cargo's MSVC linker discovery doesn't put Windows SDK tools on PATH.
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
        .current_dir(root)
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("glaze-files.rc")
        .status()
        .expect("compile Files icon resource");
    assert!(status.success(), "Files resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=glaze-files={}",
        resource.display()
    );
}
