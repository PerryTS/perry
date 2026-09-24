//! File identities returned by a wrapped CommonJS require.resolve match Node.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn successful(command: &mut Command, subject: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{subject}: {error}"));
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write(root: &Path, name: &str, source: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

#[test]
fn wrapped_require_resolve_matches_node_file_identities() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (name, source) in [
        ("app/package.json", r#"{"type":"commonjs"}"#),
        ("app/m.js", "module.exports = 42;"),
        (
            "app/only.js",
            "throw new Error('resolution must not load only.js');",
        ),
        (
            "app/literal-only.js",
            "throw new Error('resolution must not load literal-only.js');",
        ),
        ("app/folder/index.js", "module.exports = 1;"),
        ("app/package-main/package.json", r#"{"main":"entry.js"}"#),
        ("app/package-main/entry.js", "module.exports = 2;"),
        ("app/index.js", "module.exports = 3;"),
        ("app/same.js", "module.exports = 'outer';"),
        ("app/nested/same.js", "module.exports = 'inner';"),
        (
            "app/nested/resolver.cjs",
            "module.exports = function () { return require.resolve('./same.js'); };",
        ),
        (
            "app/node_modules/fs/index.js",
            "throw new Error('builtin must beat filesystem package');",
        ),
        ("shared.json", "{}"),
    ] {
        write(&root, name, source);
    }
    let source = String::from(
        r#"
const loaded = require('./m.js');
require('fs');
require('node:fs');
const nested = require('./nested/resolver.cjs');
function report(label, request) {
  try { console.log(label, require.resolve(request)); }
  catch (error) { console.log(label, 'error', error.code); }
}
console.log('value', loaded);
report('required', './m.js');
console.log('cache', require.cache[require.resolve('./m.js')].exports === loaded);
report('resolve-only', './only.js');
console.log('literal-only', require.resolve('./literal-only.js'));
report('extension', './m');
report('directory', './folder');
report('package-main', './package-main');
report('parent', '../shared.json');
report('dot', '.');
report('builtin', 'fs');
report('prefixed', 'node:fs');
console.log('nested', nested());
try { require('./missing.js'); } catch (_) {}
report('missing', './missing.js');
"#,
    );
    #[cfg(unix)]
    let source = {
        std::os::unix::fs::symlink("m.js", root.join("app/alias.js")).unwrap();
        source + "report('symlink', './alias.js');\n"
    };
    write(&root, "app/main.cjs", &source);
    let entry = root.join("app/main.cjs");
    let node = successful(
        Command::new("node").current_dir(&root).arg(&entry),
        "Node oracle",
    );
    let transcript = String::from_utf8_lossy(&node.stdout);
    assert!(
        transcript.contains("cache true\n")
            && transcript.contains("missing error MODULE_NOT_FOUND\n"),
        "oracle must exercise cache identity and failed resolution: {transcript}"
    );

    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let executable = root.join(if cfg!(windows) {
        "app.exe"
    } else {
        "app-native"
    });
    successful(
        Command::new(compiler)
            .current_dir(&root)
            .env("PERRY_RUNTIME_DIR", runtime)
            .env("PERRY_WORKSPACE_ROOT", workspace)
            .args(["compile", "--no-cache", "--no-auto-optimize"])
            .arg(&entry)
            .arg("-o")
            .arg(&executable),
        "Perry compile",
    );
    let native = successful(
        Command::new(executable).current_dir(&root),
        "Perry executable",
    );
    assert_eq!(
        native.stdout, node.stdout,
        "Perry must return Node's canonical module filenames and errors"
    );
}
