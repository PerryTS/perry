//! Private fields and private methods on instances of classes that extend a
//! native class (`http.Agent`). Such an instance is a native handle, not a
//! heap object, and the private-element holder answered "none" for it, so
//! `class HttpProxyAgent extends Agent { #m() {} }` (agent-base, under npm's
//! fetch) threw "Cannot initialize private methods of class ... on this
//! object". The holder is the handle's property bag, as for its named
//! expando properties.

use std::process::Command;

const SOURCE: &str = r##"
import * as http from "http";

class Tracked extends http.Agent {
  #n = 1;
  #seen: string[] = [];
  static #made = 0;
  constructor(opts?: any) { super(opts); Tracked.#made++; }
  #bump() { return ++this.#n; }
  get #label() { return "tracked#" + this.#n; }
  visit(host: string) { this.#seen.push(host); return this.#bump(); }
  report() { return this.#label + ":" + this.#seen.join(","); }
  static made() { return Tracked.#made; }
  static is(x: any) { return #n in x; }
}

const a = new Tracked({ keepAlive: true });
const b = new Tracked();
console.log("visit", a.visit("x"), a.visit("y"), b.visit("z"));
console.log("report", a.report(), b.report());
console.log("static", Tracked.made(), Tracked.is(a), Tracked.is(new http.Agent()), Tracked.is({}));
console.log("agent", a instanceof http.Agent, typeof a.destroy);
console.log("keys", JSON.stringify(Object.keys(a).filter((k) => k.startsWith("#"))));
try {
  (Tracked.prototype as any).visit.call(new http.Agent(), "w");
  console.log("brand none");
} catch (e) {
  console.log("brand", (e as Error).constructor.name);
}
a.destroy();
b.destroy();
"##;

const EXPECTED: &str = "visit 2 3 2
report tracked#3:x,y tracked#2:z
static 2 true false false
agent true function
keys []
brand TypeError
";

#[test]
fn private_elements_live_on_native_handle_instances() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let source = dir.path().join("private_native.ts");
    let binary = dir.path().join("private_native_bin");
    std::fs::write(&source, SOURCE).expect("write fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(
        output.status.success(),
        "fixture compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let run = Command::new(&binary).output().expect("run fixture");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        EXPECTED,
        "stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
