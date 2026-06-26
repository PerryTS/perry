//! Stress regression for the #5437 `node:vm` string-evaluator (Next.js RSC
//! client-reference manifest path). The committed
//! `vm_run_in_context_global_alias` test covers the *shapes* (`||`, object
//! literal RHS, computed-member target); this test covers the *scale* the real
//! `__RSC_MANIFEST` reaches: a wide (thousands-of-keys) and moderately nested
//! object literal assigned through a computed-member target, plus the exact
//! `globalThis.__RSC_MANIFEST = ... || {}` two-statement preamble. The
//! evaluator must build these without crashing, looping, or silently dropping
//! the write (which 500'd `/posts/[id]` + `/fetcher`).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .env("PERRY_ALLOW_PERRY_FEATURES", "1")
        .env("PERRY_ALLOW_EVAL", "1")
        .env("PERRY_ALLOW_UNIMPLEMENTED", "1")
        .arg("compile")
        .arg(&entry)
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn run_in_new_context_builds_large_nested_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const vm: any = require("vm");

// --- A nested manifest literal assigned through globalThis, read back deep. ---
const ctx: any = { process: { env: {} } };
vm.runInNewContext(
  'globalThis.__RSC_MANIFEST = {"a":{"b":[1,2,{"c":"x"}]},"d":"e"}; globalThis.X = globalThis.__RSC_MANIFEST.d',
  ctx,
);
if (!ctx.__RSC_MANIFEST) throw new Error("nested: manifest missing");
if (ctx.__RSC_MANIFEST.a.b[2].c !== "x")
  throw new Error("nested: deep c=" + JSON.stringify(ctx.__RSC_MANIFEST));
if (ctx.X !== "e") throw new Error("nested: X=" + ctx.X);
console.log("nested-ok");

// --- A LARGE (thousands of keys) manifest assigned through a computed-member
//     target, exactly like Next's evalManifest content. ---
const parts: string[] = [];
for (let i = 0; i < 4000; i++) {
  parts.push(
    '"/app/route-' + i + '/page":{"moduleLoading":{"prefix":"","crossOrigin":null},' +
      '"clientModules":{"id' + i + '":{"chunks":["chunk-' + i + '.js"],"name":"*","async":false}}}'
  );
}
const bigJson = "{" + parts.join(",") + "}";
const ctx2: any = {};
vm.runInNewContext(
  'globalThis.__RSC_MANIFEST = globalThis.__RSC_MANIFEST || {};' +
    'globalThis.__RSC_MANIFEST["/posts/[id]/page"] = ' + bigJson + ";",
  ctx2,
);
const entry = ctx2.__RSC_MANIFEST && ctx2.__RSC_MANIFEST["/posts/[id]/page"];
if (!entry) throw new Error("large: manifest entry missing");
if (Object.keys(entry).length !== 4000)
  throw new Error("large: keys=" + Object.keys(entry).length);
if (entry["/app/route-3999/page"].clientModules["id3999"].chunks[0] !== "chunk-3999.js")
  throw new Error("large: nested entry shape");
console.log("large-ok");

console.log("ok");
"#,
    );
    assert_eq!(
        stdout, "nested-ok\nlarge-ok\nok\n",
        "vm string-evaluator must build a large/nested manifest without dropping the write"
    );
}
