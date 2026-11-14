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
        "for (let i = 3; i < xs.length; i++) {\n\
           let j = i - 1;\n\
           while (j >= 0) { s += sink(xs[j]); j--; }\n\
           s += sink(xs[i - 3]);\n\
         }",
    ));
    assert_eq!(reads(&d), (2, 0, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

#[test]
fn a_guard_by_if_and_by_while_counts() {
    let d = dump(&program(
        "let k = 0;\n\
         while (k < xs.length) { s += sink(xs[k]); k++; }\n\
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

/// CWN review 1: a kept read of an Array without a floor (a pushed `[]`,
/// no birth length) whose key may be no index gets its `undefined` from
/// `elem_in_floor` (no length interval: `Some(false)`); so does one of an
/// Array with a floor (a literal birth) past the floor's reach.
#[test]
fn a_key_one_below_the_counter_from_zero_keeps_undefined() {
    for (name, body) in [
        (
            "no floor",
            "for (let i = 0; i < xs.length; i++) { s += sink(xs[i - 1]); }",
        ),
        (
            "a literal birth",
            "const ys = [1, 2, 3]; ys.push(n);\n\
             for (let i = 0; i < ys.length; i++) { s += sink(ys[i - 1]); }",
        ),
    ] {
        let d = dump(&program(body));
        assert_eq!(reads(&d).0, 0, "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// An Array with a birth length that also grows: the kept read needs no
/// floor (the key is past the birth length's reach, the text bounds it).
#[test]
fn a_literal_birth_that_grows_is_read_in_bounds() {
    let d = dump(&program(
        "const ys = [1, 2, 3]; ys.push(n & 7);\n\
         for (let i = 0; i < ys.length; i++) { s += sink(ys[i]); }",
    ));
    assert_eq!(reads(&d), (1, 0, 0), "{d}");
    assert!(sink_is_int(&d), "{d}");
}

/// CWN review 3: every event that may shrink the Array withdraws the kept
/// read, wherever it stands in the source and whenever the solve meets it:
/// each of these comes after the read was generated.
#[test]
fn a_later_shrink_by_any_route_withdraws_the_read() {
    let read = "for (let i = 0; i < xs.length; i++) { s += sink(xs[i]); }\n";
    for (name, after) in [
        ("a callee that stores length", "cut(xs);"),
        ("pop through call", "Array.prototype.pop.call(xs);"),
        ("shift", "xs.shift();"),
        ("splice", "xs.splice(1, 1);"),
        ("a delete", "delete xs[0];"),
        ("Reflect.deleteProperty", "Reflect.deleteProperty(xs, 0);"),
        (
            "defineProperty of length",
            "Object.defineProperty(xs, \"length\", { value: 0 });",
        ),
        ("Object.assign", "Object.assign(xs, { length: 0 });"),
        ("Reflect.set of length", "Reflect.set(xs, \"length\", 0);"),
        (
            "a computed key store",
            "(xs as any)[n > 3 ? \"length\" : \"x\"] = 0;",
        ),
        (
            "an escape to unknown code",
            "(globalThis as any).unknown(xs);",
        ),
        ("a length store that grows", "xs.length = n + 5;"),
    ] {
        let src = program(&format!("{read}{after}")).replace(
            "function run(",
            "function cut(a: number[]): void { a.length = 0; }\nfunction run(",
        );
        let d = dump(&src);
        assert_eq!(reads(&d), (0, 0, 1), "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// Events that keep the length and make no hole keep the read.
#[test]
fn a_permutation_or_a_push_keeps_the_read() {
    for (name, after) in [
        ("sort", "xs.sort();"),
        ("push", "xs.push(1);"),
        ("fill", "xs.fill(2);"),
    ] {
        let d = dump(&program(&format!(
            "for (let i = 0; i < xs.length; i++) {{ s += sink(xs[i]); }}\n{after}"
        )));
        assert_eq!(reads(&d), (1, 0, 0), "{name}\n{d}");
    }
}

/// CWN review 4: an Array born with holes is read in bounds, and the
/// hole's `undefined` stays (`present=0`).
#[test]
fn an_array_born_with_holes_is_not_present() {
    for (name, body) in [
        (
            "new Array(n)",
            "const ys = new Array<number>(n); ys[0] = 1;",
        ),
        ("Array(n)", "const ys = Array<number>(n); ys[0] = 1;"),
        ("a literal with a hole", "const ys = [1, , 3]; ys.push(n);"),
    ] {
        let d = dump(&program(&format!(
            "{body}\nfor (let i = 0; i < ys.length; i++) {{ s += sink(ys[i]); }}"
        )));
        assert_eq!(reads(&d).0, 0, "{name}\n{d}");
        assert!(!sink_is_int(&d), "{name}\n{d}");
    }
}

/// Two reads at one offset (`rows[i][0]`: both member expressions start
/// at `rows`): the inner one is text-proven, the outer is not, so the
/// offset carries no claim.
#[test]
fn a_shared_offset_with_an_unproven_read_is_not_exported() {
    let d = dump(
        "function sink(m: number): number { return m & 7; }\n\
         function run(n: number): number {\n\
           const rows: number[][] = [];\n\
           for (let k = 0; k < n; k++) rows.push([k, k + 1]);\n\
           let s = 0;\n\
           for (let i = 0; i < rows.length; i++) { s += sink(rows[i][0] ?? 0); }\n\
           return s;\n\
         }\n\
         console.log(run(40));",
    );
    assert_eq!(reads(&d).0 + reads(&d).1, 0, "{d}");
}

/// With intervals off the key of a read has no interval: nothing is kept.
#[test]
fn without_intervals_nothing_is_kept() {
    let src = program("for (let i = 0; i < xs.length; i++) { s += sink(xs[i]); }");
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = crate::FactsConfig {
        diagnostics: true,
        no_int_ranges: true,
        ..Default::default()
    };
    let d = crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text();
    assert_eq!(reads(&d).0, 0, "{d}");
}
