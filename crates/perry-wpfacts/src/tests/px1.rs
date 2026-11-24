//! PROXY1 (2026-11-16): `new Proxy( t, h )` with `h` a program object is a
//! fresh Proxy exotic object (ECMA-262 10.5, ProxyCreate 10.5.14): each
//! internal method reads its trap from `h` by an ordinary [[Get]]
//! (GetMethod 7.3.11), calls it with `this = h` and the spec arguments, or
//! forwards to `t` when the trap is undefined or null. Nothing escapes to
//! unknown code. RED: the get / set / has shape (fixture fxP1_tokens).
use super::*;

/// Solve one module; the unknown values' reasons (incl. escapes) and
/// whether the realm is held.
fn solve(src: &str) -> (Vec<String>, bool) {
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "main.ts").unwrap(),
    ));
    let hir = perry_hir::lower_module(ast, "main", "main.ts").unwrap();
    let hirs = vec![("main.ts", &*Box::leak(Box::new(hir)))];
    let mut g = crate::gen::Gen::new(hirs, false, true);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    let mut tops: Vec<String> = g.m.top_vals.keys().cloned().collect();
    tops.extend(
        g.m.escaped
            .values()
            .map(|&r| g.m.reasons[r as usize].clone()),
    );
    (tops, g.m.realm_held)
}

fn e9(tops: &[String]) -> bool {
    tops.iter().any(|t| t.contains("E9"))
}

const TOKENS: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { item: (i: number): string => "i" + i, length: 2 };
  const store: Record<string, string> = {};
  return new Proxy(api, {
    get(t: any, key: any) { if (typeof key !== "string") return t[key]; if (key in api) return api[key]; return store[key] ?? ""; },
    set(_t: any, key: any, v: any) { if (typeof key !== "string") return false; store[key] = String(v); return true; },
    has(_t: any, key: any) { return typeof key === "string" && (key in api || key in store); },
  });
}
const p = mk();
p.color = "red" + N;
console.log(p.color, p.length, p.item(1), "color" in p, "nope" in p, p.missing === "");
"#;

// R1 (RED on base: "E9 Proxy handler" escapes, the realm is held).
#[test]
fn px1_r1_tokens_shape_is_modeled() {
    let (tops, held) = solve(TOKENS);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// K1: a trap added to `h` after the `new` is read by the same [[Get]] at
// the operation (the field's values hold the later store).
#[test]
fn px1_k1_late_trap() {
    let (tops, held) = solve(
        r#"const h: any = {};
const p: any = new Proxy({ a: 1 } as any, h);
console.log(p.a);
h.get = (_t: any, k: any) => "late" + String(k) + process.argv[2];
console.log(p.a);
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// K2: a handler chosen by argv between two program objects: both traps.
#[test]
fn px1_k2_argv_handler() {
    let (tops, held) = solve(
        r#"const h1: any = { get: (_t: any, k: any) => "one:" + String(k) };
const h2: any = {};
const h = String(process.argv[2]) === "3" ? h1 : h2;
const p: any = new Proxy({ a: 1 } as any, h);
console.log(p.a, p.b);
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// K3: the Proxy of Proxy.revocable is a modeled Proxy (rule PXESC: its
// escape runs its traps; after revoke() each operation throws a TypeError
// first, which the traps over-run). No part is held (E9).
#[test]
fn px1_k3_revocable_is_a_modeled_proxy() {
    let (tops, _) = solve(
        r#"const { proxy, revoke } = Proxy.revocable({ a: 1 } as any, {});
console.log((proxy as any).a, process.argv[2]);
revoke();
"#,
    );
    assert!(!e9(&tops), "revocable holds E9: {tops:?}");
}

/// PROXY3: the verdicts of the sites under `key` (reads, calls, stores).
fn verdicts(src: &str, key: &str) -> Vec<String> {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let pf = crate::compute(&[(path, &hir)], &FactsConfig::default());
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

// R1 sites: the read, the call and the store on the proxy made inside `mk`
// get the trap verdict (owner 2026-11-16: trap sites count as proven).
#[test]
fn px3_r1_sites_are_trap_verdicts() {
    for key in ["item", "length", "color"] {
        let v = verdicts(TOKENS, key);
        assert!(
            !v.is_empty() && v.iter().all(|v| v == "ProxyTrap"),
            "{key}: {v:?}"
        );
    }
}

// K4: a trap that returns `this` (the handler, 10.5.8 step 7: Call( trap,
// handler, ... )): the read's value is the handler object.
#[test]
fn px3_k4_trap_returns_this() {
    let (tops, held) = solve(
        r#"function mk(): any {
  const h: any = { tag: "h", get(_t: any, _k: any) { return this; } };
  return new Proxy({ a: 1 } as any, h);
}
const p = mk();
console.log(p.a.tag, p.b.tag + String(process.argv[2]));
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// K5: a `set` trap that writes to a second object: the store reaches it.
#[test]
fn px3_k5_set_trap_writes_elsewhere() {
    let (tops, held) = solve(
        r#"function mk(side: any): any {
  return new Proxy({} as any, { set(_t: any, k: any, v: any) { side[k] = v; return true; } });
}
const side: any = {};
const p = mk(side);
p.color = "red" + String(process.argv[2]);
console.log(side.color);
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// K6: `in` with a `has` trap; K7: delete with a `deleteProperty` trap.
#[test]
fn px3_k6_k7_has_and_delete() {
    let (tops, held) = solve(
        r#"function mk(): any {
  const o: any = { a: 1, b: 2 };
  return new Proxy(o, {
    has(t: any, k: any) { return k === "z" || k in t; },
    deleteProperty(t: any, k: any) { delete t[k]; return true; },
  });
}
const p = mk();
const k = String(process.argv[2]);
console.log("a" in p, "z" in p, k in p);
delete p.a;
console.log("a" in p);
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// K8: a target that is itself a proxy: the outer forward is the inner
// proxy's operation.
#[test]
fn px3_k8_proxy_of_proxy() {
    let src = r#"function mk(): any {
  const inner: any = new Proxy({ a: 1 } as any, { get(t: any, k: any) { return k === "b" ? 2 : t[k]; } });
  return new Proxy(inner, { get(t: any, k: any) { return k === "c" ? 3 : t[k]; } });
}
const outer = mk();
console.log(outer.a, outer.b, outer.c, process.argv[2]);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops) && !held, "{tops:?}");
    let v = verdicts(src, "a");
    assert!(!v.is_empty() && v.iter().all(|v| v == "ProxyTrap"), "{v:?}");
}

// K9 (rule PXAPPLY): a call of the proxy is its [[Call]] (10.5.12): over a
// non-callable target no trap runs and the read keeps its trap verdict;
// over a function the `apply` trap runs. No E9 either way.
#[test]
fn px3_k9_call_of_the_proxy_is_exact() {
    let src = r#"const p: any = new Proxy({ a: 1 } as any, { get: (_t: any, k: any) => "g:" + String(k) });
try { p(); } catch (x) {}
console.log(p.a, process.argv[2]);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops) && !held, "{tops:?}");
    let (tops, _) = solve(
        r#"const f = function (a: number): number { return a + 1; };
const p: any = new Proxy(f, { apply(t: any, _th: any, args: any[]) { return t(...args) * 10; } });
console.log(p(Number(process.argv[2])));
"#,
    );
    assert!(!e9(&tops), "{tops:?}");
}

// PROXY4 R2 (RED on 115fc8fab3: the `=== null` test of a field that holds
// the Proxy is a `Guard` the hook did not model, so the Proxy degraded to
// "E9 Proxy target"). A lazily made Proxy in a field, as a DOM `style`.
const LAZY: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { item: (i: number): string => "i" + i };
  const store: Record<string, string> = {};
  return new Proxy(api, {
    get(t: any, key: any) { if (typeof key !== "string") return t[key]; if (key in api) return api[key]; return store[key] ?? ""; },
    set(_t: any, key: any, v: any) { if (typeof key !== "string") return false; store[key] = String(v); return true; },
  });
}
class El {
  _s: any = null;
  get style(): any { if (this._s === null) this._s = mk(); return this._s; }
}
const e = new El();
e.style.color = "red" + N;
console.log(e.style.color, e.style.item(1), e.style === e.style);
"#;

#[test]
fn px4_r2_null_test_of_a_proxy_field() {
    let (tops, held) = solve(LAZY);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
    let v = verdicts(LAZY, "color");
    // PROXY9 (owner 2026-11-17): the field holds the Proxy or `null`: the
    // trap verdict, `null` its TypeError arm.
    assert!(
        !v.is_empty() && v.iter().all(|s| s == "ProxyTrap"),
        "color sites are trap sites: {v:?}"
    );
}

// The other value tests: typeof, `??`, `&&`, `!`, `!== null`.
#[test]
fn px4_k10_value_tests_run_no_trap() {
    let src = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { n: 1 };
  return new Proxy(api, { get(t: any, key: any) { return key === "n" ? N : t[key]; } });
}
const p: any = mk();
const q: any = (N === "x" ? null : p) ?? p;
const r: any = q && q;
console.log(typeof r, !r, r !== null, r === undefined, r.n);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PROXY5 R3: ToPrimitive of a Proxy (String, `+`, a template; 7.1.1) with
// a get trap and without one: `@@toPrimitive`, `valueOf`, `toString` are
// [[Get]]s of the Proxy, the callable one runs with `this` = the Proxy.
const TO_PRIM: &str = r#"const N = String(process.argv[2]);
function mk(g: boolean): any {
  const t = { n: 1, toString(): string { return "T" + this.n; } };
  const h: any = g ? { get(o: any, k: any) { return k === "n" ? N : o[k]; } } : {};
  return new Proxy(t, h);
}
const a = mk(true), b = mk(false);
console.log(String(a), "" + a, `${a}`, String(b), "" + b, `${b}`);
"#;

#[test]
fn px5_r3_to_primitive_of_a_proxy() {
    let (tops, held) = solve(TO_PRIM);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
    let v = verdicts(TO_PRIM, "n");
    assert!(
        !v.is_empty() && v.iter().all(|s| s.contains("Proxy")),
        "`this.n` in toString sees the Proxy: {v:?}"
    );
}

// K11: a get trap that returns a non-callable `toString`: OrdinaryToPrimitive
// skips it, `valueOf` returns the Proxy (an object): TypeError (as node).
#[test]
fn px5_k11_non_callable_to_string() {
    let src = r#"const N = String(process.argv[2]);
const p: any = new Proxy({ n: N }, { get(o: any, k: any) { return k === "toString" ? 5 : o[k]; } });
try { console.log(String(p)); } catch (e) { console.log(e instanceof TypeError, p.n); }
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// K12: a Proxy over a function: ToPrimitive reads Function.prototype's
// `toString` through the forward and calls it with `this` = the Proxy.
// That builtin is opaque to the solver: it receives the Proxy (an escape:
// the old path), as it receives a plain function.
#[test]
fn px5_k12_proxy_of_a_function() {
    let src = r#"const N = String(process.argv[2]);
const f: any = new Proxy(function (): string { return N; }, {});
console.log(String(f).startsWith("function"), typeof f);
"#;
    let (tops, held) = solve(src);
    let plain = src.replace(
        "new Proxy(function (): string { return N; }, {})",
        "function (): string { return N; }",
    );
    let (ptops, pheld) = solve(&plain);
    eprintln!("px5 k12 proxy held={held} plain held={pheld} plain={ptops:?}");
    assert!(
        tops.iter()
            .filter(|t| t.contains("E9"))
            .all(|t| t == "E9 Proxy target (escape)" || t == "E9 Proxy handler"),
        "only the opaque builtin's escape: {tops:?}"
    );
}

// PROXY7 R4: the runtime's formatter showing a Proxy (a console argument, or
// an object's field it shows) unwraps it first: no trap runs (node
// `getProxyDetails`), the target's own values are shown. The shape of a DOM
// element whose lazy `style` field holds a Proxy, logged.
const SHOWN: &str = r#"const N = String(process.argv[2]);
class El {
  _style: any = null;
  get style(): any {
    if (this._style === null) this._style = new Proxy({ color: N } as any, { get(o: any, k: any) { return k === "color" ? "c" + o.color : o[k]; } });
    return this._style;
  }
}
const e = new El();
console.log(e.style.color);
console.log(e, e.style);
"#;

#[test]
fn px7_r4_the_formatter_shows_a_proxy() {
    let (tops, held) = solve(SHOWN);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// R5: the `[util.inspect.custom]` hook is read off the target and called
// with `this` = the Proxy: `this.a` in it runs the get trap.
const HOOK: &str = r#"const N = String(process.argv[2]);
const C = Symbol.for("nodejs.util.inspect.custom");
const t: any = { a: N, [C](this: any): string { return "custom a=" + this.a; } };
const p: any = new Proxy(t, { get(o: any, k: any) { return k === "a" ? "x" + o.a : o[k]; } });
console.log(p);
"#;

#[test]
fn px7_r5_inspect_hook_this_is_the_proxy() {
    let (tops, held) = solve(HOOK);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
    let v = verdicts(HOOK, "a");
    assert!(
        v.iter().any(|s| s.contains("Proxy")),
        "`this.a` in the hook sees the Proxy: {v:?}"
    );
}

// K13: a get trap that throws: the formatter runs no trap (node prints the
// target), so the log does not throw.
#[test]
fn px7_k13_throwing_get_trap_is_not_run() {
    let src = r#"const N = String(process.argv[2]);
const p: any = new Proxy({ a: N }, { get(): any { throw new Error("boom"); } });
console.log(p);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// K14: a Proxy over an array: its elements are shown.
#[test]
fn px7_k14_proxy_over_an_array() {
    let src = r#"const N = String(process.argv[2]);
const p: any = new Proxy([N, N + "1"], {});
console.log(p, p.length);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

const PX9_MK: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { item: (i: number): string => "i" + i };
  return new Proxy(api, { get(t: any, k: any) { return k === "color" ? "c" + N : t[k]; } });
}
"#;

// PROXY9 R6: a Proxy or `null` / `undefined`: reads, calls and stores are
// trap sites (the nullish arm throws the TypeError).
#[test]
fn px9_r6_proxy_or_nullish_is_a_trap_site() {
    let src = format!(
        "{PX9_MK}const p: any = N === \"x\" ? null : N === \"y\" ? undefined : mk();
try {{ p.color = N; console.log(p.color, p.item(1)); }} catch (e) {{ console.log((e as Error).message); }}
"
    );
    for key in ["color", "item"] {
        let v = verdicts(&src, key);
        assert!(
            !v.is_empty() && v.iter().all(|s| s == "ProxyTrap"),
            "{key}: {v:?}"
        );
    }
}

// PROXY9 K15: a Proxy or an ordinary object stays mixed; a Proxy or a
// string stays undecided by the trap rule.
#[test]
fn px9_k15_proxy_beside_an_object_stays_mixed() {
    let src = format!(
        "{PX9_MK}const o: any = N === \"x\" ? {{ color: \"o\" }} : mk();
console.log(o.color);
o.color = N;
const q: any = N === \"y\" ? \"s\" : mk();
console.log(q.item);
"
    );
    let v = verdicts(&src, "color");
    assert!(
        !v.is_empty() && v.iter().all(|s| s != "ProxyTrap"),
        "color: {v:?}"
    );
    let v = verdicts(&src, "item");
    assert!(
        !v.is_empty() && v.iter().all(|s| s != "ProxyTrap"),
        "item: {v:?}"
    );
}

// PROXY10 (RED on 7a7ed4e1bd: a Proxy as the KEY of a computed store degraded
// to "E9 Proxy target (storeindexed)"). ToPropertyKey is ToPrimitive: the get
// trap runs for `@@toPrimitive` and `toString`; the store uses the string.
const KEY: &str = r#"const N = String(process.argv[2]);
function mk(tag: string): any {
  const api: Record<string, unknown> = { tag };
  return new Proxy(api, {
    get(t: any, key: any) { if (key === "toString") return () => "key" + String(t.tag); return t[key]; },
  });
}
const p = mk("a" + N);
const o: Record<string, string> = {};
o[p] = "v" + N;
const g: any = globalThis;
g[p] = 7;
console.log(Object.keys(o).join(","), o["keya" + N], g["keya" + N]);
"#;

#[test]
fn px10_proxy_as_a_computed_store_key() {
    let (tops, held) = solve(KEY);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// Rule PXCALL (was a KEEP of the old path, RED on PXCASE): a call of the
// store-key Proxy, whose target (an object literal) is not callable: a
// TypeError, no trap runs (10.5.14 step 7, 7.3.14 Call step 2).
#[test]
fn px10_called_proxy_key_runs_no_trap() {
    let src = KEY.replace("g[p] = 7;", "g[p] = 7;\ntry { p(); } catch (e) {}");
    let (tops, held) = solve(&src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PXSO1 R1: a Proxy whose target may be itself (one birth site in a loop):
// the forward of each operation reaches the Proxy again. The operation
// class is made once per Proxy (`ProxyOp`), so the forward joins it
// instead of making a new trap call and forward without end (a stack
// overflow on base).
#[test]
fn pxso1_r1_proxy_of_itself() {
    let (tops, held) = solve(
        r#"const h: any = { get(t: any, k: any) { return k === "z" ? 26 : t[k]; } };
let x: any = { a: 1, b: "two" };
for (let i = 0; i < 3; i++) x = new Proxy(x, h);
console.log(x.a, x.b, x.z, "a" in x, process.argv[2]);
x.c = 3;
console.log(x.c, delete x.c, x.c);
"#,
    );
    assert!(!e9(&tops) && !held, "{tops:?}");
}

// PXSO1 R2: the reads of a Proxy of itself are solved: `x` holds the
// object and the Proxy (the mixed verdict), nothing escapes.
#[test]
fn pxso1_r2_get_of_proxy_of_itself() {
    let src = r#"const h: any = { get(t: any, k: any) { return k === "z" ? 26 : t[k]; } };
let x: any = { a: 1 };
for (let i = 0; i < 3; i++) x = new Proxy(x, h);
console.log(x.a, x.z, process.argv[2]);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops) && !held, "{tops:?}");
    let v = verdicts(src, "a");
    assert!(
        !v.is_empty() && v.iter().all(|v| !v.contains("E9") && !v.contains("escape")),
        "{v:?}"
    );
}

// PROXY11 (RED on 16c4381812: a Proxy serialized by JSON, or held by a
// console argument the formatter's `%j` serializes, degraded to "E9 Proxy
// target (implicit)"; the cube program's first ⊤, bloom's CSSStyleDeclaration
// Proxy in an Element's `_style`). SerializeJSONProperty runs the Proxy's
// internal methods: the `toJSON` [[Get]], [[OwnPropertyKeys]],
// [[GetOwnProperty]] and the member [[Get]]s, each its trap or the target's.
const JSONP: &str = r#"const N = String(process.argv[2]);
function style(host: { tag: string }): any {
  const api: Record<string, unknown> = { cssText: "a" + N, item: (i: number): string => "i" + i };
  return new Proxy(api, {
    get(t: any, key: any) { if (typeof key !== "string") return t[key]; if (key in api) return t[key]; return host.tag + key; },
    set(t: any, key: any, v: any) { t[key] = String(v); return true; },
    has(_t: any, key: any) { return typeof key === "string" && key in api; },
  });
}
class El { tag = "div" + N; _style: any = null; get style(): any { if (this._style === null) this._style = style(this); return this._style; } }
const e = new El();
e.style.color = "red";
console.log(e);
console.log({ s: e.style, n: N });
console.log(JSON.stringify(e.style), e.tag);
"#;

#[test]
fn proxy11_json_of_a_proxy_runs_its_internal_methods() {
    let (tops, held) = solve(JSONP);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// A Proxy console argument itself: `%j`, ToPrimitive and the unwrapped show;
// with `ownKeys` / `getOwnPropertyDescriptor` traps.
#[test]
fn proxy11_console_argument_proxy_with_key_traps() {
    let src = r#"const N = String(process.argv[2]);
const api: Record<string, unknown> = { a: "x" + N };
const p: any = new Proxy(api, {
  ownKeys(t: any) { return Reflect.ownKeys(t); },
  getOwnPropertyDescriptor(t: any, k: any) { return { value: t[k], enumerable: true, configurable: true, writable: true }; },
  get(t: any, k: any) { return k === "toJSON" ? undefined : t[k]; },
});
console.log(p);
console.log(JSON.stringify([p, { q: p }]), N);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// Rule PXCALL (was a KEEP of the old path, RED on PXCASE): a call of the
// style Proxy over an object literal throws a TypeError and runs no trap.
#[test]
fn proxy11_called_proxy_runs_no_trap() {
    let src = JSONP.replace(
        "console.log(e);",
        "console.log(e);\ntry { (e.style as any)(); } catch (x) {}",
    );
    let (tops, held) = solve(&src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PROXY12 (RED on 8742219af8: "E9 Proxy target (fieldtruthy)", the cube
// program's next op on bloom's CSSStyleDeclaration Proxy). A truthiness or
// Boolean test of a Proxy's field reads it by [[Get]] (the get trap or the
// target) and runs no other trap.
const TRUTHY: &str = r#"const N = String(process.argv[2]);
function style(host: { tag: string }): any {
  const api: Record<string, unknown> = { cssText: "a" + N, flag: N === "x" };
  return new Proxy(api, {
    get(t: any, key: any) { if (typeof key !== "string") return t[key]; if (key in api) return t[key]; return host.tag + key; },
    set(t: any, key: any, v: any) { t[key] = String(v); return true; },
  });
}
class El { tag = "div" + N; _style: any = null; get style(): any { if (this._style === null) this._style = style(this); return this._style; } }
const e = new El();
const s: any = N === "y" ? null : e.style;
if (s && s.color) console.log(s.color);
if (s && s.flag === true) console.log(s.cssText);
if (s && !s.missing) console.log(s.tag);
"#;

#[test]
fn proxy12_field_truthy_of_a_proxy() {
    let (tops, held) = solve(TRUTHY);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// Rule PXCALL (was a KEEP of the old path, RED on PXCASE): the same program
// with a call of the Proxy (an object-literal target): no trap runs.
#[test]
fn proxy12_called_proxy_runs_no_trap() {
    let src = TRUTHY.replace(
        "const e = new El();",
        "const e = new El();\ntry { (e.style as any)(); } catch (x) {}",
    );
    let (tops, held) = solve(&src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PROXY12 R-PXKEYS (RED on the FieldTruthy head: "E9 Proxy target (implicit
// OwnKeys: primitive container builtin)"): for-in, Object.keys and
// Reflect.ownKeys of a Proxy run its ownKeys / getOwnPropertyDescriptor /
// getPrototypeOf traps (or the target's own) and nothing else.
#[test]
fn proxy12_own_keys_of_a_proxy() {
    let src = r#"const N = String(process.argv[2]);
const api: Record<string, unknown> = { a: "x" + N, b: 2 };
const p: any = new Proxy(api, {
  ownKeys(t: any) { return Reflect.ownKeys(t); },
  getOwnPropertyDescriptor(t: any, k: any) { return { value: t[k], enumerable: true, configurable: true, writable: true }; },
  getPrototypeOf(_t: any) { return null; },
  get(t: any, k: any) { return t[k]; },
});
const q: any = new Proxy({ c: N }, {});
const out: string[] = [];
for (const k in p) out.push(k);
for (const k in q) out.push(k);
console.log(out.join(","), Object.keys(p).length, Object.getOwnPropertyNames(q).length, Reflect.ownKeys(p).length);
"#;
    let (tops, held) = solve(src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PERFNOW49 rule PXKEY: a Proxy used as a computed KEY (`arr[p]`, `rec[p]`)
// runs no internal method of the Proxy in the receiver's [[Get]]: its
// ToPropertyKey (7.1.19) is ToPrimitive (7.1.1), the key's own rule
// (`proxy_to_primitive`: the target's `toString` runs with `this` = the
// Proxy). The rules on the key node (`ElemUndef`, `IdxKey`) read the key's
// value only. RED on base: they degraded the Proxy (web tokens.ts
// `item: (i) => read()[i]`, "E9 Proxy target (elemundef)").
const PROXY_KEY: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const t = { n: 1, toString(): string { return "1" + this.n; } };
  return new Proxy(t, { get(o: any, k: any) { return o[k]; } });
}
const p = mk();
const arr: string[] = ["a" + N, "b"];
const rec: Record<string, string> = { a: N };
const item = (i: any): string => arr[i] ?? "";
const pick = (k: any): string => rec[k] ?? "";
console.log(item(p), item(1), pick(p), pick("a"));
"#;

#[test]
fn pxkey_proxy_as_computed_key() {
    let (tops, held) = solve(PROXY_KEY);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
    let v = verdicts(PROXY_KEY, "n");
    assert!(
        !v.is_empty() && v.iter().all(|s| s.contains("Proxy")),
        "ToPropertyKey runs `toString` with `this` = the Proxy: {v:?}"
    );
}

// PERFNOW50 rule PXCASE: a node holding a String and a Proxy is the receiver
// of `toLowerCase` (web tokens.ts getPropertyValue `name.toLowerCase()` with
// `name` = the style Proxy). The case-map rule (K7) is the String receivers';
// for the Proxy the call is its get trap's result. RED on base: the rule
// degraded the Proxy ("E9 Proxy target (casemap)"). The receiver is the
// `string` parameter of an object literal's arrow, as the app's is
// (tokens.ts:154): with `any`, or a `string` parameter of a plain arrow,
// the rule is not reached (pn51 d3: no E9 there on 779be4fa25).
const PROXY_CASE: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const t = { n: 1 };
  return new Proxy(t, { get(o: any, k: any) { return k === "toLowerCase" ? (() => "px" + o.n) : o[k]; } });
}
const p = mk();
const api = { gv: (name: string): string => name.toLowerCase() };
console.log(api.gv("AB" + N), api.gv(p as any));
"#;

#[test]
fn pxcase_proxy_case_map_receiver() {
    let (tops, held) = solve(PROXY_CASE);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PERFNOW51 rule PXCALL: a Proxy has a [[Call]] / [[Construct]] only when its
// target is callable (ProxyCreate 10.5.14 step 7). Over an object literal, a
// plain call and `new` each throw a TypeError (7.3.14 Call step 2, 7.3.15
// Construct) and run no trap. RED on PXCASE: proxy_apply had no call arm.
const PROXY_CALL: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { n: N };
  const px: any = new Proxy(api, { get(t: any, k: any) { return k === "self" ? px : t[k]; } });
  return px;
}
const p = mk();
try { p(); } catch (e) { console.log(e instanceof TypeError); }
try { new p(); } catch (e) { console.log(e instanceof TypeError); }
console.log(p.n, typeof p);
"#;

#[test]
fn pxcall_non_callable_target_has_no_call() {
    let (tops, held) = solve(PROXY_CALL);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// Rule PXAPPLY: a callable target gives the Proxy a [[Call]] (the `apply`
// trap or the forward, 10.5.12), modeled exactly: no E9 hold, the realm is
// not held. Also when the target node holds an object literal first and a
// function stored after the call (one birth site in a loop: the rule is
// the target node's, re-made as its tokens grow), and through a Proxy of a
// Proxy (callable when its target is). The Proxies are made in functions:
// a call of a `new Proxy` binding is its own HIR form (`ProxyApply`).
#[test]
fn pxcall_callable_target_is_exact() {
    let wraps = "function wrap1(t: any): any { return new Proxy(t, {}); }\nfunction wrap2(t: any): any { return new Proxy(t, {}); }\n";
    let late = PROXY_CALL.replace(
        "const p = mk();",
        &format!("{wraps}const p = mk();\nconst box: any = {{ f: {{ a: 1 }} }};\nfor (let i = 0; i < 2; i++) {{ const q: any = wrap1(box.f); try {{ q(); }} catch (e) {{}} box.f = (): string => N; }}"),
    );
    let nested = PROXY_CALL.replace(
        "const p = mk();",
        &format!("{wraps}const p = mk();\nconst q: any = wrap1(wrap2(function (): string {{ return N; }}));\ntry {{ q(); }} catch (e) {{}}"),
    );
    for src in [late, nested] {
        let (tops, held) = solve(&src);
        assert!(!e9(&tops), "callable target, no E9: {tops:?}");
        assert!(!held, "callable target, realm not held: {tops:?}");
    }
    // A Proxy of a Proxy over an object literal: no [[Call]] either.
    let plain = PROXY_CALL.replace(
        "const p = mk();",
        &format!("{wraps}const p = mk();\nconst q: any = wrap1(wrap2({{ a: 1 }}));\ntry {{ q(); }} catch (e) {{}}"),
    );
    let (tops, held) = solve(&plain);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// PERFNOW55 rule PXCALLR: a method call that finds the Proxy through a
// receiver (`o.m()` whose `m` the get trap answers with the Proxy itself, web
// tokens.ts: `valueOf` returns `this`; a computed-key method call) binds it
// by relays: the `this` (ThisTo), the arguments and result (CallArgs) and the
// per-receiver builtin run (BuiltinCallee). Each is part of the Proxy's
// [[Call]]: over an object literal there is none (7.3.14 Call step 2, a
// TypeError) and no trap runs. RED on PXCALL without the relay arms: "E9
// Proxy target (thisto)", "(callargs)", "(builtincallee)".
const PROXY_CALLR: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const api: Record<string, unknown> = { n: N };
  const px: any = new Proxy(api, { get(t: any, k: any) { return k === "self" ? px : t[k]; } });
  return px;
}
const p = mk();
const o: any = { m: p.self };
try { o.m(N); } catch (e) { console.log(e instanceof TypeError); }
const r: Record<string, any> = { k: p };
try { r["k" + (N.length > 9 ? "x" : "")](1); } catch (e) { console.log(e instanceof TypeError); }
console.log(p.n);
"#;

#[test]
fn pxcallr_method_of_non_callable_proxy_has_no_call() {
    let (tops, held) = solve(PROXY_CALLR);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}

// Rule PXAPPLY: a Proxy over a function found as a method has a [[Call]]
// (10.5.12), modeled exactly as for a plain call: no E9 hold.
#[test]
fn pxcallr_callable_method_is_exact() {
    let src = PROXY_CALLR.replace(
        "console.log(p.n);",
        "function wrap1(t: any): any { return new Proxy(t, {}); }\nconst f: any = { m: wrap1((): string => N) };\nconsole.log(f.m(), p.n);",
    );
    let (tops, held) = solve(&src);
    assert!(!e9(&tops), "callable method, no E9: {tops:?}");
    assert!(!held, "callable method, realm not held: {tops:?}");
}

// PERFNOW53 rule PXVAL: the Array-only and value-only rules on a Proxy (c09:
// three.js setupRenderTarget, an index store whose receiver may be a Proxy).
// RED on 1ef1a09bb2: "E9 Proxy target (lenindex)" / "(thisinst)".
const PXVAL_PRE: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const t: any = { n: 1 };
  return new Proxy(t, { get(o: any, k: any) { return o[k]; } });
}
const p = mk();
"#;
const PXVAL: &[(&str, &str)] = &[
    ("lenindex", r#"const api = { put: (a: any, i: number) => { a[i] = 1; return a; } };
console.log(api.put([0, 0], N.length).length, api.put(p, 1).n);"#),
    ("thisinst", r#"const o = { f(this: any) { return this.n; } }; console.log(o.f.call(N), o.f.call(p));"#),
];

#[test]
fn pxval_value_only_rules() {
    let mut bad = Vec::new();
    for (name, body) in PXVAL {
        let (tops, held) = solve(&format!("{PXVAL_PRE}{body}\n"));
        let holds: Vec<&String> = tops.iter().filter(|t| t.contains("E9")).collect();
        if e9(&tops) || held {
            bad.push(format!("{name} held={held} e9={holds:?}"));
        }
    }
    assert!(bad.is_empty(), "no E9 unknown, realm not held: {bad:#?}");
}

// PERFNOW53 rule PXSTR: a Proxy as an operand of the String constant folds
// (indexOf, substring, replace's receiver and search value). RED on
// 1ef1a09bb2: "E9 Proxy target (strindex)" / "(strsubstring)" /
// "(strreplace)".
const PXSTR_PRE: &str = r#"const N = String(process.argv[2]);
function mk(): any {
  const t: any = { n: 1 };
  return new Proxy(t, { get(o: any, k: any) { return o[k]; } });
}
const p = mk();
"#;
const PXSTR: &[(&str, &str)] = &[
    ("strindex", r#"const api = { ix: (s: string): number => ("ab" + N).indexOf(s) }; console.log(api.ix(N), api.ix(p as any));"#),
    ("strreplace_search", r#"const api = { r: (s: string): string => ("ab" + N).replace(s, "x") }; console.log(api.r(N), api.r(p as any));"#),
    ("strreplace_recv", r#"const api = { r: (s: string): string => s.replace("a", "x") }; console.log(api.r(N), api.r(p as any));"#),
    ("strsubstring", r#"const api = { s: (k: number): string => ("abcdef" + N).substring(k) }; console.log(api.s(1), api.s(p as any));"#),
];

#[test]
fn pxstr_string_fold_operands() {
    let mut bad = Vec::new();
    for (name, body) in PXSTR {
        let (tops, held) = solve(&format!("{PXSTR_PRE}{body}\n"));
        let holds: Vec<&String> = tops.iter().filter(|t| t.contains("E9")).collect();
        if e9(&tops) || held {
            bad.push(format!("{name} held={held} e9={holds:?}"));
        }
    }
    assert!(bad.is_empty(), "no E9 unknown, realm not held: {bad:#?}");
}

// PERFNOW64: the Proxy as the method a builtin receiver's member read finds
// (c08-B1 second chain, pn63 DIAG52N: `kind=thisto label="builtin-member-read"
// c=ThisTo{origin}`: web Canvas2D::solidRect `data.copyWithin(...)`, `data`
// a builtin whose any-key copy stored the style Proxy). The member is a
// program value on a builtin (`builtin-member-read`, lookup.rs), bound by the
// relays of PXCALLR: over an object literal the Proxy has no [[Call]] (7.3.14
// Call step 2, a TypeError) and no trap runs. RED on 1ef1a09bb2 (before
// PXCALLR): "E9 Proxy target (thisto)".
#[test]
fn pxthis_builtin_member_read_of_proxy_has_no_call() {
    let src = PROXY_CALLR.replace(
        "console.log(p.n);",
        "const m: any = Math;\nm.zz = p.self;\ntry { m.zz(N); } catch (e) { console.log(e instanceof TypeError); }\nconsole.log(p.n);",
    );
    let (tops, held) = solve(&src);
    assert!(!e9(&tops), "no E9 unknown: {tops:?}");
    assert!(!held, "realm not held: {tops:?}");
}
