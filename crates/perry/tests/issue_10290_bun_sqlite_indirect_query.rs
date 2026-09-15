//! Regression test for #10290: `bun:sqlite` `Database.query()` answered
//! `undefined` whenever the database reached the call site through any
//! indirection.
//!
//! `db.query(sql)` lowers through codegen's static NATIVE_MODULE_TABLE entry,
//! which is gated on proving the receiver's class is `bun:sqlite`'s
//! `Database`. The moment the database arrives as a field, an interface-typed
//! parameter, a generator return or a driver object, that proof is gone and
//! the call falls to the dynamic handle tower in
//! `common/dispatch/method_dispatch.rs`.
//!
//! `dispatch_node_sqlite_database_method` has always implemented `query`,
//! `run` and `transaction` — the same handle registry backs both the
//! `node:sqlite` and `bun:sqlite` bindings — but those three names were
//! missing from the `matches!` gate in front of it, so the tower fell all the
//! way through and returned `undefined`. A silent `undefined` from `query`,
//! not a throw, which is why it surfaced far from the cause.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// `runGen` launders the database through a generator, which is enough to
/// erase the static class the native-table lowering needs. `ind === db`.
const SOURCE: &str = r#"
import { Database } from "bun:sqlite"

const show = (n: string, f: () => any) => {
  try {
    const r = f()
    console.log(n, r === undefined ? "undefined" : (typeof r === "object" ? "object" : JSON.stringify(r)))
  } catch (e: any) {
    console.log(n, "THREW", e?.message)
  }
}

const db: any = new Database(":memory:")
db.run("create table t (a integer, b text)")
db.run("insert into t values (1,'x')")

function* gen(): any { return db }
function runGen(g: any) { const it = g(); let r = it.next(); while (!r.done) r = it.next(r.value); return r.value }
const ind: any = runGen(gen)

console.log("same object:", ind === db)
show("direct   query", () => db.query("select * from t"))
show("indirect query", () => ind.query("select * from t"))
show("indirect prepare", () => ind.prepare("select * from t"))
show("indirect run", () => ind.run("insert into t values (2,'y')"))
// Reading the property only exercises property dispatch; the gate under
// test is the METHOD path, so call the wrapper `transaction(fn)` returns.
show("indirect transaction", () => ind.transaction(() => "committed")())
console.log("rows", JSON.stringify(ind.query("select * from t").all()))
"#;

/// Byte-for-byte what bun prints.
const EXPECTED: &str = "\
same object: true
direct   query object
indirect query object
indirect prepare object
indirect run object
indirect transaction \"committed\"
rows [{\"a\":1,\"b\":\"x\"},{\"a\":2,\"b\":\"y\"}]
";

#[test]
fn bun_sqlite_query_survives_indirection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), SOURCE).unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        // No `--platform bun`: the flag pulls the `net` well-known wrapper in,
        // and a wrapper archive built outside this cargo invocation is refused
        // at link (#7629). `bun:sqlite` resolves on either platform, and the
        // handle tower under test is platform-independent.
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "bun:sqlite probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8(run.stdout).expect("UTF-8 stdout");
    assert!(
        !stdout.contains("undefined"),
        "no Database method may answer undefined through indirection; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "the dynamic handle tower must reach the same Database implementation \
         the static native-table lowering does"
    );
}
