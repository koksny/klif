//! Build script of klif.exe: the Tauri build plus the Windows resources.
//!
//! * VERSIONINFO: tauri-build writes ProductName, FileVersion / ProductVersion (tauri.conf.json `version`),
//!   CompanyName (`bundle.publisher`), LegalCopyright (`bundle.copyright`) and FileDescription (= `productName`);
//!   OriginalFilename, InternalName and Comments come from `[package.metadata.tauri-winres]` in Cargo.toml.
//! * Manifest: `klif.exe.manifest` (asInvoker, Windows 10 / 11, PerMonitorV2, long paths, Common-Controls v6)
//!   replaces tauri-build's default (Common-Controls only). Its assembly version follows Cargo.toml.

fn main() {
    println!("cargo:rerun-if-changed=klif.exe.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    let attributes = tauri_build::Attributes::new().windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest()));
    if let Err(e) = tauri_build::try_build(attributes) {
        println!("{e:#}");
        std::process::exit(1);
    }
}

/// The manifest with its four-part assembly version filled in (0.3.0 -> 0.3.0.0).
fn manifest() -> String {
    let part = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    let version = format!(
        "{}.{}.{}.0",
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH")
    );
    include_str!("klif.exe.manifest").replace("{{VERSION4}}", &version)
}
