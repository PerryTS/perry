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


/// node: a for-of over a Map yields `[key, value]` arrays only (24.1.3.12
/// Map.prototype[@@iterator] is entries, 24.1.5.1): `e` is never the key
/// `k`, so `e.g` is the pair's (undefined), never `s` (the PEELK35 route:
/// GLTFLoader `for (const [key, value] of parser.associations)`). The map
/// is iterated as an untyped value, so the loop calls its `@@iterator`
/// (a typed Map loop lowers to the HIR entry views instead).
#[test]
fn iteration_yields_entry_pairs_not_keys() {
    let v = verdict(
        "const k = { g: s }; const m = new Map<any, any>(); m.set(k, 1); \
         const walk = (x: any) => { for (const e of x) { take(e.g); } }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: a for-of over a Set yields its elements (24.2.3.11): `e` is `k`, `take` gets `s`.
#[test]
fn set_iteration_yields_the_elements() {
    let v = verdict(
        "const k = { g: s }; const st = new Set<any>(); st.add(k); \
         const walk = (x: any) => { for (const e of x) { take(e.g); } }; walk(st);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: unknown code holding the map may set any value: the pair element
/// read out of an escaped map is unknown.
#[test]
fn iteration_of_an_escaped_map_reads_unknown_values() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(\"k\", { b: 2 }); \
         new Function(process.argv[2])(m); \
         const walk = (x: any) => { for (const [a, b] of x) { b.f(s); } }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}

// ---- main's fixtures (solve/map_vals.rs) ----
// A Map's values apart from its keys (`solve/map_vals.rs`): `get` of a
// closed Map returns what `set` stored as a value, never a key.

fn dump_with(src: &str, cfg: &crate::FactsConfig) -> String {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute(&[("t.ts".to_string(), &hir)], cfg).dump_text()
}

/// The probe lines are diagnostics.
fn dump(src: &str) -> String {
    let cfg = crate::FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    dump_with(src, &cfg)
}

/// `sink` (the module's first function) takes the `move` field of an entry
/// read from a string-keyed Map; `extra` runs once on the Map `table`.
fn program(extra: &str) -> String {
    format!(
        "function sink(m: number): number {{ return m & 7; }}\n\
         interface Entry {{ key: string; move: number }}\n\
         const table = new Map<string, Entry>();\n\
         function step(k: string, n: number): number {{\n\
           const next = table.get(k);\n\
           const move = next === undefined ? 0 : next.move;\n\
           table.set(k, {{ key: k, move: n & 1023 }});\n\
           return sink(move);\n\
         }}\n\
         {extra}\n\
         let s = 0;\n\
         for (let i = 0; i < 20; i++) s += step(\"k\" + String(i & 3), i * 5);\n\
         console.log(s);"
    )
}

fn sink_is_int(d: &str) -> bool {
    d.lines()
        .any(|l| l.starts_with("# signature module=0 func=0 params=(i32"))
}

fn closed(d: &str) -> Vec<bool> {
    d.lines()
        .filter(|l| l.starts_with("# map_values "))
        .map(|l| l.contains(" closed=1 "))
        .collect()
}

#[test]
fn a_get_of_a_closed_map_is_no_key() {
    let d = dump(&program(""));
    assert_eq!(closed(&d), [true], "{d}");
    assert!(sink_is_int(&d), "{d}");
}

/// Soundness: each of these may put something else among the values (or
/// the model cannot tell), so `get` returns the whole contents again.
#[test]
fn an_opened_map_returns_its_contents() {
    let mut failed = Vec::new();
    for (name, extra) in [
        (
            "spread arguments",
            "const kv: [string, Entry] = [\"z\", { key: \"z\", move: 1 }]; table.set(...kv);",
        ),
        (
            "a computed store",
            "(table as any)[String(s0())] = 1; function s0(): number { return 3; }",
        ),
        ("a copy into it", "Object.assign(table, { a: 1 });"),
    ] {
        let d = dump(&program(extra));
        if closed(&d) != [false] || sink_is_int(&d) {
            failed.push(format!("{name}\n{d}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Soundness: a value that is no entry reaches the read.
#[test]
fn a_stored_string_value_reaches_the_read() {
    let d = dump(&program(
        "(table as Map<string, any>).set(\"q\", \"text\");",
    ));
    assert_eq!(closed(&d), [true], "{d}");
    assert!(!sink_is_int(&d), "{d}");
}

/// Soundness: a Map born from an iterable holds what the iterable yields:
/// element "1" of each entry is a value (24.1.1.2), so the Map stays
/// closed and `get` reads "text".
#[test]
fn a_map_born_from_entries_holds_their_values() {
    let src = "function sink(m: number): number { return m & 7; }\n\
         const table = new Map<string, any>([[\"a\", \"text\"]]);\n\
         function step(k: string, n: number): number {\n\
           const v = table.get(k);\n\
           table.set(k, n & 1023);\n\
           return sink(v === undefined ? 0 : v);\n\
         }\n\
         let s = 0;\n\
         for (let i = 0; i < 20; i++) s += step(\"k\" + String(i & 3), i * 5);\n\
         console.log(s);";
    let d = dump(src);
    assert_eq!(closed(&d), [true], "{d}");
    assert!(!sink_is_int(&d), "{d}");
}

#[test]
fn the_sabotage_takes_the_key_for_the_value() {
    let src = "function sink(m: number): number { return m & 7; }\n\
         const table = new Map<number, string>();\n\
         function step(k: number): number {\n\
           const v: any = table.get(k);\n\
           table.set(k, \"v\" + String(k));\n\
           return sink(v === undefined ? 0 : v);\n\
         }\n\
         let s = 0;\n\
         for (let i = 0; i < 20; i++) s += step(i & 3);\n\
         console.log(s);";
    assert!(!sink_is_int(&dump(src)));
    let cfg = crate::FactsConfig {
        sabotage: Some("mapvals".into()),
        ..Default::default()
    };
    let d = dump_with(src, &cfg);
    assert!(sink_is_int(&d), "{d}");
}
