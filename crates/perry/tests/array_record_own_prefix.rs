//! Native acceptance for the record read's existing dense-own-prefix proof.
use std::process::Command;

const EXPECTED: &str = "dense 1,2,3,\n\
custom-proto 1,31,3,\n\
custom-proto-labeled 1,31,3,\n\
delete 1,undefined,3,\n\
truncate 1,\n\
grow 1,2,3,\n\
accessor 1,21,3,\n\
inherited 1,41,3, 1,2,3, 1\n\
throw-get get 0\n\
throw-body body 1\n\
break-close 2\n\
custom 10,11,\n\
captured 1,2,3,\n\
move 256 32896\n\
unbacked 32\n";

#[test]
fn record_own_prefix_preserves_mutation_protocol_and_moving_roots() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main");
    std::fs::write(
        &entry,
        include_str!("../../../test-files/test_parity_array_record_own_prefix.ts"),
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args(["compile", "--no-cache"])
        // Select the instrumented archive at compile time; release archives
        // intentionally compile the runtime GC stress/diagnostic knobs out.
        .env("PERRY_GC_INSTRUMENTS", "1")
        // Diagnostic hook installation is selected separately from the archive.
        .env("PERRY_GC_DIAG", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    for moving in [false, true] {
        let mut run = Command::new(&binary);
        if moving {
            run.env("PERRY_GC_FORCE_EVACUATE", "1")
                .env("PERRY_GC_VERIFY_EVACUATION", "1")
                .env("PERRY_GC_DIAG", "1");
        }
        let output = run.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "moving={moving}: {stderr}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            EXPECTED,
            "moving={moving}"
        );
        if moving {
            let relocated: u64 = stderr
                .lines()
                .filter_map(|line| line.strip_prefix("[gc-copy-minor] ran "))
                .filter(|line| !line.contains("in_place=true"))
                .flat_map(|line| line.split_whitespace())
                .filter_map(|field| field.split_once('='))
                .filter(|(key, _)| matches!(*key, "copied_objects" | "promoted_objects"))
                .map(|(_, value)| value.parse::<u64>().unwrap())
                .sum();
            assert!(
                relocated > 0,
                "the moving-GC arm must actually relocate objects: {stderr}"
            );
        }
    }
}
