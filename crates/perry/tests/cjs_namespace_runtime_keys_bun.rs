//! `import * as ns` of a CommonJS module under `--platform bun` exposes every
//! own enumerable key of the evaluated `module.exports`, as Bun does. The
//! static export lexer only sees literal shapes (`exports.x =`,
//! `module.exports = { x }`), so a UMD factory (`fuzzysort`:
//! `module.exports = UMD()` behind a `typeof define` guard) or a
//! `module.exports = make()` produced a namespace with only `default` and
//! `module.exports`; `ns.go` read `undefined`. opencode's command palette calls
//! `fuzzysort.go(...)` on every filter keystroke, so typing into it crashed
//! the TUI with `go is not a function`.
//!
//! Without `--platform bun` the namespace keeps Node's semantics: names the
//! lexer cannot see stay absent (`node main.mjs` prints the same line).

use std::path::{Path, PathBuf};
use std::process::Command;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn package(root: &Path, name: &str, index: &str) {
    write(
        root,
        &format!("node_modules/{name}/package.json"),
        &format!(r#"{{"name": "{name}", "version": "1.0.0", "main": "index.js"}}"#),
    );
    write(root, &format!("node_modules/{name}/index.js"), index);
}

fn compile_and_run(root: &Path, platform: Option<&str>) -> String {
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().expect("compiler directory").to_path_buf());
    let tag = platform.unwrap_or("node");
    let output = root.join(format!("main_{tag}"));
    let mut cmd = Command::new(&compiler);
    cmd.current_dir(root)
        .arg("compile")
        .arg(root.join("main.mjs"));
    if let Some(platform) = platform {
        cmd.arg("--platform").arg(platform);
    }
    let compile = cmd
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "{tag} compile failed: {:?}\n{}\n{}",
        compile.status,
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled program");
    assert!(
        run.status.success(),
        "{tag} program failed: {:?}\n{}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 output")
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name": "cjs-ns-keys", "private": true}"#,
    );
    // fuzzysort's UMD wrapper.
    package(
        root,
        "umdpkg",
        ";((root, UMD) => {\n\
           if (typeof define === 'function' && define.amd) define([], UMD)\n\
           else if (typeof module === 'object' && module.exports) module.exports = UMD()\n\
           else root['umdpkg'] = UMD()\n\
         })(this, _ => {\n\
           'use strict'\n\
           var go = (s) => s + '!'\n\
           return { 'single': go, 'go': go }\n\
         })\n",
    );
    package(
        root,
        "madepkg",
        "function make() { return { hello: () => 'hi' } }\nmodule.exports = make()\n",
    );
    package(
        root,
        "litpkg",
        "module.exports = { a: 1 }\nmodule.exports['la' + 'te'] = 2\n",
    );
    write(
        root,
        "main.mjs",
        "import * as umd from \"umdpkg\"\n\
         import * as made from \"madepkg\"\n\
         import * as lit from \"litpkg\"\n\
         const k = \"go\"\n\
         console.log(typeof umd.go, typeof umd[k], umd.go === umd.default.go, Object.keys(umd).includes(\"go\"))\n\
         console.log(typeof made.hello, made.default.hello())\n\
         console.log(lit.late)\n\
         if (typeof umd.go === \"function\") console.log(umd.go(\"x\"), umd.single === umd.go)\n",
    );
    dir
}

#[test]
fn bun_namespace_of_cjs_exposes_run_time_exports() {
    let dir = fixture();
    // Matches `bun main.mjs`.
    assert_eq!(
        compile_and_run(dir.path(), Some("bun")),
        "function function true true\nfunction hi\n2\nx! true\n"
    );
}

#[test]
fn node_namespace_of_cjs_keeps_lexer_names_only() {
    let dir = fixture();
    // Matches `node main.mjs`.
    assert_eq!(
        compile_and_run(dir.path(), None),
        "undefined undefined false false\nundefined hi\nundefined\n"
    );
}
