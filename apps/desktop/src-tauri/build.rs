//! Tauri build script: app manifest listing every command (explicit `allow-<command>` permissions,
//! ARCHITECTURE §11.2). The list lives in `src/commands/names.rs`.

include!("src/commands/names.rs");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attrs)?;
    Ok(())
}
