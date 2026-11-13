//! In-bounds index stores (`gen/in_bounds.rs`, `Cons::IndexKept`): a store
//! whose key the text bounds by a length guard leaves no hole in an Array
//! that never shrinks, so a for-of over it yields no `undefined`.

fn dump(src: &str) -> String {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    // The probe lines are diagnostics.
    let cfg = crate::FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text()
}

/// `sink` (the module's first function) takes the for-of element of the
/// Array `body` builds in `xs` from the pushed `src`.
fn program(body: &str) -> String {
    format!(
        "function sink(m: number): number {{ return m & 7; }}\n\
         function make(n: number): number[] {{\n\
           const src: number[] = [];\n\
           for (let k = 0; k < n; k++) src.push((k * 7) & 1023);\n\
           return src;\n\
         }}\n\
         function run(n: number): number {{\n\
           const xs = make(n).slice();\n\
           {body}\n\
           let s = 0;\n\
           for (const x of xs) s += sink(x);\n\
           return s;\n\
         }}\n\
         console.log(run(40));"
    )
}

fn sink_is_int(d: &str) -> bool {
    d.lines()
        .any(|l| l.starts_with("# signature module=0 func=0 params=(i32"))
}

fn kept(d: &str) -> (usize, usize) {
    let lines: Vec<&str> = d
        .lines()
        .filter(|l| l.starts_with("# inbounds_store "))
        .collect();
    (
        lines.iter().filter(|l| l.contains(" kept=1")).count(),
        lines.iter().filter(|l| l.contains(" kept=0")).count(),
    )
}

const SORT: &str = "for (let i = 1; i < xs.length; i++) {\n\
    const v = xs[i] ?? 0;\n\
    let j = i - 1;\n\
    while (j >= 0 && (xs[j] ?? 0) < v) { xs[j + 1] = xs[j] ?? 0; j--; }\n\
    xs[j + 1] = v;\n\
  }";

#[test]
fn an_unstored_copy_yields_numbers() {
    let d = dump(&program(""));
    assert!(sink_is_int(&d), "{d}");
}

#[test]
fn an_insertion_sort_keeps_the_copy_hole_free() {
    let d = dump(&program(SORT));
    assert_eq!(kept(&d), (2, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

#[test]
fn a_store_at_the_counter_is_in_bounds() {
    let d = dump(&program(
        "for (let i = 0; i < xs.length; i++) { xs[i] = i & 3; }",
    ));
    assert_eq!(kept(&d), (1, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

/// Soundness: each of these may store past the end (or the text does not
/// prove otherwise), so the element keeps `undefined`.
#[test]
fn an_unproven_store_keeps_undefined() {
    for (name, body) in [
        (
            "a key past the counter",
            "for (let i = 0; i < xs.length; i++) { xs[i + 1] = 1; }",
        ),
        (
            "a derived local that also rises",
            "for (let i = 1; i < xs.length; i++) { let j = i - 1; j += 2; xs[j] = 1; }",
        ),
        (
            "a constant added past the derivation",
            "for (let i = 1; i < xs.length; i++) { let j = i - 1; xs[j + 2] = 1; }",
        ),
        (
            "a counter written before the store",
            "for (let i = 0; i < xs.length; i++) { i += 3; xs[i] = 1; }",
        ),
        (
            "a guard on another array",
            "const ys = make(n + 5); for (let i = 0; i < ys.length; i++) { xs[i] = 1; }",
        ),
        ("no guard", "xs[n] = 1;"),
        (
            "a bound past the length",
            "for (let i = 0; i <= xs.length + 1; i++) { xs[i] = 1; }",
        ),
    ] {
        let d = dump(&program(body));
        assert_eq!(kept(&d).0, 0, "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// Soundness: the text proves the key, but the Array may shrink between
/// the guard and the store (here or through another name), so the store
/// may land past its end.
#[test]
fn a_shrinking_array_keeps_undefined() {
    for (name, body) in [
        (
            "a pop in the loop",
            "for (let i = 0; i < xs.length; i++) { xs.pop(); xs[i] = 1; }",
        ),
        (
            "a length store",
            "for (let i = 0; i < xs.length; i++) { xs[i] = 1; } xs.length = 2;",
        ),
        (
            "a splice through an alias",
            "const al = xs; for (let i = 0; i < xs.length; i++) { al.splice(0, 1); xs[i] = 1; }",
        ),
    ] {
        let d = dump(&program(body));
        assert_eq!(kept(&d).0, 0, "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

#[test]
fn the_switch_sabotage_claims_every_store() {
    let src = program("xs[n + 3] = 1;");
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = crate::FactsConfig {
        sabotage: Some("inbounds".into()),
        ..Default::default()
    };
    let d = crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text();
    assert!(sink_is_int(&d), "{d}");
}
