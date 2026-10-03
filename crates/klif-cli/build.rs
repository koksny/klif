//! klif-cli.exe resources (VERSIONINFO + `klif-cli.exe.manifest`) via winresource on Windows (SPEC sections 8, 10).
//! Only when building FOR Windows on a Windows host (winresource is a cfg(windows) build-dependency); other
//! targets build without resources.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=klif-cli.exe.manifest");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if let Err(e) = windows_resources() {
            panic!("klif-cli resources could not be compiled: {e}");
        }
    }
}

#[cfg(windows)]
fn windows_resources() -> std::io::Result<()> {
    use std::path::PathBuf;

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let numeric: Vec<u32> = version.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    let four = format!("{}.{}.{}.0", numeric.first().unwrap_or(&0), numeric.get(1).unwrap_or(&0), numeric.get(2).unwrap_or(&0));

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let template = std::fs::read_to_string(manifest_dir.join("klif-cli.exe.manifest"))?;
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into()));
    let manifest = out_dir.join("klif-cli.exe.manifest");
    std::fs::write(&manifest, template.replace("@VERSION@", &four))?;

    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "KLIF")
        .set("FileDescription", "KLIF command-line interface")
        .set("CompanyName", "Koksny.com")
        .set("LegalCopyright", "Copyright (c) 2026 Wojciech Górny (Koksny.com)")
        .set("OriginalFilename", "klif-cli.exe")
        .set("InternalName", "klif-cli")
        .set(
            "Comments",
            "Koksny.com LOCAL INFERENCE FORNICATOR. AMD optimized frontend for transformers and DiT. Inspect, tune and calibrate KLIF from a terminal or a coding agent.",
        )
        .set("FileVersion", &version)
        .set("ProductVersion", &version)
        .set_language(0x0409)
        .set_manifest_file(&manifest.to_string_lossy());
    res.compile()
}
