//! Regression test: `createRequire(...)(spec)` must resolve every implemented
//! Node built-in module, not just a subset.
//!
//! `tls` (and `dgram`/`domain`/`vm`/`repl`/`sqlite`/`inspector`) are fully
//! implemented native modules (runtime registry buckets + dispatch + exports),
//! but they were missing from the `supported_require_builtin` allowlist in
//! `module_require.rs`, so `require('tls')` via `createRequire` was rejected with
//! `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE` ("package/file require('tls') is not
//! supported"). This blocked `claude-code --help`: the bundled follow-redirects
//! module does `const tls = require('tls')` through a `createRequire` bridge.

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

/// #6644 (pi wall #3): `require('node:diagnostics_channel')` through
/// `createRequire` threw `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE` — the module is
/// implemented as a node_submodules spec (real pub/sub channel registry) but was
/// missing from the `supported_require_builtin` allowlist and never routed to
/// `js_node_submodule_namespace`. lru-cache's node build requires it through the
/// esbuild createRequire banner shim, so any ESM bundle of CJS deps hit this.
/// Covers both the `node:`-prefixed and bare spellings, real pub/sub between
/// handles from each spelling, the tracingChannel shape, another
/// `node:`-prefixed builtin (`node:path`) through the same require, and the
/// `process.getBuiltinModule` sibling path.
#[test]
fn createrequire_resolves_diagnostics_channel_and_node_prefixed_builtins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);

const dc1 = require("node:diagnostics_channel");
const dc2 = require("diagnostics_channel");
console.log("shapes:", typeof dc1.channel, typeof dc1.subscribe, typeof dc1.unsubscribe, typeof dc1.hasSubscribers, typeof dc1.tracingChannel);
console.log("fresh:", dc1.hasSubscribers("never-subscribed"));

const seen: string[] = [];
const onMsg = (message: any, name: string) => { seen.push(`${name}:${JSON.stringify(message)}`); };
dc1.subscribe("pi.test", onMsg);
console.log("subscribed:", dc2.hasSubscribers("pi.test"));
const ch = dc2.channel("pi.test");
console.log("channel.hasSubscribers:", ch.hasSubscribers);
ch.publish({ n: 1 });
dc1.channel("pi.test").publish({ n: 2 });
console.log("seen:", seen.join(" | "));
console.log("unsubscribe:", dc2.unsubscribe("pi.test", onMsg));
console.log("after:", dc1.hasSubscribers("pi.test"));

const tc = dc1.tracingChannel("pi.trace");
console.log("tracing:", typeof tc.traceSync, typeof tc.tracePromise, typeof tc.traceCallback, tc.hasSubscribers);

const path = require("node:path");
console.log("path:", typeof path.join, path.join("a", "b"));

const gbm = process.getBuiltinModule("node:diagnostics_channel");
console.log("getBuiltinModule:", typeof gbm.channel, gbm.hasSubscribers("z"));
"#,
    );
    assert_eq!(
        stdout,
        "shapes: function function function function function\n\
         fresh: false\n\
         subscribed: true\n\
         channel.hasSubscribers: true\n\
         seen: pi.test:{\"n\":1} | pi.test:{\"n\":2}\n\
         unsubscribe: true\n\
         after: false\n\
         tracing: function function function false\n\
         path: function a/b\n\
         getBuiltinModule: function false\n"
    );
}

#[test]
fn createrequire_resolves_tls_and_other_implemented_builtins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const tls = require("tls");
console.log("tls:", typeof tls, typeof tls.createSecureContext === "function");
console.log("dgram:", typeof require("dgram"));
console.log("vm:", typeof require("vm"));
console.log("domain:", typeof require("domain"));
console.log("ok");
"#,
    );
    assert_eq!(
        stdout,
        "tls: object true\ndgram: object\nvm: object\ndomain: object\nok\n"
    );
}

/// #6651 (pi wall #5): `require('node:v8')` through `createRequire` threw
/// `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE` — `v8` has a full native module
/// (`node_v8.rs`, nm dispatch bucket, static-import support) but was missing
/// from BOTH runtime dynamic allowlists. The pi bundle's esbuild createRequire
/// banner funnels `__require("node:v8")` through this path. Expected output
/// captured from `node v26.3.0` (byte-identical).
#[test]
fn createrequire_resolves_v8_in_both_spellings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);

const v8a = require("node:v8");
const v8b = require("v8");
console.log("serialize:", typeof v8a.serialize, typeof v8b.deserialize);
const round = v8b.deserialize(v8a.serialize({ pi: 5, arr: [1, 2, 3] }));
console.log("roundtrip:", JSON.stringify(round));
console.log("getBuiltinModule:", typeof process.getBuiltinModule("node:v8").serialize);
"#,
    );
    assert_eq!(
        stdout,
        "serialize: function function\n\
         roundtrip: {\"pi\":5,\"arr\":[1,2,3]}\n\
         getBuiltinModule: function\n"
    );
}

/// #6651 family regression guard, fixture side: every entry of
/// `module.builtinModules` (the runtime's `MODULE_BUILTIN_MODULES`, which the
/// dynamic allowlists now derive from) must resolve through BOTH
/// `createRequire(...)`'s `require` and `process.getBuiltinModule`, in every
/// spelling Node accepts — and must keep failing in the spellings Node
/// rejects (bare scheme-only names) or Perry does not implement (`_`-prefixed
/// legacy internals, whose error must still name the module).
#[test]
fn createrequire_and_get_builtin_module_reach_every_builtin_modules_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const failures: string[] = [];
const resolvable = (t: string) => t === "object" || t === "function";
let checked = 0;
const moduleNs = require("node:module");
const builtins = moduleNs.builtinModules;
for (const entry of builtins) {
  if (entry.startsWith("_")) {
    // Unimplemented legacy internals: must throw, naming the module.
    try {
      require(entry);
      failures.push(entry + ": internal resolved");
    } catch (e: any) {
      if (!String(e && e.message).includes(entry)) failures.push(entry + ": error hides module name");
    }
    if (process.getBuiltinModule(entry) !== undefined) failures.push(entry + ": gbm resolved internal");
    continue;
  }
  const bare = entry.startsWith("node:") ? entry.slice(5) : entry;
  const spellings = entry.startsWith("node:") ? [entry] : [entry, "node:" + entry];
  for (const s of spellings) {
    checked++;
    try {
      if (!resolvable(typeof require(s))) failures.push(s + ": require gave non-namespace");
    } catch (e: any) {
      failures.push(s + ": require threw " + (e && e.code));
    }
    if (!resolvable(typeof process.getBuiltinModule(s))) failures.push(s + ": gbm gave non-namespace");
  }
  if (entry.startsWith("node:")) {
    // Scheme-only builtin: the bare spelling is an npm name (Node parity).
    try {
      require(bare);
      failures.push(bare + ": scheme-only resolved bare");
    } catch (e: any) {
      if (!String(e && e.message).includes(bare)) failures.push(bare + ": error hides module name");
    }
    if (process.getBuiltinModule(bare) !== undefined) failures.push(bare + ": gbm resolved bare scheme-only");
  }
}
console.log("checked enough:", checked >= 100);
console.log("failures:", JSON.stringify(failures));
"#,
    );
    assert_eq!(stdout, "checked enough: true\nfailures: []\n");
}
