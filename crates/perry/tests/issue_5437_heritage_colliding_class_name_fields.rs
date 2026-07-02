//! Regression test for #5437 (Next.js Wall #7b, dynamic PAGE routes 500
//! `Error: Invalid URL`): a minified bundle declares the SAME class name in
//! several turbopack factory closures (Next.js's chunk has `class f{...}`
//! three times). When a capturing child `class h extends f` is declared in a
//! LATER factory, Phase-1.5 disambiguates that factory's `f` by renaming it
//! (`f` -> `f$0`) so it registers as a distinct class — but the child's
//! `extends f` was binding to the RAW name, which resolves to the FIRST
//! (wrong) `f`. The codegen parent-chain walk then pulls the wrong class's
//! fields into `packed_keys`, dropping the real parent's `method`/`url`/`body`.
//!
//! Concretely, Next.js's `NodeNextRequest (h) extends BaseNextRequest (f)`
//! lost `f`'s `method`/`url`/`body` fields, so `new NodeNextRequest(req).url`
//! read `undefined` and the app-page render's `if (!e.url) throw "Invalid URL"`
//! 500'd every dynamic page route (`/posts/[id]`, `/fetcher`).
//!
//! Fix: resolve the Ident super-class name through the active scope-local class
//! renames (`resolve_class_name`) in both `lower_class_decl` and
//! `lower_class_from_ast`, so heritage binds to the SAME disambiguated class the
//! parent decl registered under. Identity for non-colliding names.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn heritage_binds_to_disambiguated_same_named_parent_keeping_its_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
"use strict";
const MODS = { 85729: { NEXT_REQUEST_META: Symbol("next.req.meta") } };
const a = { i: (id) => MODS[id] };

// Factory A: a DECOY `class f` with NO method/url/body, declared FIRST so a
// naive by-name `extends f` resolution picks this one.
const modA = ((a) => {
  class f { constructor() { this.decoyA = 1; this.decoyB = 2; } }
  class q extends f { constructor() { super(); this.qf = 9; } }
  return { Q: q };
})(a);

// Factory B: the REAL base `class f` (method/url/body) + a capturing child
// `class h extends f`. Phase-1.5 renames this `f` to a unique name; the child's
// `extends f` must bind to THAT renamed class, not factory A's decoy.
const modB = ((a) => {
  var c, d = a.i(85729);
  class f { constructor(m, u, b) { this.method = m; this.url = u; this.body = b; } }
  class h extends f {
    static #a = (c = d.NEXT_REQUEST_META);
    constructor(req) {
      super(req.method.toUpperCase(), req.url, req);
      this._req = req;
      this.headers = req.headers;
      this.fetchMetrics = req.fetchMetrics;
      this[c] = req[d.NEXT_REQUEST_META] || {};
      this.streaming = false;
    }
  }
  return { NodeNextRequest: h };
})(a);

const make = (b) => new modB.NodeNextRequest(b);
const req = { method: "get", url: "/posts/123", headers: { a: 1 }, fetchMetrics: null };
const n = make(req);
console.log("url=" + n.url);
console.log("method=" + n.method);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "url=/posts/123\nmethod=GET\n",
        "a capturing child extending a disambiguated same-named parent must \
         inherit THAT parent's ctor-assigned fields (method/url/body), not a \
         decoy same-named class's fields — #5437 NodeNextRequest Wall #7b"
    );
}
