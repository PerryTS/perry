//! Runtime regression for direct lowering of static-key method literals.
//!
//! Method-only object literals without a `super` home dependency can use the
//! ordinary final-shape object path instead of a synthetic builder IIFE. The
//! controls below cover the source-ordered forms that must retain the IIFE.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

/// Keep the forced-moving arm independent of ambient developer/CI settings.
/// Some inputs affect code generation, so normalize the runtime build,
/// fixture compile, and child process alike.
const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
];

fn remove_gc_env_overrides(command: &mut Command) {
    for key in GC_ENV_OVERRIDES {
        command.env_remove(key);
    }
}

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn runtime_dir() -> PathBuf {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).arg("build");
        remove_gc_env_overrides(&mut command);
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let build = command
            .args(["-p", "perry-runtime-static"])
            .output()
            .expect("build static runtime archive");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });

    perry_bin()
        .parent()
        .expect("Perry binary directory")
        .to_path_buf()
}

fn run_fixture(binary: &Path, force_evacuation: bool) -> Output {
    let mut command = Command::new(binary);
    remove_gc_env_overrides(&mut command);
    if force_evacuation {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run method-literal fixture")
}

#[test]
fn direct_method_literal_preserves_semantics_and_iife_controls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
let order = "";
function evaluated(label: string, value: number): number {
  order += label;
  const churn: any[] = [];
  for (let i = 0; i < 256; i++) churn.push({ i });
  return value;
}

const outer = { offset: 7 };
const fast: any = {
  first: evaluated("a", 1),
  captured(x: number) { return outer.offset + x; },
  dynamicThis(x: number) { return this.first + x; },
  last: evaluated("b", 3),
};

const base: any = { read() { return 10; } };
const withSuper: any = { read() { return super.read() + 1; } };
Object.setPrototypeOf(withSuper, base);

const computedKey = "computed";
const computed: any = { [computedKey]() { return 9; } };

let getterCalls = 0;
const accessor: any = {
  get value() { getterCalls++; return 11; },
};

const spread: any = { ...{ x: 1 }, method() { return 2; } };

console.log(
  order + ":" + Object.keys(fast).join(",") + ":" + fast.captured(5) + ":" +
  fast.dynamicThis(5) + ":" + fast.captured.name + ":" + fast.dynamicThis.name +
  ":" + withSuper.read() + ":" + computed.computed() + ":" + accessor.value +
  ":" + getterCalls + ":" + (spread.x + spread.method()),
);
"#,
    )
    .expect("write method-literal fixture");

    let mut compile_command = Command::new(perry_bin());
    compile_command
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .env("PERRY_RUNTIME_DIR", runtime_dir());
    remove_gc_env_overrides(&mut compile_command);
    let compile = compile_command
        .output()
        .expect("compile method-literal fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    for force_evacuation in [false, true] {
        let run = run_fixture(&binary, force_evacuation);
        assert!(
            run.status.success(),
            "fixture failed (force_evacuation={force_evacuation})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "ab:first,captured,dynamicThis,last:12:6:captured:dynamicThis:11:9:11:1:3\n"
        );
    }
}
