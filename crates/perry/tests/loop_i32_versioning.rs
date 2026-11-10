//! Dynamic loop representation must preserve overflow, coercion and observations.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn versioned_counter_preserves_double_semantics() {
    check_counter(false);
    check_counter(true);
}

/// Compile `source` (with canonical i32 locals on or off), run it, and
/// return its stdout and the emitted LLVM IR.
fn compile_and_run(name: &str, source: &str, disabled: bool) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{name}.ts"));
    let binary = dir.path().join(name);
    std::fs::write(&path, source).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--trace", "llvm"])
        .arg(&path)
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
    let trace = dir.path().join(".perry-trace/llvm");
    let ir: String = std::fs::read_dir(trace)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    (String::from_utf8_lossy(&run.stdout).into_owned(), ir)
}

fn check_counter(disabled: bool) {
    let (stdout, ir) = compile_and_run(
        "counter",
        include_str!("../../../tests/parity/language/control-flow/test_gap_loop_i32_versioning.ts"),
        disabled,
    );
    assert_eq!(stdout,
        "15 5\n12 4\n0 0\n0 0\n12 4\n9 3\n1 2147483647\n28 2147483653\n10 5\n6 4\n0 1 4\n0\n1\ncaught 2\n9 6\n15\n12\n0\n12\n1\n28\n");
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

/// The function `name` in `ir` (its body up to the closing brace).
fn function<'a>(ir: &'a str, file: &str, name: &str) -> &'a str {
    let symbol = format!("double @perry_fn_{file}_ts__{name}(");
    let function = ir
        .split("\ndefine ")
        .find(|f| f.starts_with(&symbol))
        .unwrap_or_else(|| panic!("missing function {symbol}"));
    function.split("\n}").next().unwrap()
}

/// Branching, calling and nested bodies get the integer copy too; every way
/// out of it (exit, `break`, labels declared inside, `return`, `throw`)
/// leaves the counter's Number value exact, and a labelled jump past the
/// copy's exit, a capture or a write keeps the generic loop.
#[test]
fn versioned_counter_branchy_bodies_resynchronize() {
    let (stdout, ir) = compile_and_run(
        "branchy",
        include_str!("../../../tests/parity/language/control-flow/test_gap_loop_i32_versioning_branchy.ts"),
        false,
    );
    assert_eq!(
        stdout,
        "find 1 3\nfind 3 3\nfind 3 2.5\nfind 0 2147483650\nfind 2 3\nwalk true\nwalk false\n\
         firstIndex 2 -1\nskipOdd 108 6\ntriangle 10 5 4\ntriangle 56 7 7\ntriangle 10 5 4\n\
         labels 4 1\ncaughtInside a;[x1:1];b; 3\nthrower at 1\ncaptured 3,3 3\nwritten 7.5 4.5\n\
         shrinking 3 2\nnegativeStart <<abc 3\n"
    );
    // The integer copy must be live for the observations above to test it.
    for name in [
        "find",
        "walk",
        "skipOdd",
        "triangle",
        "caughtInside",
        "negativeStart",
    ] {
        let function = function(&ir, "branchy", name);
        assert!(
            function.contains("for.iv.fast.cond") && function.contains("for.iv.slow.cond"),
            "{name} must be versioned"
        );
    }
    for name in ["labels", "captured", "written", "shrinking"] {
        assert!(
            !function(&ir, "branchy", name).contains("for.iv.fast"),
            "must retain generic storage in {name}"
        );
    }
}
