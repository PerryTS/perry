use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_json(project: &Path, entry: &Path, output: &Path) -> Value {
    compile_json_with_env(project, entry, output, &[])
}

fn compile_json_with_env(
    project: &Path,
    entry: &Path,
    output: &Path,
    envs: &[(&str, &str)],
) -> Value {
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(project)
        .arg("--format")
        .arg("json")
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(output);
    for (name, value) in envs {
        cmd.env(name, value);
    }
    let out = cmd.output().expect("run perry compile");

    assert!(
        out.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .expect("json line in stdout");
    serde_json::from_str(line).expect("parse compile json")
}

fn run_binary(output: &Path) -> String {
    let out = Command::new(output).output().expect("run compiled binary");
    assert!(
        out.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn assert_linked(result: &Value) {
    assert_eq!(result["link_cache"]["linked"], true, "{result}");
    assert_eq!(result["link_cache"]["skipped"], false, "{result}");
}

fn assert_skipped(result: &Value) {
    assert_eq!(result["link_cache"]["linked"], false, "{result}");
    assert_eq!(result["link_cache"]["skipped"], true, "{result}");
}

fn assert_build_cache_hit(result: &Value) {
    assert_eq!(result["build_cache"]["hit"], true, "{result}");
}

fn assert_build_cache_miss(result: &Value, reason: &str) {
    assert_eq!(result["build_cache"]["hit"], false, "{result}");
    assert_eq!(result["build_cache"]["miss_reason"], reason, "{result}");
}

fn assert_codegen_cache(
    result: &Value,
    hits: u64,
    misses: u64,
    path_reuses: u64,
    cache_paths_stored: u64,
    object_temp_writes: u64,
) {
    let cache = &result["codegen_cache"];
    assert_eq!(cache["hits"], hits, "{result}");
    assert_eq!(cache["misses"], misses, "{result}");
    assert_eq!(cache["path_reuses"], path_reuses, "{result}");
    assert_eq!(cache["object_cache_paths_reused"], path_reuses, "{result}");
    assert_eq!(
        cache["object_cache_paths_stored"], cache_paths_stored,
        "{result}"
    );
    assert_eq!(cache["hit_bytes_materialized"], 0, "{result}");
    assert_eq!(cache["object_temp_writes"], object_temp_writes, "{result}");
}

#[test]
fn native_compile_skips_link_on_identical_second_build() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path();
    let src = project.join("src");
    let dist = project.join("dist");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dist).unwrap();
    fs::write(
        project.join("package.json"),
        "{\"name\":\"link-cache-test\"}\n",
    )
    .unwrap();
    fs::write(
        src.join("util.ts"),
        "export function answer(): number { return 41; }\n",
    )
    .unwrap();
    fs::write(
        src.join("main.ts"),
        "import { answer } from './util';\nconsole.log(answer() + 1);\n",
    )
    .unwrap();

    let entry = src.join("main.ts");
    let output = dist.join("app");

    let first = compile_json(project, &entry, &output);
    assert_linked(&first);
    assert_build_cache_miss(&first, "manifest-missing");
    assert_codegen_cache(&first, 0, 2, 0, 2, 0);
    let first_bytes = fs::read(&output).expect("first output");
    assert_eq!(run_binary(&output).trim(), "42");

    let second = compile_json(project, &entry, &output);
    assert_skipped(&second);
    assert_build_cache_hit(&second);
    assert_eq!(fs::read(&output).expect("second output"), first_bytes);
    assert_eq!(run_binary(&output).trim(), "42");

    fs::write(
        project.join("package.json"),
        "{\"name\":\"link-cache-test\",\"version\":\"1.0.1\"}\n",
    )
    .unwrap();
    let config_changed = compile_json(project, &entry, &output);
    assert_build_cache_miss(&config_changed, "config");
    assert_eq!(run_binary(&output).trim(), "42");

    let env_changed = compile_json_with_env(project, &entry, &output, &[("PERRY_DEBUG_INIT", "1")]);
    assert_linked(&env_changed);
    assert_build_cache_miss(&env_changed, "env");

    let env_restored = compile_json(project, &entry, &output);
    assert_linked(&env_restored);
    assert_build_cache_miss(&env_restored, "env");
    assert_eq!(run_binary(&output).trim(), "42");

    let warm_again = compile_json(project, &entry, &output);
    assert_skipped(&warm_again);
    assert_build_cache_hit(&warm_again);

    fs::write(
        src.join("util.ts"),
        "export function answer(): number { return 40; }\n",
    )
    .unwrap();
    let changed = compile_json(project, &entry, &output);
    assert_linked(&changed);
    assert_build_cache_miss(&changed, "source");
    assert_codegen_cache(&changed, 1, 1, 1, 1, 0);
    assert_eq!(run_binary(&output).trim(), "41");

    fs::remove_file(&output).unwrap();
    let missing_output = compile_json(project, &entry, &output);
    assert_linked(&missing_output);
    assert_build_cache_miss(&missing_output, "output");
    assert_codegen_cache(&missing_output, 2, 0, 2, 0, 0);
    assert!(output.exists());
}
