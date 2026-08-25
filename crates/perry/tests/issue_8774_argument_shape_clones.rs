//! Exact-shape ordinary-argument clone-and-route coverage (#8774).
//!
//! The fast fixture pins compiler output and the public checksum. The semantic
//! fixture drives every guard-failure family against Node, both normally and
//! while the copying collector relocates live objects.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_GC_DIAG",
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

fn target_debug_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PERRY_TEST_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let runtime_dir = target_debug_dir();
        let runtime_name = if cfg!(windows) {
            "perry_runtime.lib"
        } else {
            "libperry_runtime.a"
        };
        let stdlib_name = if cfg!(windows) {
            "perry_stdlib.lib"
        } else {
            "libperry_stdlib.a"
        };
        if runtime_dir.join(runtime_name).is_file() && runtime_dir.join(stdlib_name).is_file() {
            return;
        }
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build static runtime archives");
        assert_success("static runtime build", &build);
    });
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn compile(dir: &Path, entry: &str, explain: bool) -> (PathBuf, Output) {
    ensure_runtime_archive();
    let output = dir.join(format!("{entry}.bin"));
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--trace")
        .arg("llvm")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        // The clone contract under test is the portable tagged shadow slot;
        // this also avoids Windows' unsupported RS4GC + funclet-EH pairing in
        // the exception fixture.
        .env("PERRY_RS4GC", "0");
    if explain {
        command.arg("--opt-report=json").arg("--explain-lowering");
    }
    remove_gc_env_overrides(&mut command);
    // Compile-time half of the precise-root moving-loop-poll route. Set after
    // the override scrub so it survives.
    command.env("PERRY_GC_MOVING_LOOP_POLLS", "1");
    let result = command.output().expect("run perry compile");
    assert_success("perry compile", &result);
    (output, result)
}

fn run(binary: &Path, dir: &Path, moving: bool) -> Output {
    let mut command = Command::new(binary);
    command.current_dir(dir);
    remove_gc_env_overrides(&mut command);
    if moving {
        command
            .env("PERRY_GC_SCAVENGE", "1")
            .env("PERRY_GC_SCAVENGE_NURSERY_MB", "1")
            .env("PERRY_GC_MOVING_LOOP_POLLS", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1")
            .env("PERRY_GC_INCREMENTAL", "0")
            .env("PERRY_CONSERVATIVE_STACK_SCAN", "off")
            .env("PERRY_GC_DIAG", "1");
    }
    let output = command.output().expect("run compiled fixture");
    assert_success("compiled fixture", &output);
    output
}

fn run_node(dir: &Path, entry: &str) -> Output {
    let output = Command::new("node")
        .current_dir(dir)
        .arg(entry)
        .output()
        .expect("run Node semantic oracle");
    assert_success("Node semantic oracle", &output);
    output
}

fn copy_minor_relocated_objects(stderr: &str) -> u64 {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("[gc-copy-minor] ran "))
        .map(|fields| {
            let mut in_place = false;
            let mut copied = 0;
            let mut promoted = 0;
            for field in fields.split_whitespace() {
                let Some((key, value)) = field.split_once('=') else {
                    continue;
                };
                match key {
                    "in_place" => in_place = value == "true",
                    "copied_objects" => copied = value.parse::<u64>().unwrap_or(0),
                    "promoted_objects" => promoted = value.parse::<u64>().unwrap_or(0),
                    _ => {}
                }
            }
            if in_place {
                0
            } else {
                copied + promoted
            }
        })
        .sum()
}

fn function_body<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

fn fixture_dir() -> PathBuf {
    workspace_root().join("test-files/fixtures/issue_8774_argument_shapes")
}

fn copy_semantic_fixture(dir: &Path) {
    for file in ["package.json", "foreign.ts", "barrel.ts", "main.ts"] {
        std::fs::copy(fixture_dir().join(file), dir.join(file))
            .unwrap_or_else(|error| panic!("copy {file}: {error}"));
    }
}

fn read_native_records(dir: &Path) -> Vec<serde_json::Value> {
    let lowering = dir.join(".perry-trace/lowering");
    let run_dir = std::fs::read_dir(&lowering)
        .expect("read lowering directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("lowering run directory");
    let mut records = Vec::new();
    for entry in std::fs::read_dir(run_dir).expect("read lowering run") {
        let path = entry.expect("lowering entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("perry_native_reps_") || !name.ends_with(".json") {
            continue;
        }
        let artifact: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read native records"))
                .expect("parse native records");
        records.extend(
            artifact["records"]
                .as_array()
                .expect("native record array")
                .iter()
                .cloned(),
        );
    }
    records
}

#[test]
fn stable_argument_clones_are_direct_reported_and_moving_gc_safe() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::copy(
        workspace_root().join("test-files/test_issue_8774_argument_shape_clones.ts"),
        temp.path().join("valid.ts"),
    )
    .expect("copy valid fixture");
    let (binary, compile_output) = compile(temp.path(), "valid.ts", true);

    assert_eq!(run_node(temp.path(), "valid.ts").stdout, b"20000500000\n");
    assert_eq!(run(&binary, temp.path(), false).stdout, b"20000500000\n");
    let moving = run(&binary, temp.path(), true);
    assert_eq!(moving.stdout, b"20000500000\n");
    let diagnostics = String::from_utf8_lossy(&moving.stderr);
    assert!(
        copy_minor_relocated_objects(&diagnostics) > 0,
        "forced-moving arm relocated no object:\n{diagnostics}"
    );

    let compiler_stdout = String::from_utf8_lossy(&compile_output.stdout);
    assert!(
        !compiler_stdout.contains("passed as a call argument"),
        "valid route retained the retired containment denial:\n{compiler_stdout}"
    );

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/valid_ts.ll"))
        .expect("read valid LLVM IR");
    for method in ["add", "hash", "clear"] {
        let public = format!("perry_method_valid_ts__Registry__{method}");
        let clone = format!("{public}$pshape_args");
        let clone_body = function_body(&ir, &format!("@{clone}("));
        assert!(
            clone_body.contains("getelementptr double") && clone_body.contains("inttoptr i64"),
            "{clone} must use fixed-offset field access:\n{clone_body}"
        );
        assert!(
            !clone_body.contains("shape_descriptor_by_id")
                && !clone_body.contains("js_typed_feedback_class_field_get_guard"),
            "{clone} rebuilt a field IC diamond:\n{clone_body}"
        );
        assert!(ir.contains(&format!("call double @{clone}(")));
        assert!(ir.contains(&format!("call double @{public}(")));
    }
    assert!(ir.contains("pshape_arg.fallback"));

    let records = read_native_records(temp.path());
    for method in ["add", "hash", "clear"] {
        let suffix = format!("Registry__{method}$pshape_args");
        assert!(
            records.iter().any(|record| {
                record["consumer"] == "proven_shape_argument_method_call"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str().is_some_and(|note| {
                                note.starts_with("argument_clone=") && note.ends_with(&suffix)
                            })
                        })
                    })
            }),
            "missing explain-lowering selection for {method}: {records:#?}"
        );
    }
}

#[test]
fn guard_failures_match_node_and_unsafe_parameters_stay_generic() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_semantic_fixture(temp.path());
    let (binary, _) = compile(temp.path(), "main.ts", false);
    let node = run_node(temp.path(), "main.ts");
    let ordinary = run(&binary, temp.path(), false);
    let moving = run(&binary, temp.path(), true);
    assert_eq!(
        ordinary.stdout, node.stdout,
        "ordinary Perry differs from Node"
    );
    assert_eq!(
        moving.stdout, node.stdout,
        "moving-GC Perry differs from Node"
    );

    let ir = std::fs::read_to_string(temp.path().join(".perry-trace/llvm/main_ts.ll"))
        .expect("read semantic LLVM IR");
    for method in ["read", "throws"] {
        assert!(
            ir.contains(&format!("Registry__{method}$pshape_args")),
            "safe declared-field method should get a clone: {method}\n{ir}"
        );
    }
    for method in ["alias", "reassign"] {
        assert!(
            !ir.contains(&format!("Registry__{method}$pshape_args")),
            "aliased/reassigned parameter must stay generic: {method}\n{ir}"
        );
    }
    assert!(
        !ir.contains("ForeignReader__read$pshape_args"),
        "an imported argument class must stay on the generic route:\n{ir}"
    );
    let alias_clone = "perry_method_main_ts__AliasReader__read$pshape_args";
    let _alias_clone_body = function_body(&ir, &format!("@{alias_clone}("));
    assert!(
        !ir.contains(&format!("call double @{alias_clone}(")),
        "a receiver/argument alias must never enter the argument clone:\n{ir}"
    );
    assert!(ir.contains("pshape_arg.fallback"));
}
