//! Regression for #9526: runtime member writes to a declared class static must
//! update the LLVM global used by direct `C.field` reads.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

const EXPECTED: &str = "5\n6\n7\n8\n9\n10\n11\nab\n";

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
        command.current_dir(workspace_root()).args([
            "build",
            "-p",
            "perry-runtime-static",
            "-p",
            "perry-stdlib-static",
        ]);
        remove_gc_env_overrides(&mut command);
        let build = command.output().expect("build static runtime archives");
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

fn write_fixture(path: &Path, strict: bool) {
    let module_marker = if strict { "export {};" } else { "" };
    std::fs::write(
        path,
        format!(
            r#"
{module_marker}
class K {{
  static n = 1;
  static text = "a";
}}

K.n = 5;
console.log(K.n);
K.n += 1;
console.log(K.n);
K["n"] += 1;
console.log(K.n);
const constantKey = "n";
K[constantKey] += 1;
console.log(K.n);
const runtimeKey: string = JSON.parse("\"n\"");
K[runtimeKey] += 1;
console.log(K.n);
K.n++;
console.log(K.n);
const alias: any = K;
alias[runtimeKey] += 1;
console.log(K.n);

const textKey: string = JSON.parse("\"text\"");
K[textKey] += "b";
const keep: any[] = [];
for (let i = 0; i < 12000; i++) {{
  const value = {{ i, pad: "x" + i }};
  if (i % 997 === 0) keep.push(value);
}}
(globalThis as any).gc?.();
console.log(K.text);
"#
        ),
    )
    .expect("write static compound-assignment fixture");
}

fn compile(dir: &Path, entry: &Path, label: &str) -> PathBuf {
    let output = dir.join(format!("main_{label}"));
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .env("PERRY_RUNTIME_DIR", runtime_dir());
    remove_gc_env_overrides(&mut command);
    let compile = command.output().expect("compile static compound fixture");
    assert!(
        compile.status.success(),
        "{label} compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(binary: &Path, force_evacuation: bool) -> Output {
    let mut command = Command::new(binary);
    remove_gc_env_overrides(&mut command);
    if force_evacuation {
        command
            .env("PERRY_GC_SCAVENGE", "1")
            .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_INCREMENTAL", "0");
    }
    command.output().expect("run static compound fixture")
}

#[test]
fn declared_static_compound_writes_share_the_direct_read_cell_in_both_modes() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (label, filename, strict) in [
        ("strict", "strict.ts", true),
        ("sloppy", "sloppy.cts", false),
    ] {
        let entry = dir.path().join(filename);
        write_fixture(&entry, strict);
        let binary = compile(dir.path(), &entry, label);
        for force_evacuation in [false, true] {
            let result = run(&binary, force_evacuation);
            assert!(
                result.status.success(),
                "{label} run failed (force_evacuation={force_evacuation})\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout),
                EXPECTED,
                "{label} output differed (force_evacuation={force_evacuation})"
            );
        }
    }
}
