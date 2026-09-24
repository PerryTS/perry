//! The key-add store path (`o.k = v` where `k` is not yet own): the inline
//! hit (`perry-codegen/src/expr/put_value_store_ic.rs`, `emit_key_add_hit`)
//! and the runtime memo behind it (`perry-runtime/src/proxy/put_value/
//! packed_add.rs`).
//!
//! Each test primes a key-add site hot, then changes the world in a way the
//! memo must notice, and compares the output with node's. A broken hit that
//! falls through to the miss is correct and only slow, so every test also
//! asserts from the store census (`PERRY_STORE_CENSUS`, compile and run time)
//! that the path it guards actually RAN.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` with the store census, run it, and return (stdout, the
/// inline add hits, the runtime memo serves).
fn run(source: &str) -> (String, u64, u64) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_STORE_CENSUS", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .env("PERRY_STORE_CENSUS", "1")
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(run.status.success(), "binary failed ({:?})\nstderr:\n{stderr}", run.status);
    let count = |name: &str| -> u64 {
        stderr
            .split_whitespace()
            .find_map(|w| w.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
            .unwrap_or(0)
    };
    let memo = count("rt.add.memo_inline") + count("rt.add.memo_spill");
    (
        String::from_utf8_lossy(&run.stdout).trim().to_owned(),
        count("emit.add.inline_hit"),
        memo,
    )
}

/// A key-add site primed on `new F` / `new K`, then an inherited setter and an
/// inherited non-writable property appear for its keys. Sabotage: the emitted
/// hit skips the chain generation compare -> the setter is skipped and an own
/// property is created.
#[test]
fn a_setter_or_non_writable_property_appearing_on_the_prototype_is_honoured() {
    let (stdout, hits, memo) = run(r#"// A key-add site primed hot, then an inherited setter / non-writable property
// appears for its key: the next store must run the setter / be refused.
function F(this: any, v: number) { this.a = v; this.b = v + 1; }
class K { constructor(v: number) { (this as any).p = v; (this as any).q = v; } }
let s = 0;
for (let i = 0; i < 2000; i++) { const o = new (F as any)(i); s += o.b; const k: any = new K(i); s += k.q; }
const log: number[] = [];
Object.defineProperty((F as any).prototype, "b", { set(v: number) { log.push(v); }, configurable: true });
Object.defineProperty(K.prototype, "q", { set(v: number) { log.push(-v); }, configurable: true });
const f: any = new (F as any)(10);
const k: any = new K(20);
Object.defineProperty((F as any).prototype, "a", { value: 99, writable: false, configurable: true });
const logged = log.join(",");
const g: any = Object.create((F as any).prototype);
try { (F as any).call(g, 30); } catch (e) { log.push(-1); }
console.log(s, logged, Object.prototype.hasOwnProperty.call(f, "b"), Object.prototype.hasOwnProperty.call(k, "q"), f.a, Object.prototype.hasOwnProperty.call(g, "a"), g.a);
"#);
    assert_eq!(stdout, r#"4000000 11,-20 false false 10 false 99"#);
    assert!(hits > 0, "the inline add hit never ran (hits={hits} memo={memo})");
}

/// Integrity-restricted receivers of the primed pre-shape's key list, at the
/// primed site. Sabotage: `preventExtensions` keeps the ShapeId -> the add
/// lands on a non-extensible object.
#[test]
fn frozen_sealed_and_non_extensible_receivers_refuse_the_add() {
    let (stdout, hits, memo) = run(r#"// One key-add site primed hot on extensible receivers, then fed frozen /
// sealed / non-extensible receivers of the same key list at the SAME site
// (one loop, so an inlining compiler cannot split the site): the add must be
// refused.
function mk(i: number): any { return { x: i }; }
const objs: any[] = [];
for (let i = 0; i < 2000; i++) objs.push(mk(i));
objs.push(Object.freeze(mk(1)), Object.seal(mk(2)), Object.preventExtensions(mk(3)), mk(4));
for (let i = 0; i < objs.length; i++) {
  const o = objs[i];
  try { o.z = i; } catch (e) { /* strict mode throws, sloppy ignores */ }
}
let s = 0; for (let i = 0; i < 2000; i++) s += objs[i].z;
const tail = objs.slice(2000).map((o: any) => ("z" in o) + ":" + Object.keys(o).join("")).join(" ");
console.log(s, tail);
"#);
    assert_eq!(stdout, r#"1999000 false:x false:x false:x true:xz"#);
    assert!(hits + memo > 0, "the add memo never ran (hits={hits} memo={memo})");
}

/// `setPrototypeOf(this, Q)` between two adds of one construction. Two
/// independent guards protect it: the prototype is part of the ShapeId, and
/// recording a per-instance prototype moves the verdict generation. Sabotage
/// (both): a prototype change keeps the ShapeId AND the hit skips the
/// generation compare -> Q's setter is skipped.
#[test]
fn a_prototype_change_between_two_adds_reaches_the_new_chain() {
    let (stdout, hits, memo) = run(r#"// The prototype changes between two key-adds of one construction: the second
// add must see the new chain (its setter), not the memo of the old one.
const Q = { set b(v: number) { hits.push(v); } };
const hits: number[] = [];
function F(this: any, v: number, swap: boolean) { this.a = v; if (swap) Object.setPrototypeOf(this, Q); this.b = v; }
let s = 0;
for (let i = 0; i < 2000; i++) { const o = new (F as any)(i, false); s += o.b; }
const o: any = new (F as any)(7, true);
const p: any = new (F as any)(8, false);
console.log(s, hits.join(","), Object.keys(o).join(","), Object.keys(p).join(","), p.b);
"#);
    assert_eq!(stdout, r#"1999000 7 a a,b 8"#);
    assert!(hits + memo > 0, "the add memo never ran (hits={hits} memo={memo})");
}

/// Dictionary receivers (unmatchable shapes) reach a primed add site. Sabotage:
/// the hit skips the pre-shape compare -> the add lands in the wrong slot.
#[test]
fn dictionary_receivers_take_every_add() {
    let (stdout, hits, memo) = run(r#"// Dictionary receivers reach a primed key-add site: every add must land.
function add(o: any, v: number) { o.k = v; }
let s = 0;
for (let i = 0; i < 2000; i++) { const o: any = { a: 1, b: 2 }; add(o, i); s += o.k; }
const ds: any[] = [];
for (let i = 0; i < 50; i++) {
  const d: any = { a: 1, b: 2 };
  for (let j = 0; j < 40; j++) d["x" + j] = j;
  for (let j = 0; j < 40; j += 2) delete d["x" + j];
  delete d.a;
  add(d, i);
  ds.push(d);
}
let t = 0; for (const d of ds) t += d.k + d.x39;
const d0 = ds[0];
console.log(s, t, Object.keys(d0).slice(0, 3).join(","), Object.keys(d0).slice(-2).join(","), JSON.stringify(ds[1]).length);
"#);
    assert_eq!(stdout, r#"1999000 3175 b,x1,x3 x39,k 183"#);
    assert!(hits + memo > 0, "the add memo never ran (hits={hits} memo={memo})");
}

/// Every add's RHS allocates, so moving minors and fulls land mid-construction
/// and the hit must use the receiver re-read after the RHS. That re-read is
/// the existing-key store's (#11241, whose `test_gap_gc_store_ic_rhs_collects`
/// is red under its sabotage); this test holds the key-add path to it.
#[test]
fn moving_collections_between_adds_of_one_construction() {
    let (stdout, hits, memo) = run(r#"// Every key-add's RHS allocates, so collections (moving minors, and fulls)
// land between the adds of one construction.
function F(this: any, i: number) {
  this.a = [i, i + 1, i + 2];
  this.b = { v: i, w: new Array(64).fill(i) };
  this.c = "s" + i;
  this.d = new Array(200).fill(i).map((x: number) => ({ x }));
  this.e = i;
}
const keep: any[] = [];
let s = 0;
for (let i = 0; i < 20000; i++) {
  const o = new (F as any)(i);
  if (i % 50 === 0) keep.push(o);
  s += o.a[2] + o.b.w[63] + o.d[199].x + o.e + o.c.length;
}
let t = 0; for (const o of keep) t += o.a[0] + o.b.v + o.d[5].x + Number(o.c.slice(1)) + o.e;
console.log(s, t, Object.keys(keep[3]).join(","));
"#);
    assert_eq!(stdout, r#"800108890 19950000 a,b,c,d,e"#);
    assert!(hits + memo > 0, "the add memo never ran (hits={hits} memo={memo})");
}

/// Old receivers get young values through a key-add. Sabotage: the runtime
/// memo store skips the barrier -> the young values are collected.
#[test]
fn an_old_receiver_keeps_a_young_value_added_to_it() {
    let (stdout, hits, memo) = run(r#"// Old objects receive young values through a key-add: the young values must
// survive later collections (the store must be remembered).
const olds: any[] = [];
for (let i = 0; i < 3000; i++) olds.push({ id: i });
for (let r = 0; r < 30; r++) { const junk: any[] = []; for (let i = 0; i < 20000; i++) junk.push({ r, i }); }
function add(o: any, v: any) { o.young = v; }
for (let i = 0; i < 3000; i++) add(olds[i], { v: i, arr: [i, i] });
for (let r = 0; r < 30; r++) { const junk: any[] = []; for (let i = 0; i < 20000; i++) junk.push({ r, i, s: "x" + i }); }
let s = 0; for (const o of olds) s += o.young.v + o.young.arr[1];
console.log(s, Object.keys(olds[7]).join(","));
"#);
    assert_eq!(stdout, r#"8997000 id,young"#);
    assert!(hits + memo > 0, "the add memo never ran (hits={hits} memo={memo})");
}

/// Object.keys / JSON / for-in / entries after inline adds mixed with
/// overwrites. Sabotage: the memo publishes slot n+1 -> values move.
#[test]
fn key_order_after_inline_adds_matches_insertion_order() {
    let (stdout, hits, memo) = run(r#"// Key order after inline key-adds, mixed with overwrites: Object.keys, JSON,
// for-in and entries must all list keys in insertion order.
function P(this: any, i: number) { this.z = i; this.a = i; this.m = i; this.z = i + 1; this.b = String(i); this.a = -i; }
let s = "";
for (let i = 0; i < 3000; i++) { const p = new (P as any)(i); if (i % 1000 === 0) s += JSON.stringify(p) + ";"; }
const p: any = new (P as any)(5);
const fi: string[] = []; for (const k in p) fi.push(k);
console.log(s, Object.keys(p).join(","), fi.join(","), JSON.stringify(Object.entries(p)));
"#);
    assert_eq!(stdout, r#"{"z":1,"a":0,"m":0,"b":"0"};{"z":1001,"a":-1000,"m":1000,"b":"1000"};{"z":2001,"a":-2000,"m":2000,"b":"2000"}; z,a,m,b z,a,m,b [["z",6],["a",-5],["m",5],["b","5"]]"#);
    assert!(hits > 0, "the inline add hit never ran (hits={hits} memo={memo})");
}
