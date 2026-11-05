fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("mypaper-shell.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Widget GPU sampling and HTTP fetchers start after the first bar. Keep their DLL
    // initialization behind those first API calls too, instead of paying it in the PE loader.
    println!("cargo:rustc-link-arg=/DEFAULTLIB:delayimp.lib");
    for library in ["pdh.dll", "winhttp.dll"] {
        println!("cargo:rustc-link-arg=/DELAYLOAD:{library}");
    }
}
