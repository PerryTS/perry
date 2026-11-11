//! A Map's values apart from its keys (`solve/map_vals.rs`): `get` of a
//! closed Map returns what `set` stored as a value, never a key.

fn dump_with(src: &str, cfg: &crate::FactsConfig) -> String {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute(&[("t.ts".to_string(), &hir)], cfg).dump_text()
}

fn dump(src: &str) -> String {
    dump_with(src, &crate::FactsConfig::default())
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
    for (name, extra) in [
        ("an escape", "(globalThis as any).leak = table;"),
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
        assert_eq!(closed(&d), [false], "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
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

/// Soundness: a Map born from an iterable holds what the iterable yields.
#[test]
fn a_map_born_from_entries_is_open() {
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
    assert_eq!(closed(&d), [false], "{d}");
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
