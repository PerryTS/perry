//! Regression test for #10662: `axios` throws `TypeError: Class extends
//! value is not a constructor` at module-init time because its transitive
//! dependency chain `https-proxy-agent` -> `agent-base` hits a TypeScript
//! declaration-merging shape Perry's HIR does not lower correctly.
//!
//! `agent-base`'s real source (`src/index.ts`) is:
//!
//! ```ts
//! function createAgent(opts) { return new createAgent.Agent(opts); }
//! namespace createAgent {
//!   export class Agent extends EventEmitter { ... }
//! }
//! export = createAgent;
//! ```
//!
//! `perry.compilePackages` prefers compiling a package's raw TypeScript
//! source over its published JS emit (`resolve_package_source_entry`), and
//! picks `src/index.ts` here since `agent-base` ships both. Perry's HIR
//! lowers the namespace's exported `Agent` class as a `StaticFieldSet`
//! against a synthetic class entity that is NOT the same runtime object
//! `export =` ends up exporting: `require("agent-base").Agent` reads back
//! as `undefined`, and `https-proxy-agent`'s `class HttpsProxyAgent extends
//! agent_base_1.Agent` throws.
//!
//! The fix (`is_hybrid_cjs_emit_input` in `resolve.rs`, alongside its
//! existing #6586 ESM+CJS-epilogue trigger) detects the namespace-block +
//! `export =` shape and falls back to the package's compiled JS emit
//! instead — the same file Node itself runs (raw `namespace`/`export =`
//! isn't valid under `--experimental-strip-types` either, so a package
//! built this way is never executed from its `.ts` source in practice).
//!
//! This fixture mirrors the real shape exactly enough to reproduce the bug
//! (namespace-merged-with-function class extending a native `EventEmitter`,
//! consumed by a downstream CJS `class X extends pkg.Agent`) without
//! depending on the actual npm packages.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_merged_function_export_equals_falls_back_to_js_emit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-10662-consumer",
  "private": true,
  "perry": {
    "compilePackages": ["agent-base-like"],
    "allow": { "compilePackages": ["agent-base-like"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    // `agent-base`'s exact shape: a package.json "main" pointing at the
    // compiled JS, PLUS a `src/index.ts` Perry would otherwise prefer.
    let pkg = root.join("node_modules").join("agent-base-like");
    std::fs::create_dir_all(pkg.join("src")).expect("mkdir src");
    std::fs::create_dir_all(pkg.join("dist").join("src")).expect("mkdir dist/src");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "agent-base-like", "version": "1.0.0", "main": "dist/src/index", "typings": "dist/src/index" }"#,
    )
    .expect("write agent-base-like package.json");

    // The raw TS source: `namespace createAgent { export class Agent
    // extends EventEmitter { ... } }` merged onto `function createAgent()`,
    // exported via `export =`. Perry cannot correctly lower this shape
    // today (#10662) — the JS-emit fallback is what makes it work.
    std::fs::write(
        pkg.join("src").join("index.ts"),
        r#"import { EventEmitter } from 'events';

function createAgent(opts?: any) {
  return new createAgent.Agent(opts);
}

namespace createAgent {
  export class Agent extends EventEmitter {
    public tag: string;
    constructor(opts?: any) {
      super();
      this.tag = "agent-tag";
    }
  }
}

export = createAgent;
"#,
    )
    .expect("write agent-base-like src/index.ts");

    // The compiled emit `tsc` would actually publish — plain CJS, no
    // namespace-merge complexity, `require()`d by Node in practice.
    std::fs::write(
        pkg.join("dist").join("src").join("index.js"),
        r#""use strict";
const events_1 = require("events");
function createAgent(opts) {
    return new createAgent.Agent(opts);
}
(function (createAgent) {
    class Agent extends events_1.EventEmitter {
        constructor(opts) {
            super();
            this.tag = "agent-tag";
        }
    }
    createAgent.Agent = Agent;
})(createAgent || (createAgent = {}));
module.exports = createAgent;
"#,
    )
    .expect("write agent-base-like dist/src/index.js");

    // The `https-proxy-agent` half: a downstream CJS file (already
    // "compiled" — no namespace complexity of its own) whose class extends
    // the namespace-merged package's exported member.
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import "./downstream.cjs";
"#,
    )
    .expect("write entry");
    std::fs::write(
        root.join("downstream.cjs"),
        r#"'use strict';
const pkg = require("agent-base-like");
if (typeof pkg !== "function") {
  throw new Error("expected agent-base-like's export = value to be callable, got " + typeof pkg);
}
if (typeof pkg.Agent !== "function") {
  throw new Error("expected pkg.Agent to be a constructor, got " + typeof pkg.Agent);
}
class Downstream extends pkg.Agent {
  constructor() {
    super();
    this.extra = "downstream";
  }
}
const d = new Downstream();
let seen = 0;
d.on("x", () => { seen++; });
d.emit("x");
console.log("tag:", d.tag, "extra:", d.extra, "events:", seen);
"#,
    )
    .expect("write downstream.cjs");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (namespace-merge JS-emit fallback regressed?)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "compiled binary failed (agent-base-like's namespace-merged Agent should have resolved via the dist/ fallback)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, "tag: agent-tag extra: downstream events: 1\n",
        "downstream class extending a namespace-merged native-base subclass must construct and behave correctly"
    );
}
