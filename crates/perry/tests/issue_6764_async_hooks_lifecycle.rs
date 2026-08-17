//! Regression coverage for #6764 hook snapshot rooting and scheduler resources.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("utf8 stdout")
}

fn assert_multiple_hook_callbacks_survive_allocating_siblings() {
    let output = compile_and_run(include_str!(
        "../../../test-parity/node-suite/async_hooks/hooks/multiple-hooks.ts"
    ));
    assert_eq!(
        output,
        concat!(
            "both hooks callback\n",
            "first hook events: init>before>after\n",
            "second hook events: init>before>after\n",
            "second hook callback\n",
            "first hook after disable: init>before>after\n",
            "second hook after first disable: init>before>after>before>after\n",
        )
    );
}

fn assert_scheduler_resources_preserve_identity_execution_and_trigger_chain() {
    let output = compile_and_run(include_str!(
        "../../../test-parity/node-suite/async_hooks/hooks/scheduler-resource-identity.ts"
    ));
    assert_eq!(
        output,
        concat!(
            "scheduler handles match resources: true true\n",
            "scheduler execution resources: true true true true\n",
            "scheduler trigger chain: true true true\n",
        )
    );
}

fn assert_repeating_timer_uses_one_async_resource_until_clear() {
    let output = compile_and_run(include_str!(
        "../../../test-parity/node-suite/async_hooks/hooks/interval-repeat-lifecycle.ts"
    ));
    assert_eq!(
        output,
        concat!(
            "interval resource relationship: true true true\n",
            "interval repeated calls: 2\n",
            "interval repeated lifecycle: init>before>after>before>after>destroy\n",
        )
    );
}

#[test]
fn async_hooks_lifecycle_regressions() {
    // Keep these compiles sequential. The compiler's auto-optimized runtime
    // build is shared, and parallel test functions would launch redundant
    // Cargo builds before the first one populated that cache.
    assert_multiple_hook_callbacks_survive_allocating_siblings();
    assert_scheduler_resources_preserve_identity_execution_and_trigger_chain();
    assert_repeating_timer_uses_one_async_resource_until_clear();
}
