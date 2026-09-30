//! The method-call site (`recv.m(args)` on the One Path): the emitted hit in
//! `perry-codegen/src/expr/method_site.rs` and its memo in
//! `perry-runtime/src/object/method_site.rs`.
//!
//! Each test calls a site hot, then changes the world in a way the memo must
//! notice, and compares the output with node's. A broken hit that falls
//! through to the miss is correct and only slow, so every test also asserts
//! from the site's counters (`PERRY_METHOD_SITE_STATS`) that entries were
//! primed and that the hot calls were served inline (few misses).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile and run `source`; return (stdout, own primes, inherited primes, misses).
fn run(source: &str) -> (String, u64, u64, u64) {
    let (stdout, count) = run_counted(source);
    (
        stdout,
        count("primes_own"),
        count("primes_inherited"),
        count("misses"),
    )
}

/// Compile and run `source`; return stdout and a reader of the site counters.
fn run_counted(source: &str) -> (String, impl Fn(&str) -> u64) {
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
        .env("PERRY_METHOD_SITE_STATS", "1")
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{stderr}",
        run.status
    );
    let count = move |name: &str| -> u64 {
        stderr
            .split_whitespace()
            .find_map(|w| w.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
            .unwrap_or(0)
    };
    (
        String::from_utf8_lossy(&run.stdout).trim().to_owned(),
        count,
    )
}

/// Sabotage: the emitted own hit skips the code-pointer compare -> the reassigned method is not called.
#[test]
fn an_own_method_reassigned_turned_into_a_getter_or_deleted_is_seen() {
    let (stdout, own, inherited, misses) = run(
        r#"// ONE own-slot site, called in one loop with a runtime trip count, while the
// method is reassigned (no shape change), shadowed by a getter, and deleted.
const N = process.argv.length > 99 ? 1 : 4000;
function mk(k: number): any { return { k, m(x: number) { return x + this.k; } }; }
const a = mk(1), b = mk(100);
const out: any[] = [];
let s = 0;
for (let i = 0; i < N; i++) {
  if (i === 1000) a.m = function (x: number) { return x * 2 + this.k; };
  if (i === 2000) Object.defineProperty(b, "m", { get() { return (x: number) => x - 1; }, configurable: true });
  if (i === 3000) delete a.m;
  const o = i & 1 ? a : b;
  let r: any;
  try { r = o.m(i); } catch (e) { r = -1; }
  s += r;
  if (i % 997 === 0 || i === 1001 || i === 2001 || i === 3001) out.push(r);
}
console.log(s, out.join(","));
"#,
    );
    assert_eq!(stdout, r#"8348000 100,998,2003,2094,4003,5983,-1,3987"#);
    assert!(
        own > 0 && misses < 3000,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// Sabotage: the inherited hit reuses a memoized closure instead of loading
/// the holder's slot -> a replacement still calls the old method.
#[test]
fn an_inherited_method_reassigned_by_any_store_spelling_is_seen() {
    let (stdout, own, inherited, misses) = run(
        r#"// ONE inherited site over Object.create / ES5 receivers; the prototype's
// method is reassigned mid-loop by every spelling of a store.
const N = process.argv.length > 99 ? 1 : 6000;
const proto: any = { m(x: number) { return x + this.k; } };
function P(this: any, k: number) { this.k = k; }
(P as any).prototype.m = function (x: number) { return x * 10 + this.k; };
const o1: any = Object.create(proto); o1.k = 1;
const o2: any = new (P as any)(2);
const key = ["m"][0];
const out: any[] = [];
let s = 0;
for (let i = 0; i < N; i++) {
  if (i === 1000) proto.m = function (x: number) { return x * 3 + this.k; };
  if (i === 2000) (P as any).prototype["m"] = function (x: number) { return -x + this.k; };
  if (i === 3000) Object.assign(proto, { m(x: number) { return x * 7 + this.k; } });
  if (i === 4000) Reflect.set((P as any).prototype, "m", function (this: any, x: number) { return x * 8 + this.k; });
  if (i === 5000) proto[key] = function (x: number) { return x * 9 + this.k; };
  const o = i & 1 ? o1 : o2;
  const r = o.m(i);
  s += r;
  if (i % 1000 < 2) out.push(r);
}
console.log(s, out.join(","));
"#,
    );
    assert_eq!(
        stdout,
        r#"105992000 2,2,10002,3004,-1998,6004,-2998,21008,32002,28008,40002,45010"#
    );
    assert!(
        inherited > 0 && misses < 50,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// Sabotage: the emitted inherited hit skips the holder-word compare -> a
/// redefined or deleted method is still called.
#[test]
fn define_property_and_delete_on_the_holder_invalidate_the_inherited_entry() {
    let (stdout, own, inherited, misses) = run(
        r#"// ONE inherited site; defineProperty (data, then accessor) and delete on the
// holder mid-loop, then an own property shadows it.
const N = process.argv.length > 99 ? 1 : 6000;
const base: any = { m(x: number) { return x + 1000; } };
const proto: any = Object.create(base);
proto.m = function (x: number) { return x + this.k; };
const o: any = Object.create(proto); o.k = 7;
const out: any[] = [];
let s = 0;
for (let i = 0; i < N; i++) {
  if (i === 1000) Object.defineProperty(proto, "m", { value: function (this: any, x: number) { return x * 2 + this.k; }, writable: true, configurable: true });
  if (i === 2000) Object.defineProperty(proto, "m", { get() { return (x: number) => x * 100; }, configurable: true });
  if (i === 3000) delete proto.m;
  if (i === 4000) delete base.m;
  if (i === 5000) o.m = (x: number) => x - 1;
  let r: any;
  try { r = o.m(i); } catch (e) { r = -1; }
  s += r;
  if (i % 1000 < 2) out.push(r);
}

console.log(s, out.join(","));
"#,
    );
    assert_eq!(
        stdout,
        r#"263459500 7,8,2007,2009,200000,200100,4000,4001,-1,-1,4999,5000"#
    );
    assert!(
        inherited > 0 && misses < 4100,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// Mutating an unrelated marked prototype must not invalidate the inherited
/// method entry: the receiver and its direct holder keep their shape words.
#[test]
fn an_unrelated_prototype_write_does_not_invalidate_the_method_site() {
    let (stdout, own, inherited, misses) =
        run(r#"const proto: any = { m(x: number) { return x + 1; } };
const o: any = Object.create(proto);
const unrelated: any = { y: 0 };
const child: any = Object.create(unrelated);
let sum = 0;
for (let i = 0; i < 6000; i++) {
  unrelated.y = i;
  sum += o.m(i);
}
console.log(sum, unrelated.y, child.y);
"#);
    assert_eq!(stdout, "18003000 5999 5999");
    assert!(
        inherited > 0 && misses < 20,
        "the inherited entry was invalidated by an unrelated store (own={own} inherited={inherited} misses={misses})"
    );
}

/// What the prime must refuse (rest, `arguments`, bound) and what the hit must keep (arity padding,
/// per-object captures, `this` after a throw, a GC-moved inherited closure). Sabotage: the own hit
/// skips the code-pointer compare -> another object's method runs.
#[test]
fn call_semantics_arity_rest_bound_captures_this_proxy_and_gc() {
    let (stdout, own, inherited, misses) = run(
        r#"// Arity, rest, arguments, bound values, factory captures, `this` after a
// throw, primitives, Proxy, and a GC-moved inherited closure.
function c1(o: any, x: number): any { return o.m(x); }
function c3(o: any): any { return o.m(1, 2, 3); }
const lit: any = { a(x: number, y: number) { return String(x) + ":" + String(y); } };
const out: any[] = [];
for (let i = 0; i < 1000; i++) c1({ m: lit.a }, i);
out.push(c1({ m: lit.a }, 1));
const r: any = { m(...xs: number[]) { return xs.length; } };
const g: any = { m() { return arguments.length; } };
let t = 0; for (let i = 0; i < 1000; i++) t += c3(i & 1 ? r : g);
out.push(t);
const bound: any = { k: 5, m: function (this: any, x: number) { return x + this.k; }.bind({ k: 40 }) };
for (let i = 0; i < 1000; i++) c1(bound, 1);
out.push(c1(bound, 1));
function mkCounter(start: number): any { let n = start; return { m(x: number) { n += x; return n; } }; }
const cs = [mkCounter(0), mkCounter(1000), mkCounter(-5)];
for (let i = 0; i < 3000; i++) c1(cs[i % 3], 1);
out.push(cs.map((c) => c.m(0)).join(","));
const thrower: any = { k: "t", m(x: number) { if (x > 2998) throw new Error("boom" + this.k); return x; } };
const plain: any = { k: "p", m(x: number) { return this.k; } };
try { for (let i = 0; i < 3000; i++) c1(thrower, i); } catch (e: any) { out.push(e.message); }
out.push(c1(plain, 0));
out.push((() => { try { return c1("abc" as any, 0); } catch (e) { return "TE"; } })());
const sm: any = { m(x: number) { return "str" + x; } };
out.push(["x", 1, sm].map((v: any) => { try { return typeof v.m; } catch (e) { return "err"; } }).join(","));
const px = new Proxy({ m(x: number) { return x + 1; } }, { get(tg: any, p: any) { return p === "m" ? (x: number) => x * 3 : tg[p]; } });
out.push(c1(px, 5));
const proto: any = { m(x: number) { return x + this.k; } };
const objs: any[] = [];
for (let i = 0; i < 200; i++) { const o = Object.create(proto); o.k = i; objs.push(o); }
let acc = 0;
for (let round = 0; round < 50; round++) { for (let i = 0; i < 200; i++) acc += c1(objs[i], round); const junk = []; for (let j = 0; j < 2000; j++) junk.push({ j, s: "x" + j }); }
out.push(acc);
console.log(out.join(" "));
"#,
    );
    assert_eq!(
        stdout,
        r#"1:undefined 3000 41 1000,2000,995 boomt p TE undefined,undefined,function 15 1240000"#
    );
    assert!(
        own > 0 && misses < 3100,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// A method key past the inline slots (tsc's wide namespaces): served from `meta.spill`, reassigned
/// and deleted mid-loop. Sabotage: no code-pointer compare -> red.
#[test]
fn a_method_in_the_spill_buffer_is_served_and_invalidated() {
    let (stdout, own, inherited, misses) = run(
        r#"// ONE site over wide objects whose method key lives past the inline slots
// (the spill buffer): factory-built namespaces, as tsc's are.
const N = process.argv.length > 99 ? 1 : 6000;
function mkNs(tag: number): any {
  const ns: any = {};
  for (let i = 0; i < 40; i++) ns["f" + i] = i;
  ns.k = tag;
  ns.m = function (x: number) { return x + this.k; };
  return ns;
}
const a = mkNs(1), b = mkNs(1000);
const out: any[] = [];
let s = 0;
for (let i = 0; i < N; i++) {
  if (i === 2000) a.m = function (x: number) { return x * 2 + this.k; };
  if (i === 3000) b.f3 = "grown";
  if (i === 4000) delete b.m;
  const o = i & 1 ? a : b;
  let r: any;
  try { r = o.m(i); } catch (e) { r = -1; }
  s += r;
  if (i % 1000 < 2) out.push(r);
}
console.log(s, out.join(","));
"#,
    );
    assert_eq!(
        stdout,
        r#"23000000 1000,2,2000,1002,3000,4003,4000,6003,-1,8003,-1,10003"#
    );
    assert!(
        own > 0 && misses < 1100,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// A method served by a site throws; `this` is read afterwards in the catching method, arrow, async
/// method and generator, and by the next call. The catch savepoint (#10564) restores the
/// implicit-`this` cell on every transport, so the site's plain save/restore needs no landing pad.
#[test]
fn this_is_intact_after_a_method_throws() {
    let (stdout, own, inherited, misses) = run(
        r#"// A method called through a site throws; `this` is read afterwards in the
// catching caller (method, async method, generator, arrow, class method) and
// by the next call. Also the class own-closure override route.
const N = process.argv.length > 99 ? 1 : 3000;
const out: any[] = [];
const bad: any = { tag: "bad", m(x: number) { if (x % 1000 === 999) throw new Error("t" + this.tag); return x; } };
const good: any = { tag: "good", m(x: number) { return this.tag; } };
function hot(o: any, x: number): any { return o.m(x); }
const caller: any = {
  tag: "caller",
  run() {
    let caught = 0;
    for (let i = 0; i < N; i++) { try { hot(bad, i); } catch (e) { caught++; if (this.tag !== "caller") out.push("LOST:" + this.tag); } }
    const arrow = () => this.tag;
    out.push(caught, this.tag, arrow(), hot(good, 1));
  },
  async arun() {
    try { for (let i = 0; i < N; i++) hot(bad, i); } catch (e: any) { out.push("async:" + this.tag + ":" + e.message); }
  },
  *gen() {
    try { for (let i = 0; i < N; i++) hot(bad, i); } catch (e) { yield "gen:" + this.tag; }
  },
};
caller.run();
out.push(caller.gen().next().value);
class K {
  tag = "K";
  f: any = function (this: any, x: number) { if (x === 5) throw new Error("k"); return this.tag; };
  go() {
    let r = "";
    for (let i = 0; i < 10; i++) { try { r = (this as any).f(i); } catch (e) { r = "caught:" + this.tag; } }
    return r + ":" + (this as any).f(0);
  }
}
out.push(new K().go());
function sloppyReader(this: any) { return typeof this === "object" && this !== null && this.tag ? this.tag : "global"; }
try { hot(bad, 999); } catch (e) {}
out.push(sloppyReader.call({ tag: "explicit" }));
caller.arun().then(() => console.log(out.join(" ")));
"#,
    );
    assert_eq!(
        stdout,
        r#"3 caller caller good gen:caller K:K explicit async:caller:tbad"#
    );
    assert!(
        own > 0 && misses < 50,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// Three shapes over two ways: every call misses and primes, with young receiver and arguments.
#[test]
fn a_collection_during_a_polymorphic_miss_keeps_every_pointer_live() {
    let (stdout, own, inherited, misses) = run(
        r#"// A polymorphic site (three shapes over two ways, so every call misses) whose
// arguments and receiver are young objects: a collection during the miss must
// not hand the method stale pointers.
const N = process.argv.length > 99 ? 1 : 2000;
function mk(i: number): any {
  if (i % 3 === 0) return { a: 1, m(p: any, q: any) { return p.x + q.y.z + this.a; } };
  if (i % 3 === 1) return { b: 2, a: 10, m(p: any, q: any) { return p.x * 2 + q.y.z + this.a; } };
  return { c: 3, d: 4, a: 100, m(p: any, q: any) { return p.x - q.y.z + this.a; } };
}
function site(o: any, p: any, q: any): number { return o.m(p, q); }
let s = 0;
for (let i = 0; i < N; i++) {
  const o = mk(i);
  s += site(o, { x: i, pad: "p" + i }, { y: { z: i & 7 } });
}
console.log(s);
"#,
    );
    assert_eq!(stdout, r#"2742275"#);
    assert!(
        own > 0 && misses < 2100,
        "the site was not served inline (own={own} inherited={inherited} misses={misses})"
    );
}

/// `F.m()`: a function object's own methods live in its own-property object, an inline slot
/// the keyed Function ShapeId pins, so the site serves them from a function-bag entry (bit 61)
/// — with the method reassigned (same shape, new body: the code-pointer compare misses), a key
/// added (new keyed shape) and deleted (FunctionDictionary: never memoized) mid-loop, results
/// stay node's. Sabotage: serve the bag entry without the code-pointer compare, or read the
/// receiver's inline slot instead of the bag's -> the output changes.
#[test]
fn function_object_receivers_are_served_from_their_property_object() {
    let (stdout, count) = run_counted(
        r#"// ONE site over function-object receivers (`F.m()`), with the method
// reassigned, a key added (new keyed shape), a key deleted (dictionary), and a
// namespace-style function carrying several methods.
const N = process.argv.length > 99 ? 1 : 6000;
function F() { return 0; }
(F as any).k = 5;
(F as any).m = function (this: any, x: number) { return x + this.k; };
const G: any = () => 0;
G.k = 50; G.m = function (this: any, x: number) { return x * 2 + this.k; }; G.other = 1;
const out: any[] = [];
let s = 0;
for (let i = 0; i < N; i++) {
  if (i === 1000) (F as any).m = function (this: any, x: number) { return x * 3 + this.k; };
  if (i === 2000) (F as any).extra = 1;
  if (i === 3000) G.m = (x: number) => -x;
  if (i === 4000) delete (F as any).extra;
  if (i === 5000) G.m = function (this: any, x: number) { return x + this.other; };
  const o: any = i & 1 ? F : G;
  let r: any;
  try { r = o.m(i); } catch (e) { r = -1; }
  s += r;
  if (i % 1000 < 2) out.push(r);
}
console.log(s, out.join(","));
"#,
    );
    assert_eq!(
        stdout,
        r#"29838000 50,6,2050,3008,4050,6008,-3000,9008,-4000,12008,5001,15008"#
    );
    let (own, inherited, function, misses) = (
        count("primes_own"),
        count("primes_inherited"),
        count("primes_function"),
        count("misses"),
    );
    assert!(
        own == 0 && inherited == 0 && function >= 2,
        "function receivers prime function-bag entries only (own={own} inherited={inherited} function={function})"
    );
    // F leaves its keyed shape at i=4000 (a delete: FunctionDictionary), so
    // its last 1000 calls miss by design; everything else is served inline.
    assert!(
        misses < 2000,
        "the hot calls must be served inline (function={function} misses={misses})"
    );
}

/// One shape, several bodies. `{ k, m }` objects whose `m` is one of two functions get one entry
/// per body (the own hit falls through to the next way on a body mismatch), so that site is served
/// inline; a site that sees four bodies for one shape (more than its two ways) stops priming after
/// a bounded number of evictions instead of re-priming on every miss.
#[test]
fn one_shape_with_several_bodies_gets_an_entry_per_body_and_stops_priming_past_its_ways() {
    let (stdout, own, inherited, misses) = run(r#"const N = process.argv.length > 99 ? 1 : 6000;
const fs: any[] = [
  function (this: any, x: number) { return x + this.k; },
  function (this: any, x: number) { return x * 2 + this.k; },
  function (this: any, x: number) { return x * 3 + this.k; },
  function (this: any, x: number) { return x * 4 + this.k; },
];
const objs: any[] = [];
for (let j = 0; j < 4; j++) objs.push({ k: j, m: fs[j] });
function site2(o: any, x: number): number { return o.m(x); }
function site4(o: any, x: number): number { return o.m(x); }
let s = 0;
let t = 0;
for (let i = 0; i < N; i++) {
  s += site2(objs[i & 1], i);
  t += site4(objs[i & 3], i);
}
console.log(s, t);
"#);
    assert_eq!(stdout, "27000000 45009000");
    // site2: two primes (one entry per body), then inline. site4: four bodies
    // over two ways keeps evicting until it latches after 16 evictions; the
    // two entries it latched with keep serving their half of the calls, so
    // misses stay near N/2. Without the per-body fall-through site2 would
    // re-prime on every call; without the latch site4 would prime ~N/2 times.
    assert!(
        inherited == 0 && own <= 24 && misses <= 3100,
        "site2 must be served inline and site4 must stop priming \
         (own={own} inherited={inherited} misses={misses})"
    );
}

/// Sabotage: let the static unroller clone a loop body holding a function
/// literal -> the 8 copies are 8 code pointers and the site primes past its
/// ways and latches megamorphic.
#[test]
fn a_method_literal_built_in_a_short_counted_loop_is_one_function() {
    let (stdout, own, inherited, misses) = run(r#"const N = process.argv.length > 99 ? 1 : 4000;
const objs: any[] = [];
function build() { for (let i = 0; i < 8; i++) objs.push({ a: i, b: 2, m() { return this.a; } }); }
build();
function run(n: number): number {
  let h = 0;
  for (let k = 0; k < n; k++) h += objs[k & 7].m();
  return h;
}
console.log(run(N), objs[0].m === objs[7].m);
"#);
    // One source literal is one function: node prints `false` for the
    // identity (each evaluation is a fresh function object) but every object
    // shares one body, so the site keeps one entry for all 8 receivers.
    assert_eq!(stdout, "14000 false");
    assert!(
        inherited == 0 && own <= 2 && misses <= 4,
        "8 objects from one method literal must share one site entry \
         (own={own} inherited={inherited} misses={misses})"
    );
}
