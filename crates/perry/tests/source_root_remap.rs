//! Regression test: `--source-root` / `PERRY_SOURCE_ROOT` bakes module paths
//! relative to a root instead of the build directory.
//!
//! A module under the root sees `<root>/a/b.js` as `<target>/a/b.js` in
//! `__dirname`, `__filename` and `import.meta.{url,dirname,filename}`, in every
//! module (entry and `createRequire`d sibling alike), while compile-time
//! resolution keeps using the real files, so
//! `require('./lib/sibling.cjs')` (with `const require = createRequire(import.meta.url)`)
//! still works.
//! Without the option the real absolute paths are baked, unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const MAIN: &str = r#"import { createRequire } from "node:module";
console.log("dirname=" + __dirname);
console.log("filename=" + __filename);
console.log("meta.url=" + import.meta.url);
console.log("meta.dirname=" + import.meta.dirname);
console.log("meta.filename=" + import.meta.filename);
const require = createRequire(import.meta.url);
const sibling = require("./lib/sibling.cjs");
console.log("sibling.answer=" + sibling.answer);
console.log("sibling.dirname=" + sibling.dirname);
console.log("sibling.filename=" + sibling.filename);
"#;

const SIBLING: &str = r#"module.exports = {
  answer: 42,
  dirname: __dirname,
  filename: __filename,
};
"#;

/// Lay out `<tmp>/project/src/{main.ts,lib/sibling.cjs}` and return the
/// canonical project root plus the entry path.
fn write_project(tmp: &Path) -> (PathBuf, PathBuf) {
    let project = tmp.join("project");
    std::fs::create_dir_all(project.join("src/lib")).expect("create project dirs");
    std::fs::write(project.join("src/main.ts"), MAIN).expect("write main");
    std::fs::write(project.join("src/lib/sibling.cjs"), SIBLING).expect("write sibling");
    let project = project.canonicalize().expect("canonicalize project");
    let entry = project.join("src/main.ts");
    (project, entry)
}

fn compile_and_run(entry: &Path, extra: &[&str], env: &[(&str, &str)]) -> String {
    let output = entry.with_file_name("main_bin");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(entry.parent().unwrap())
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(&output)
        .args(extra);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn expected(base: &str) -> String {
    format!(
        "dirname={base}/src\n\
         filename={base}/src/main.ts\n\
         meta.url=file://{base}/src/main.ts\n\
         meta.dirname={base}/src\n\
         meta.filename={base}/src/main.ts\n\
         sibling.answer=42\n\
         sibling.dirname={base}/src/lib\n\
         sibling.filename={base}/src/lib/sibling.cjs\n"
    )
}

#[test]
fn source_root_rebases_baked_paths_onto_slash() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (project, entry) = write_project(tmp.path());
    let stdout = compile_and_run(&entry, &["--source-root", project.to_str().unwrap()], &[]);
    assert_eq!(stdout, expected(""));
    // The entry path, module registrations and worker/dynamic-require keys
    // are baked into the binary too, not only the values the program prints.
    let binary = std::fs::read(entry.with_file_name("main_bin")).expect("read compiled binary");
    let needle = project.to_str().unwrap().as_bytes();
    assert!(
        !binary.windows(needle.len()).any(|w| w == needle),
        "compiled binary still contains the build directory {}",
        project.display()
    );
}

#[test]
fn source_root_target_and_env_vars() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (project, entry) = write_project(tmp.path());
    let stdout = compile_and_run(
        &entry,
        &[],
        &[
            ("PERRY_SOURCE_ROOT", project.to_str().unwrap()),
            ("PERRY_SOURCE_ROOT_TARGET", "/app"),
        ],
    );
    assert_eq!(stdout, expected("/app"));
}

#[test]
fn without_source_root_real_paths_are_baked() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (project, entry) = write_project(tmp.path());
    let stdout = compile_and_run(&entry, &[], &[]);
    assert_eq!(stdout, expected(project.to_str().unwrap()));
}
