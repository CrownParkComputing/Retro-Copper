// The core's version, read where it is actually written.
//
// The About screen names the core and its version, and a number typed into
// a string literal drifts the moment the submodule moves - which is exactly
// what happened here: a commit titled "update to v19" left the pointer on
// 0.17 and nothing said so. This reads copperline's own Cargo.toml at build
// time instead, so the app cannot claim a version it is not running.
use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../copperline/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let version = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.trim_start().starts_with("version"))
                .and_then(|line| line.split('"').nth(1).map(str::to_string))
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=COPPERLINE_VERSION={version}");
}
