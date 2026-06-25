//! Regression test for #5437 (Next.js dynamic PAGE routes 500 `Error: Invalid
//! URL`): a minified class constructor collapses every field assignment into
//! ONE comma-`Seq` expression statement — Next.js's `BaseNextRequest` is
//! `constructor(a,b,c){this.method=a,this.url=b,this.body=c}`.
//!
//! The ctor-this-field scan in `lower_class_decl` only matched
//! `Stmt::Expr(Assign)`, so it detected ZERO declared fields for such a
//! constructor: `method`/`url`/`body` never entered `packed_keys`. The subclass
//! instance was then allocated with too-few inline slots, so a capturing child's
//! synthesized `__perry_cap_*` hidden fields were prepended AHEAD of the
//! (missing) real slots — Object.keys reported the captures before the real
//! fields and `e.url` could read undefined.
//!
//! Fix: descend through `Seq`/`Paren` (and chained-assign RHS) in the
//! ctor-field scan so each comma-separated `this.x = …` registers the same as a
//! standalone assignment statement. The real fields then get stable
//! packed-keys slots ahead of any synthesized capture fields.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, src).expect("write entry");

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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// The comma-`Seq` constructor's `this.x = …` assignments must each be detected
/// as own fields, so they read back correctly and surface as Object.keys in
/// source order — exactly as Node does.
#[test]
fn comma_seq_ctor_assignments_become_own_fields() {
    let stdout = compile_and_run(
        r#"
class f { constructor(a, b, c) { this.method = a, this.url = b, this.body = c } }
const x = new f("GET", "/posts/123", null);
console.log("url=" + x.url);
console.log("method=" + x.method);
console.log("keys=" + Object.keys(x).join(","));
"#,
    );
    assert_eq!(
        stdout, "url=/posts/123\nmethod=GET\nkeys=method,url,body\n",
        "comma-Seq ctor `this.method=a,this.url=b,this.body=c` must register \
         method/url/body as own fields in source order"
    );
}

/// A capturing subclass extending a comma-`Seq`-ctor parent: the parent's
/// method/url/body must inherit correctly and read back as valid values (the
/// Next.js NodeNextRequest-extends-BaseNextRequest shape). The synthesized
/// capture hidden field must NOT shift the real parent-field slots.
#[test]
fn capturing_child_of_comma_seq_parent_keeps_parent_fields() {
    let stdout = compile_and_run(
        r#"
var c, d = { META: Symbol("meta") };
class f { constructor(a, b, c) { this.method = a, this.url = b, this.body = c } }
class h extends f {
  static #a = (c = d.META);
  constructor(req) {
    var b;
    super(req.method.toUpperCase(), req.url, req),
      this._req = req,
      this.headers = req.headers,
      this.fetchMetrics = null,
      this[c] = req.meta,
      this.streaming = false;
  }
}
const req = { method: "get", url: "/abc", headers: { a: 1 }, meta: 1 };
const n = new h(req);
console.log("method=" + n.method);
console.log("url=" + n.url);
console.log("body?" + (!!n.body));
console.log("streaming=" + n.streaming);
"#,
    );
    assert_eq!(
        stdout, "method=GET\nurl=/abc\nbody?true\nstreaming=false\n",
        "a capturing child of a comma-Seq-ctor parent must inherit \
         method/url/body and read them back as valid values — #5437"
    );
}
