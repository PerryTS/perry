//! #11448: embedded CommonJS JSON must survive removal of the build tree.
use std::path::PathBuf;
use std::process::{Command, Output};

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn json_requires_and_cache_reloads_survive_deleted_sources() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path().canonicalize().unwrap();
    std::fs::write(root.join("main.ts"), "import './entry.cjs';\n").unwrap();
    std::fs::write(root.join("data.json"), r#"{"value":42,"nested":["yes"]}"#).unwrap();
    std::fs::write(root.join("null.json"), "null").unwrap();
    std::fs::write(root.join("number.json"), "17").unwrap();
    std::fs::write(
        root.join("entry.cjs"),
        r#"
const first = require('./data.json');
console.log('data', first.value, first.nested[0]);
console.log('same', require('./data.json') === first);
const key = __dirname + '/data.json';
first.value = 99;
delete require.cache[key];
const second = require('./data.json');
console.log('reload', second.value, second === first);
console.log('cache', require.cache[key].exports === second);
const { createRequire } = require('node:module');
const load = createRequire(__filename);
console.log('resolve', load.resolve('./data') === key);
console.log('createRequire', load('./data') === second);
console.log('primitive', require('./null.json'), require('./number.json'));
module.exports = {};
"#,
    )
    .unwrap();
    let node = success(Command::new("node").current_dir(&root).arg("main.ts"));
    assert_eq!(
        String::from_utf8_lossy(&node.stdout),
        "data 42 yes\nsame true\nreload 42 false\ncache true\nresolve true\ncreateRequire true\nprimitive null 17\n"
    );
    let destination = tempfile::tempdir().unwrap();
    let executable = destination
        .path()
        .join(if cfg!(windows) { "app.exe" } else { "app" });
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    success(
        Command::new(compiler)
            .current_dir(&root)
            .env("PERRY_RUNTIME_DIR", runtime)
            .args([
                "compile",
                "main.ts",
                "--no-cache",
                "--no-auto-optimize",
                "-o",
            ])
            .arg(&executable),
    );
    source.close().expect("delete the entire build source tree");
    assert!(!root.exists());
    let native = success(Command::new(executable).current_dir(destination.path()));
    assert_eq!(
        native.stdout, node.stdout,
        "relocated output must match Node"
    );
}
