//! Dynamic loop representation must preserve overflow, coercion and observations.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn versioned_counter_preserves_double_semantics() {
    check_counter(false);
    check_counter(true);
}

fn check_counter(disabled: bool) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("counter.ts");
    let binary = dir.path().join("counter");
    std::fs::write(
        &source,
        include_str!("../../../test-files/test_gap_loop_i32_versioning.ts"),
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--trace", "llvm"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env(
            "PERRY_RUNTIME_DIR",
            std::path::Path::new(env!("CARGO_BIN_EXE_perry"))
                .parent()
                .unwrap(),
        )
        .env(
            "PERRY_CANONICAL_I32_LOCALS",
            if disabled { "0" } else { "1" },
        )
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(&binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "counter failed to terminate: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let run = child.wait_with_output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout),
        "15 5\n12 4\n0 0\n0 0\n12 4\n9 3\n1 2147483647\n28 2147483653\n10 5\n6 4\n0 1 4\n0\n1\ncaught 2\n9 6\n15\n12\n0\n12\n1\n28\n");
    let trace = dir.path().join(".perry-trace/llvm");
    let ir: String = std::fs::read_dir(trace)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    if disabled {
        assert!(
            !ir.contains("for.iv.fast"),
            "canonical-i32 kill switch must disable versioning"
        );
        return;
    }
    assert!(
        ir.contains("for.iv.fast.cond"),
        "integer loop must actually be emitted"
    );
    assert!(
        ir.contains("for.iv.slow.cond"),
        "out-of-range bounds need double semantics"
    );
    for name in ["captured", "caught", "modified"] {
        let symbol = format!("double @perry_fn_counter_ts__{name}(");
        let function = ir
            .split("\ndefine ")
            .find(|f| f.starts_with(&symbol))
            .unwrap_or_else(|| panic!("missing negative-control function {symbol}"));
        let function = function.split("\n}").next().unwrap();
        assert!(
            !function.contains("for.iv.fast"),
            "must retain generic storage in {name}"
        );
    }
    let mut updates = 0;
    for block in ir.split("\n\n") {
        if block.contains("for.iv.fast.update")
            && block
                .lines()
                .any(|line| line.starts_with("for.iv.fast.update"))
        {
            updates += 1;
            assert!(
                !block.contains("fadd double"),
                "integer update retains a double shadow: {block}"
            );
        }
    }
    assert!(updates > 0, "the integer update check must have a subject");
}
