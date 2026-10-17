//! A recompile of unchanged sources reuses every cached module object whatever
//! the job count, output path or PATH wrapper, while another `clang` on PATH
//! still changes the key.
#![cfg(target_os = "linux")]

use serde_json::Value;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(project: &Path, output: &str, jobs: &str, path: &OsString) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(project)
        .args(["--format", "json", "compile", "main.ts", "-o", output])
        .arg("--cache-dir")
        .arg(project.join("cache"))
        .env("PATH", path)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_DISABLE_BUILD_CACHE", "1")
        .env("PERRY_MODULE_JOBS", jobs)
        .env("PERRY_CODEGEN_UNIT_JOBS", jobs)
        .env("PERRY_CODEGEN_CPU_SLOTS", jobs)
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_LLVM_CLANG")
        .env_remove("PERRY_OBJECT_CACHE_BUILD_ID")
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .expect("compile JSON");
    serde_json::from_str(line).expect("parse compile JSON")
}

fn objects(cache: &Path) -> BTreeSet<PathBuf> {
    let mut result = BTreeSet::new();
    for target in fs::read_dir(cache.join("objects")).expect("object cache") {
        for entry in fs::read_dir(target.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "o") {
                result.insert(path);
            }
        }
    }
    result
}

fn prefixed_path(dir: &Path) -> OsString {
    let mut dirs = vec![dir.to_path_buf()];
    dirs.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(dirs).unwrap()
}

fn run(binary: &Path) -> String {
    let out = Command::new(binary).output().expect("run fixture");
    assert!(out.status.success(), "fixture failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn objects_survive_job_count_output_path_and_path_wrappers() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    fs::write(
        project.join("package.json"),
        "{\"name\":\"env-cache-test\"}\n",
    )
    .unwrap();
    fs::write(
        project.join("a.ts"),
        "export function twice(x: number) { return x * 2; }\n",
    )
    .unwrap();
    fs::write(
        project.join("b.ts"),
        "export const names = [\"x\", \"y\"].map((s) => s + \"!\");\n",
    )
    .unwrap();
    fs::write(
        project.join("main.ts"),
        "import { twice } from \"./a\";\nimport { names } from \"./b\";\nconsole.log(twice(21), names.join(\",\"));\n",
    )
    .unwrap();
    let base_path = std::env::var_os("PATH").unwrap_or_default();

    let cold = compile(project, "app1", "1", &base_path);
    let cold_objects = objects(&project.join("cache"));
    assert_eq!(cold["codegen_cache"]["hits"], 0, "{cold}");
    assert_eq!(
        cold["codegen_cache"]["misses"],
        cold_objects.len(),
        "{cold}"
    );
    assert!(cold_objects.len() >= 3, "{cold_objects:?}");
    assert_eq!(run(&project.join("app1")), "42 x!,y!\n");

    // What `perf` does to its child: its own directories first on PATH.
    let wrapper = tempfile::tempdir().unwrap();
    let warm = compile(project, "app2", "4", &prefixed_path(wrapper.path()));
    assert_eq!(warm["codegen_cache"]["misses"], 0, "{warm}");
    assert_eq!(warm["codegen_cache"]["hits"], cold_objects.len(), "{warm}");
    assert_eq!(objects(&project.join("cache")), cold_objects);
    assert_eq!(
        fs::read(project.join("app1")).unwrap(),
        fs::read(project.join("app2")).unwrap(),
        "same objects, same binary"
    );

    // Negative control: a different `clang` first on PATH is a different
    // toolchain, so every module misses.
    let Some(real_clang) = perry_codegen::linker::find_clang() else {
        eprintln!("no clang found; skipping the toolchain negative control");
        return;
    };
    let shim = tempfile::tempdir().unwrap();
    let shim_clang = shim.path().join("clang");
    fs::write(
        &shim_clang,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then \"{0}\" --version; echo object-cache-test-shim; exit 0; fi\nexec \"{0}\" \"$@\"\n",
            real_clang.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shim_clang, fs::Permissions::from_mode(0o755)).unwrap();
    let other = compile(project, "app3", "1", &prefixed_path(shim.path()));
    assert_eq!(other["codegen_cache"]["hits"], 0, "{other}");
    assert_eq!(
        other["codegen_cache"]["misses"],
        cold_objects.len(),
        "{other}"
    );
    assert_eq!(run(&project.join("app3")), "42 x!,y!\n");
}
