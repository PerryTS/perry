//! Compile-budget and moving-root witness for the typed root reload boundary.
#![cfg(all(target_arch = "x86_64", target_os = "linux"))]

use std::path::Path;
use std::process::Command;

#[test]
fn typed_root_reload_fits_the_budget_and_preserves_moving_roots() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (fixture, budget) in [
        ("test_gap_rs4gc_root_launder_budget.ts", Some("4890")),
        ("test_gap_rs4gc_capture_reload_budget.ts", Some("6290")),
        ("test_gap_gc_byte_view_binding_identity.ts", None),
    ] {
        let source = workspace.join("test-files").join(fixture);
        let binary = dir.path().join("normalized");
        let oracle = Command::new("node")
            .args(["--no-warnings", "--experimental-strip-types"])
            .arg(&source)
            .output()
            .unwrap();
        assert!(oracle.status.success(), "{:?}", oracle);
        let mut compile_cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
        if let Some(budget) = budget {
            compile_cmd.env("PERRY_LL_RS4GC_MAX_INSTRS", budget);
        } else {
            compile_cmd.env_remove("PERRY_LL_RS4GC_MAX_INSTRS");
        }
        let compile = compile_cmd
            .arg("compile")
            .arg(&source)
            .args(["--no-cache", "-o"])
            .arg(&binary)
            .env("PERRY_NO_AUTO_OPTIMIZE", "1")
            .env("PERRY_SKIP_BUILD", "1")
            .env("PERRY_WORKSPACE_ROOT", workspace)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "typed reloads must fit an enforced budget:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        for stress in [false, true] {
            let mut run = Command::new(&binary);
            if stress {
                run.env("PERRY_GC_FORCE_EVACUATE", "1")
                    .env("PERRY_GC_VERIFY_EVACUATION", "1")
                    .env("PERRY_GC_PROTECT_FROMSPACE", "1")
                    .env("PERRY_GC_SCHEDULE_SEED", "41")
                    .env("PERRY_GC_SCHEDULE_RATE", "1")
                    .env("PERRY_GC_SCHEDULE_ALLOC_KB", "0")
                    .env("PERRY_GC_DIAG", "1");
            }
            let output = run.output().unwrap();
            assert!(
                output.status.success(),
                "stress={stress}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, oracle.stdout, "stress={stress}");
            if stress {
                let diagnostic = String::from_utf8_lossy(&output.stderr);
                assert!(
                    diagnostic.lines().any(|line| {
                        line.starts_with("[gc-copy-minor] ran ")
                            && !line.contains("in_place=true")
                            && line.split_whitespace().any(|field| {
                                field
                                    .strip_prefix("copied_objects=")
                                    .or_else(|| field.strip_prefix("promoted_objects="))
                                    .and_then(|n| n.parse::<usize>().ok())
                                    .is_some_and(|n| n > 0)
                            })
                    }),
                    "the stress arm must actually copy objects:\n{diagnostic}"
                );
            }
        }
    }
}
