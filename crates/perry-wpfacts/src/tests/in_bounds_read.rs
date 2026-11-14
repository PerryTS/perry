//! In-bounds reads (`gen/in_bounds.rs`, `Cons::IdxLoad` with `kept`): a
//! read whose key the text bounds by a length guard is not past the end of
//! an Array that never shrinks, so it yields no `undefined` when the Array
//! has no hole.

fn dump_with(src: &str, sabotage: Option<&str>) -> String {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    // The probe lines are diagnostics.
    let cfg = crate::FactsConfig {
        diagnostics: true,
        sabotage: sabotage.map(str::to_string),
        ..Default::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text()
}

fn dump(src: &str) -> String {
    dump_with(src, None)
}

/// `sink` (the module's first function) takes what `body` reads from the
/// pushed Array `xs` (`n` elements, int32 Numbers).
fn program(body: &str) -> String {
    format!(
        "function sink(m: number): number {{ return m & 7; }}\n\
         function make(n: number): number[] {{\n\
           const src: number[] = [];\n\
           for (let k = 0; k < n; k++) src.push((k * 7) & 1023);\n\
           return src;\n\
         }}\n\
         function run(n: number): number {{\n\
           const xs = make(n);\n\
           let s = 0;\n\
           {body}\n\
           return s;\n\
         }}\n\
         console.log(run(40));"
    )
}

fn sink_is_int(d: &str) -> bool {
    d.lines()
        .any(|l| l.starts_with("# signature module=0 func=0 params=(i32"))
}

/// (kept and present, kept with a hole, not kept) read sites.
fn reads(d: &str) -> (usize, usize, usize) {
    let lines: Vec<&str> = d
        .lines()
        .filter(|l| l.starts_with("# inbounds_read "))
        .collect();
    let n = |pat: &str| lines.iter().filter(|l| l.ends_with(pat)).count();
    (
        n(" kept=1 present=1"),
        n(" kept=1 present=0"),
        n(" kept=0 present=0"),
    )
}

#[test]
fn a_read_at_the_counter_yields_a_number() {
    let d = dump(&program(
        "for (let i = 0; i < xs.length; i++) { s += sink(xs[i]); }",
    ));
    assert_eq!(reads(&d), (1, 0, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

#[test]
fn reads_below_the_counter_yield_numbers() {
    let d = dump(&program(
        "for (let i = 1; i < xs.length; i++) {\n\
           let j = i - 1;\n\
           while (j >= 0) { s += sink(xs[j]); j--; }\n\
           if (i >= 3) s += sink(xs[i - 3]);\n\
         }",
    ));
    assert_eq!(reads(&d), (2, 0, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

#[test]
fn a_guard_by_if_and_by_while_counts() {
    let d = dump(&program(
        "let k = 0;\n\
         while (k < xs.length) { s += sink(xs[k]); k += 3; }\n\
         const m = n >> 1;\n\
         if (m >= 0 && m < xs.length) s += sink(xs[m]);",
    ));
    assert_eq!(reads(&d).0, 2, "{d}");
    assert!(sink_is_int(&d), "{d}");
}

/// Soundness: each of these may read past the end or under a key that is
/// no index (or the text does not prove otherwise): the read keeps
/// `undefined`.
#[test]
fn an_unproven_read_keeps_undefined() {
    for (name, body) in [
        (
            "a key past the counter",
            "for (let i = 0; i < xs.length; i++) { s += sink(xs[i + 1]); }",
        ),
        (
            "a counter written before the read",
            "for (let i = 0; i < xs.length; i++) { i += 3; s += sink(xs[i]); }",
        ),
        (
            "a guard on another array",
            "const ys = make(n + 5); for (let i = 0; i < ys.length; i++) { s += sink(xs[i]); }",
        ),
        ("no guard", "s += sink(xs[n]);"),
        (
            "a bound at the length",
            "for (let i = 0; i <= xs.length; i++) { s += sink(xs[i]); }",
        ),
        (
            "a key that may be negative",
            "for (let i = 0; i < xs.length; i++) { s += sink(xs[i - 2]); }",
        ),
        (
            "a derived local that may fall below zero",
            "for (let i = 1; i < xs.length; i++) { let j = i - 1; j -= 4; s += sink(xs[j]); }",
        ),
        (
            "a key that may be a fraction",
            "for (let i = 0.5; i < xs.length; i++) { s += sink(xs[i]); }",
        ),
    ] {
        let d = dump(&program(body));
        assert_eq!(reads(&d).0, 0, "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// Soundness: the text proves the key, but the Array may shrink between
/// the guard and the read (here or through another name).
#[test]
fn a_shrinking_array_keeps_undefined() {
    for (name, body) in [
        (
            "a pop in the loop",
            "for (let i = 0; i < xs.length; i++) { xs.pop(); s += sink(xs[i]); }",
        ),
        (
            "a length store after the loop",
            "for (let i = 0; i < xs.length; i++) { s += sink(xs[i]); } xs.length = 2;",
        ),
        (
            "a splice through an alias",
            "const al = xs; for (let i = 0; i < xs.length; i++) { al.splice(0, 1); s += sink(xs[i]); }",
        ),
    ] {
        let d = dump(&program(body));
        assert_eq!(reads(&d), (0, 0, 1), "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// A kept read of an Array that may hold a hole is in bounds (no bound
/// test), and still yields the hole's `undefined`.
#[test]
fn a_hole_is_still_read() {
    let d = dump(&program(
        "xs[n + 3] = 1;\n\
         for (let i = 0; i < xs.length; i++) { s += sink(xs[i]); }",
    ));
    assert_eq!(reads(&d), (0, 1, 0), "{d}");
    assert!(!sink_is_int(&d), "{d}");
}

#[test]
fn the_sabotage_claims_every_read() {
    let d = dump_with(&program("s += sink(xs[n + 3]);"), Some("inread"));
    assert!(sink_is_int(&d), "{d}");
}
