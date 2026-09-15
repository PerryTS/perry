//! #10287: a receiver carrying a property descriptor keeps the store fast
//! paths for its OTHER keys, and a receiver whose prototype was set explicitly
//! is vetted per key instead of wholesale. Both are semantics-preserving, so
//! every fixture here pins output verified against Node 26.

use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(root: &Path, entry: &str) -> PathBuf {
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_owned());
    let output = root.join("native");
    let result = Command::new(compiler)
        .current_dir(root)
        .args(["compile", entry, "--no-cache", "--no-auto-optimize"])
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime)
        .output()
        .expect("compile fixture");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

fn run(root: &Path, source: &str) -> String {
    std::fs::write(root.join("entry.js"), source).unwrap();
    let binary = compile(root, "entry.js");
    let result = Command::new(&binary).output().expect("run fixture");
    assert!(
        result.status.success(),
        "exit {:?}\n{}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

/// The zod `$constructor` shape: one non-writable, non-enumerable descriptor
/// installed before the receiver's methods are assigned. The descriptor must
/// keep rejecting its own key while every other key stores normally.
#[test]
fn a_descriptor_does_not_change_the_semantics_of_other_keys() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"
const make = (n) => {
  const o = {};
  Object.defineProperty(o, "_zod", { value: n, enumerable: false });
  for (let i = 0; i < 12; i++) o["m" + i] = n * 100 + i;
  return o;
};
const objs = [make(1), make(2), make(3)];
console.log(JSON.stringify(Object.keys(objs[2])));
console.log(JSON.stringify(objs.map((o) => o.m7)) + " " + JSON.stringify(objs.map((o) => o._zod)));
objs[0]._zod = 99;
console.log("sloppy " + objs[0]._zod);
try { "use strict"; (() => { "use strict"; objs[0]._zod = 5; })(); console.log("strict ok"); }
catch (e) { console.log("strict " + e.constructor.name); }
console.log(JSON.stringify(objs[1]) + " " + JSON.stringify(Object.getOwnPropertyNames(objs[1])));
const d = Object.getOwnPropertyDescriptor(objs[1], "_zod");
console.log(`${d.value} w=${d.writable} e=${d.enumerable} c=${d.configurable}`);
"#,
    );
    assert_eq!(
        out,
        "[\"m0\",\"m1\",\"m2\",\"m3\",\"m4\",\"m5\",\"m6\",\"m7\",\"m8\",\"m9\",\"m10\",\"m11\"]\n\
         [107,207,307] [1,2,3]\n\
         sloppy 1\n\
         strict TypeError\n\
         {\"m0\":200,\"m1\":201,\"m2\":202,\"m3\":203,\"m4\":204,\"m5\":205,\"m6\":206,\"m7\":207,\
         \"m8\":208,\"m9\":209,\"m10\":210,\"m11\":211} \
         [\"_zod\",\"m0\",\"m1\",\"m2\",\"m3\",\"m4\",\"m5\",\"m6\",\"m7\",\"m8\",\"m9\",\"m10\",\"m11\"]\n\
         2 w=false e=false c=false\n"
    );
}

/// Receivers that share a construction sequence share a shape; one of them
/// diverging (an accessor over a key its siblings hold as data) must not
/// change what the siblings see.
#[test]
fn a_diverging_sibling_keeps_its_own_descriptor_state() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"
const make = (n) => {
  const o = {};
  Object.defineProperty(o, "_zod", { value: n, enumerable: false });
  for (let i = 0; i < 6; i++) o["m" + i] = n * 10 + i;
  return o;
};
const objs = [make(1), make(2), make(3)];
Object.defineProperty(objs[1], "m3", { get: () => "getter", configurable: true });
console.log(objs.map((o) => String(o.m3)).join("|"));
objs[0].m3 = -1;
objs[2].m3 = -2;
console.log(objs.map((o) => String(o.m3)).join("|"));
console.log(objs.map((o) => {
  const d = Object.getOwnPropertyDescriptor(o, "m3");
  return d.get ? "accessor" : "data:" + d.value;
}).join("|"));
"#,
    );
    assert_eq!(
        out,
        "13|getter|33\n-1|getter|-2\ndata:-1|accessor|data:-2\n"
    );
}

/// A function-constructed receiver has an explicitly recorded prototype. Its
/// inherited setter must still run, and an inherited writable data property
/// must still shadow onto the receiver.
#[test]
fn a_custom_prototype_still_intercepts_the_keys_it_owns() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"
function F() {}
F.prototype = { set x(v) { this._x = v * 2; }, get x() { return this._x; }, inherited: 7 };
const objs = [];
for (let i = 0; i < 20; i++) {
  const o = new F();
  Object.defineProperty(o, "_zod", { value: i, enumerable: false });
  o.a = i; o.x = i; o.inherited = i;
  objs.push(o);
}
const o = objs[19];
console.log(`${o.x} ${o._x} ${o.a} ${o.inherited}`);
console.log(JSON.stringify(Object.getOwnPropertyNames(o)));
console.log(String(Object.getOwnPropertyDescriptor(F.prototype, "inherited").value));
const proto = {};
Object.defineProperty(proto, "ro", { value: 1, writable: false });
const child = Object.create(proto);
child.ro = 5; child.other = 6;
console.log(`${child.ro} own=${child.hasOwnProperty("ro")} other=${child.other}`);
"#,
    );
    assert_eq!(
        out,
        "38 38 19 19\n[\"_zod\",\"a\",\"_x\",\"inherited\"]\n7\n1 own=false other=6\n"
    );
}

/// A Proxy anywhere on the chain owns the write, and a frozen prototype makes
/// an inherited key read-only. Both must survive the per-key vetting.
#[test]
fn proxy_and_frozen_prototypes_keep_the_slow_walk() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"
const target = {};
const p = new Proxy(target, { set(t, k, v) { t[k] = v * 2; return true; } });
const viaProxy = Object.create(p);
Object.defineProperty(viaProxy, "_zod", { value: 0, enumerable: false });
viaProxy.z = 21;
console.log(`${target.z} own=${viaProxy.hasOwnProperty("z")}`);
const frozen = Object.freeze({ f: 1 });
const child = Object.create(frozen);
Object.defineProperty(child, "_zod", { value: 0, enumerable: false });
child.f = 2; child.g = 3;
console.log(`${child.f} own=${child.hasOwnProperty("f")} g=${child.g}`);
const sealed = {};
Object.defineProperty(sealed, "_zod", { value: 1 });
sealed.a = 1;
Object.seal(sealed);
sealed.b = 2; sealed.a = 9;
console.log(`${JSON.stringify(Object.keys(sealed))} a=${sealed.a} b=${sealed.b}`);
"#,
    );
    assert_eq!(
        out,
        "42 own=false\n1 own=false g=3\n[\"a\"] a=9 b=undefined\n"
    );
}

/// Deleting and re-adding keys, and installing a later descriptor, must keep
/// insertion order and attributes exact on a receiver that already shares a
/// shape with its siblings.
#[test]
fn delete_readd_and_later_descriptors_keep_order_and_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"
const make = () => {
  const o = {};
  Object.defineProperty(o, "_zod", { value: 1, enumerable: false, configurable: true });
  o.k1 = 1; o.k2 = 2; o.k3 = 3;
  return o;
};
const a = make(), b = make();
delete a.k1;
a.k1 = 9; a.k4 = 10;
console.log(JSON.stringify(Object.keys(a)) + " " + JSON.stringify([a.k1, a.k2, a.k3, a.k4]));
console.log(JSON.stringify(Object.keys(b)) + " " + JSON.stringify([b.k1, b.k2, b.k3]));
delete a._zod;
console.log(String(Object.getOwnPropertyDescriptor(a, "_zod")) + " " +
  String(Object.getOwnPropertyDescriptor(b, "_zod").value));
Object.defineProperty(b, "k2", { value: 42, writable: false });
b.k2 = 7;
console.log(`${b.k2} ${JSON.stringify(Object.keys(b))}`);
"#,
    );
    assert_eq!(
        out,
        "[\"k2\",\"k3\",\"k1\",\"k4\"] [9,2,3,10]\n[\"k1\",\"k2\",\"k3\"] [1,2,3]\n\
         undefined 1\n42 [\"k1\",\"k2\",\"k3\"]\n"
    );
}
