//! Coherent archives for compiled integration fixtures.
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

pub fn dir() -> PathBuf {
    if let Some(path) =
        std::env::var_os("PERRY_TEST_RUNTIME_DIR").or_else(|| std::env::var_os("PERRY_RUNTIME_DIR"))
    {
        return PathBuf::from(path);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        workspace().join(target)
    };
    match std::env::var_os("CARGO_BUILD_TARGET") {
        Some(triple) => target.join(triple).join("release"),
        None => target.join("release"),
    }
}

pub fn ensure() {
    static PREPARE: Once = Once::new();
    PREPARE.call_once(|| {
        let prepared = std::env::var("PERRY_TEST_RUNTIME_PREBUILT").as_deref() == Ok("1");
        assert!(
            !prepared
                || std::env::var_os("PERRY_TEST_RUNTIME_DIR").is_some()
                || std::env::var_os("PERRY_RUNTIME_DIR").is_some(),
            "prepared runtime needs an explicit archive directory"
        );
        if !prepared {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let build = Command::new(cargo)
                .current_dir(workspace())
                .args(["build-perry", "--locked", "--release", "-j6"])
                .output()
                .expect("build coherent runtime archives");
            assert!(
                build.status.success(),
                "runtime build failed:\n{}\n{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            let effects = Command::new("bash")
                .current_dir(workspace())
                .arg(workspace().join("scripts/gc_call_effects/sidecars.sh"))
                .arg(dir())
                .output()
                .expect("generate runtime sidecars");
            assert!(
                effects.status.success(),
                "runtime sidecars failed:\n{}\n{}",
                String::from_utf8_lossy(&effects.stdout),
                String::from_utf8_lossy(&effects.stderr)
            );
        }
        for name in [
            "runtime",
            "stdlib",
            "wasm_host",
            "ext_http",
            "ext_net",
            "ext_nodemailer",
            "ext_ws",
        ] {
            let file = if cfg!(windows) {
                format!("perry_{name}.lib")
            } else {
                format!("libperry_{name}.a")
            };
            assert!(
                dir().join(&file).is_file(),
                "missing coherent runtime archive: {}",
                dir().join(file).display()
            );
        }
        assert!(
            dir().join("libperry_runtime.a.gc-effects").is_file() || cfg!(windows),
            "missing runtime GC effects sidecar in {}",
            dir().display()
        );
    });
}
