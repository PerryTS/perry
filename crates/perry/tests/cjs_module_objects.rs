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

const ORD: &str = "exports.b = 2;\nexports.a = 1;\nexports.c = 3;\n";
const COMP: &str = "const k = [\"x\", \"y\"][Number(process.argv.length > 99)];\nexports[k] = \"computed\";\nexports.y = \"named\";\nexports.x = exports.x + \"!\";\n";
const ESB: &str = "var __defProp = Object.defineProperty;\nvar __export = (target, all) => { for (var name in all) __defProp(target, name, { get: all[name], enumerable: true }); };\nlet val = 1;\n__export(exports, { val: () => val, bump: () => bump });\nfunction bump() { val++; }\nexports.plain = \"p\";\n";
const REPL: &str = "exports.a = 1;\nmodule.exports = { other: 2 };\n";
const MAIN: &str = r#"
const ord = require("./ord.js"), comp = require("./comp.js"), esb = require("./esb.js"), repl = require("./repl.js");
console.log("proto", Object.getPrototypeOf(ord) === Object.prototype, ord.constructor === Object, typeof ord.hasOwnProperty);
console.log("order", Object.keys(ord).join(","), JSON.stringify(ord));
console.log("comp", JSON.stringify(comp), Object.keys(comp).join(","));
console.log("esb", esb.val, (esb.bump(), esb.val), Object.keys(esb).join(","), typeof Object.getOwnPropertyDescriptor(esb, "val").get);
console.log("repl", JSON.stringify(repl), "a" in repl);
console.log("in", "a" in ord, "zz" in ord, ord.zz);
exports.late = 1;
console.log("main", Object.keys(module.exports).join(","));
"#;

/// Step 2: `exports` is born with the module's export names as declared
/// keys. Node's prototype, key order, computed stores, esbuild accessors and
/// replacement all hold.
#[test]
fn born_exports_objects_keep_node_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        &[("ord.js", ORD), ("comp.js", COMP), ("esb.js", ESB), ("repl.js", REPL), ("main.js", MAIN)],
        "main.js",
    );
    assert_eq!(
        stdout,
        "proto true true function\n\
         order b,a,c {\"b\":2,\"a\":1,\"c\":3}\n\
         comp {\"x\":\"computed!\",\"y\":\"named\"} x,y\n\
         esb 1 2 val,bump,plain function\n\
         repl {\"other\":2} false\n\
         in true false undefined\n\
         main late\n"
    );
}
