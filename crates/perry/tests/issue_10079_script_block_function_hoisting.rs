//! Pin package context and native optimization level for #10079. The gap
//! runner's enclosing ESM package otherwise hides the sloppy-mode regression.

use std::process::Command;

const SOURCE: &str = include_str!("../../../test-files/test_gap_10079_block_function_hoisting.ts");

#[test]
fn script_and_esm_block_functions_match_node_at_o0_os_and_oz() {
    let node = Command::new("node")
        .arg("--version")
        .output()
        .expect("Node oracle");
    assert!(node.status.success());
    assert_eq!(
        String::from_utf8_lossy(&node.stdout).trim(),
        format!("v{}", include_str!("../../../.node-version").trim()),
        "use the pinned Node oracle"
    );

    for kind in ["commonjs", "module"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            format!(r#"{{"type":"{kind}"}}"#),
        )
        .unwrap();
        let entry = dir.path().join("fixture.ts");
        std::fs::write(&entry, SOURCE).unwrap();
        let oracle = Command::new("node")
            .arg("--experimental-strip-types")
            .arg(&entry)
            .current_dir(dir.path())
            .output()
            .expect("run Node");
        assert!(
            oracle.status.success(),
            "{kind}: {}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        assert!(String::from_utf8_lossy(&oracle.stdout).contains("var 2,2,2\nlet 10,11,12\n"));

        for level in ["0", "s", "z"] {
            let output = dir.path().join(format!("fixture-{level}"));
            let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
                .args([
                    "compile",
                    "--no-cache",
                    "--no-auto-optimize",
                    "--no-codegen",
                    "--platform",
                    "bun",
                ])
                .arg(&entry)
                .arg("-o")
                .arg(&output)
                .env("PERRY_LL_OPT_LEVEL", level)
                .current_dir(dir.path())
                .output()
                .expect("compile fixture");
            assert!(
                compile.status.success(),
                "{kind}/O{level}: {}",
                String::from_utf8_lossy(&compile.stderr)
            );
            let run = Command::new(&output)
                .current_dir(dir.path())
                .output()
                .expect("run fixture");
            assert!(
                run.status.success(),
                "{kind}/O{level}: stdout={} stderr={}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&oracle.stdout),
                "{kind}/O{level}: native output must match its own package-context oracle"
            );
        }
    }
}
