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

// K3: Proxy.revocable keeps the old path (E9).
#[test]
fn px1_k3_revocable_keeps_old_path() {
    let (tops, _) = solve(
        r#"const { proxy, revoke } = Proxy.revocable({ a: 1 } as any, {});
console.log((proxy as any).a, process.argv[2]);
revoke();
"#,
    );
    assert!(e9(&tops), "revocable stays E9: {tops:?}");
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

// K9: an operation the hook does not model (`Object.keys`: [[OwnPropertyKeys]];
// a call of the proxy: [[Call]]) keeps the old path: E9, and no trap verdict.
#[test]
fn px3_k9_unmodeled_operation_keeps_old_path() {
    let src = r#"const p: any = new Proxy({ a: 1 } as any, { get: (_t: any, k: any) => "g:" + String(k) });
console.log(p.a, Object.keys(p), process.argv[2]);
"#;
    let (tops, _) = solve(src);
    assert!(e9(&tops), "{tops:?}");
    let v = verdicts(src, "a");
    assert!(v.iter().all(|v| v != "ProxyTrap"), "{v:?}");
    let (tops, _) = solve(
        r#"const f = function (a: number): number { return a + 1; };
const p: any = new Proxy(f, { apply(t: any, _th: any, args: any[]) { return t(...args) * 10; } });
console.log(p(Number(process.argv[2])));
"#,
    );
    assert!(e9(&tops), "{tops:?}");
}
