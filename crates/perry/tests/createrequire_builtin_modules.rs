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
