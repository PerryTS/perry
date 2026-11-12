//! A Map or WeakMap keeps each entry's key and value apart: `get` returns
//! the [[Value]] of the entry whose [[Key]] matches, never a key (ECMA-262
//! 24.1.3.6 Map.prototype.get, 24.3.3.3 WeakMap.prototype.get). The values
//! are a field of the map object (`MAP_VALUES`), written by `set` and the
//! constructor; unknown code that holds the map may set any value.

use super::*;

/// The map is a local of a function: a script's top-level binding is
/// reachable by any code the host runs. The sink is a method of an unknown
/// value (as in `console_output_arguments`), not of the global object: a call `globalThis.hostTake(x)` hands unknown code the
/// global object as `this`, which may then replace the global `Map` that
/// `new Map` reads (ECMA-262 13.3.5.1 step 2 evaluates the constructor
/// expression: a [[Get]] of the live global binding), so every `new Map`
/// would construct with an unknown function value.
fn verdict(tail: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        const unknown: any = (globalThis as any)[process.argv[2]];
        const take = (x: any) => {{ unknown.take(x); }};
        (function () {{ {tail} }})();
        console.log(g(s));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: `hostTake` gets `{ b: 2 }`, never the key `s`.
#[test]
fn get_returns_the_value_not_the_key() {
    let v = verdict("const m = new Map<any, any>(); m.set(s, { b: 2 }); take(m.get(s));");
    assert!(proven(&v), "{v:?}");
}

/// node: as for Map (24.3.3.3).
#[test]
fn weakmap_get_returns_the_value_not_the_key() {
    let v = verdict("const m = new WeakMap<any, any>(); m.set(s, { b: 2 }); take(m.get(s));");
    assert!(proven(&v), "{v:?}");
}

/// node: the copy of the value has only `b`; `c.f` is undefined and the
/// call throws before `hostTake` runs (the PEELK26 route: GLTFLoader
/// `{ ...parser.associations.get(node) }`).
#[test]
fn a_spread_of_the_value_copies_no_key_field() {
    let v = verdict(
        "const k = { f: take }; const m = new Map<any, any>(); m.set(k, { b: 2 }); \
         const c: any = { ...m.get(k) }; try { c.f(s); } catch (e) {}",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: `new Map([[k, v]])` holds `v` as the value of `k` (24.1.1.2
/// AddEntriesFromIterable reads element "1" as the value).
#[test]
fn the_constructor_fills_the_values_from_element_1() {
    let v = verdict("const m = new Map<any, any>([[s, { b: 2 }]]); take(m.get(s));");
    assert!(proven(&v), "{v:?}");
}

/// node: as above with an entry that is no array: AddEntriesFromIterable
/// reads Get(entry, "0") as the key and Get(entry, "1") as the value of any
/// object (24.1.1.2 steps 4.d-4.f), so `get` returns `{ b: 2 }` only. (The
/// array-literal entry above also reads `s` under "1" while an array
/// literal's elements share one field.)
#[test]
fn the_constructor_reads_element_1_of_any_entry_object() {
    let v = verdict("const m = new Map<any, any>([{ 0: s, 1: { b: 2 } } as any]); take(m.get(s));");
    assert!(proven(&v), "{v:?}");
}

/// node: the constructor's values are values: `get` returns `take`, which runs on `s`.
#[test]
fn a_constructor_value_read_out_still_runs() {
    let v = verdict("const m = new Map<any, any>([[\"k\", take]]); m.get(\"k\")(s);");
    assert!(!proven(&v), "{v:?}");
}

/// node: `Map.prototype.set.call` stores the value as `m.set` does.
#[test]
fn a_set_through_call_stores_the_value() {
    let v = verdict("const m = new Map<any, any>(); Map.prototype.set.call(m, \"k\", take); m.get(\"k\")(s);");
    assert!(!proven(&v), "{v:?}");
}

/// Control: code the program does not know holds the map and may set any
/// value under any key (`new Function(process.argv[2])`).
#[test]
fn an_escaped_map_gets_unknown_values() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(\"k\", { b: 2 }); \
         new Function(process.argv[2])(m); const x = m.get(\"k\"); x.f(s);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: `values()` iterates the values only (24.1.3.11).
#[test]
fn values_iterates_the_values_only() {
    let v = verdict("const m = new Map<any, any>(); m.set(s, 1); for (const x of m.values()) take(x);");
    assert!(proven(&v), "{v:?}");
}

/// node: forEach calls back with (value, key, map) (24.1.3.5): the first
/// argument is never a key.
#[test]
fn foreach_passes_the_value_first() {
    let v = verdict("const m = new Map<any, any>(); m.set(s, 1); m.forEach((x: any) => take(x));");
    assert!(proven(&v), "{v:?}");
}

/// node: the key argument of forEach is the key.
#[test]
fn foreach_passes_the_key_second() {
    let v = verdict("const m = new Map<any, any>(); m.set(s, 1); m.forEach((x: any, k: any) => take(k));");
    assert!(!proven(&v), "{v:?}");
}

