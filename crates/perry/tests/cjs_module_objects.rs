//! CommonJS module objects (lane CJM): the program's `require.cache`, the
//! module records and `require.main` are compiler-owned objects with Node's
//! semantics. Every expected line here is what Node prints for the same
//! three files. (`require.cache[b].parent === module` is not pinned: perry
//! evaluates `b.js` eagerly, before the entry's record exists, so it prints
//! `false` where Node prints `true`, on main as well.)

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, files: &[(&str, &str)], entry: &str) -> String {
    for (name, src) in files {
        std::fs::write(dir.join(name), src).expect("write module");
    }
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(dir.join(entry))
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
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

const B: &str = "exports.name = 'b';\n";
const C: &str = "let n = 0;\nexports.count = () => ++n;\n";
const A: &str = r#"
const b = require("./b.js");
const c = require("./c.js");
console.log("cache proto null", Object.getPrototypeOf(require.cache) === null);
console.log("b cached", require.cache[require.resolve("./b.js")].exports === b);
console.log("main", require.main === module);
console.log("in cache", require.resolve("./b.js") in require.cache, "x" in require.cache);
console.log("count", c.count());
delete require.cache[require.resolve("./c.js")];
console.log("deleted", require.resolve("./c.js") in require.cache);
const c2 = require("./c.js");
console.log("reloaded", c2 !== c, c2.count());
// Node's loader reads Module._cache, never the `require.cache` property.
const saved = require.cache;
require.cache = {};
console.log("reassigned keeps instance", require("./b.js") === b);
require.cache = saved;
console.log("builtin", typeof require("os").platform);
"#;

#[test]
fn commonjs_cache_records_and_main_keep_node_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(dir.path(), &[("b.js", B), ("c.js", C), ("a.js", A)], "a.js");
    assert_eq!(
        stdout,
        "cache proto null true\n\
         b cached true\n\
         main true\n\
         in cache true false\n\
         count 1\n\
         deleted false\n\
         reloaded true 1\n\
         reassigned keeps instance true\n\
         builtin function\n"
    );
}
