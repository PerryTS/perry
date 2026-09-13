//! A discarded function can leave a dispatch descriptor whose bytes belong
//! to another codegen unit. Both native skeletons and text units must resolve it.

use std::path::Path;
use std::process::Command;

#[test]
fn orphan_dispatch_compiles_with_bytes_owned_by_another_unit() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-files/test_codegen_unit_orphan_dispatch.ts");
    for backend in ["native", "1"] {
        let dir = tempfile::tempdir().expect("temporary compile directory");
        let ll_dir = dir.path().join("ll");
        std::fs::create_dir(&ll_dir).unwrap();
        let output = dir.path().join("out.o");
        let compiled = Command::new(env!("CARGO_BIN_EXE_perry"))
            .arg("compile")
            .arg(&fixture)
            .args(["--no-link", "--no-auto-optimize", "--output"])
            .arg(&output)
            .arg("--cache-dir")
            .arg(dir.path().join("cache"))
            .env("PERRY_CODEGEN_UNITS", "2")
            .env("PERRY_LLVM_INPROCESS", backend)
            .env("PERRY_SAVE_LL", &ll_dir)
            .env("PERRY_NO_CACHE", "1")
            .env("PERRY_DISABLE_WELL_KNOWN", "1")
            .output()
            .expect("run perry compile");
        assert!(
            compiled.status.success(),
            "{backend} compile failed:\n{}\n{}",
            String::from_utf8_lossy(&compiled.stdout),
            String::from_utf8_lossy(&compiled.stderr)
        );
        assert!(std::fs::metadata(output).unwrap().len() > 0);

        let suffix = if backend == "native" {
            "native.ll"
        } else {
            "ll"
        };
        let prefix = "test_codegen_unit_orphan_dispatch_ts";
        let units: Vec<String> = (0..2)
            .map(|i| {
                std::fs::read_to_string(ll_dir.join(format!("{prefix}.unit{i}.{suffix}")))
                    .expect("forced split must emit both units")
            })
            .collect();
        let bytes_line = units[1]
            .lines()
            .find(|line| line.contains("c\"segment\\00\""))
            .expect("unit 1 must define the segment bytes");
        let bytes = bytes_line.split_once(" = ").unwrap().0;
        let dispatch = bytes.replace(".bytes", ".dispatch");
        assert!(units[0].contains(&format!("{dispatch} =")));
        assert!(!units[1].contains(&format!("{dispatch} =")));
        assert!(units[0].contains(&format!("ptr {bytes}")));
        // The descriptor is an owner-only fallback, not a live function use.
        assert!(units.iter().flat_map(|unit| unit.lines()).all(|line| {
            !line.contains(&dispatch) || line.starts_with(&format!("{dispatch} ="))
        }));
        if cfg!(target_os = "macos") {
            assert!(units[0].contains(&format!("{bytes} = linkonce_odr")));
            assert!(bytes_line.contains("linkonce_odr"));
        } else {
            assert!(units[0].contains(&format!("{bytes} = external constant [8 x i8]")));
            assert!(!bytes_line.contains("external constant"));
        }
    }
}
