//! Private fields and private methods on instances of classes that extend a
//! built-in with its own cell kind (Map, Set, Error, Date, Promise). Their
//! instances are not ordinary shaped objects, and the private-element holder
//! answered "none" for them, so `class CIMap extends Map { #keys = new Map() }`
//! (npm's arborist) threw "Cannot initialize #keys on this object" and
//! private methods threw "Cannot initialize private methods of class CIMap".
//! The holder is the cell's property bag, as it is for a function.

use std::process::Command;

const SOURCE: &str = r#"
class CIMap extends Map<any, any> {
  #keys = new Map<any, any>();
  constructor(items: any[] = []) { super(); for (const [k, v] of items) this.set(k, v); }
  #norm(key: any) { return typeof key === "string" ? key.toLowerCase() : key; }
  get(key: any) { const n = this.#norm(key); return this.#keys.has(n) ? super.get(this.#keys.get(n)) : undefined; }
  set(key: any, val: any) {
    const n = this.#norm(key);
    if (this.#keys.has(n)) super.delete(this.#keys.get(n));
    this.#keys.set(n, key);
    return super.set(key, val);
  }
  static isCI(x: any) { return #keys in x; }
}
const m = new CIMap([["Foo", 1], ["FOO", 2]]);
console.log("map", m.get("foo"), m.size, m instanceof Map, CIMap.isCI(m), CIMap.isCI(new Map()));
console.log("keys", JSON.stringify(Object.keys(m)), JSON.stringify(Object.getOwnPropertyNames(m)));

class TagSet extends Set<any> {
  #tag: string;
  constructor(tag: string) { super(); this.#tag = tag; }
  get #label() { return "<" + this.#tag + ">"; }
  label() { return this.#label + this.size; }
}
const s = new TagSet("t"); s.add(1);
console.log("set", s.label());

class CodedError extends Error {
  #code: number;
  constructor(code: number) { super("e" + code); this.#code = code; }
  #describe() { return this.message + ":" + this.#code; }
  describe() { return this.#describe(); }
}
console.log("error", new CodedError(7).describe());

class Stamp extends Date {
  #n = 3;
  n() { return this.#n; }
}
console.log("date", new Stamp(0).n());

class Brand extends Map<any, any> { #m() { return 5; } call(o: any) { return o.#m(); } }
try { new Brand().call(new Map()); console.log("brand none"); } catch (e) { console.log("brand", (e as Error).constructor.name); }
"#;

const EXPECTED: &str = "map 2 1 true true false
keys [] []
set <t>1
error e7:7
date 3
brand TypeError
";

#[test]
fn private_elements_live_on_exotic_cell_instances() {
    let dir = tempfile::tempdir().expect("create fixture directory");
    let source = dir.path().join("private_exotic.ts");
    let binary = dir.path().join("private_exotic_bin");
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
