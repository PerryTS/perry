//! Integer intervals on Number bindings (`model::range`): each test names the
//! values a binding takes at run time and checks the proven interval covers
//! them (soundness), and where a bound is expected, that it is found.

use std::collections::HashMap;

/// `name -> int_range` for every `let`/parameter of `t.ts` (the first binding
/// of a name wins; test programs use distinct names).
fn ranges(src: &str) -> HashMap<String, Option<(i64, i64)>> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &Default::default());
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let text = format!("{hir:?}");
    let mut names: HashMap<u32, String> = HashMap::new();
    for pat in ["Let { id: ", "Param { id: "] {
        let mut rest = text.as_str();
        while let Some(i) = rest.find(pat) {
            rest = &rest[i + pat.len()..];
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let (Ok(id), Some(j)) = (num.parse::<u32>(), rest.find("name: \"")) {
                let name: String = rest[j + 7..].chars().take_while(|c| *c != '"').collect();
                names.entry(id).or_insert(name);
            }
        }
    }
    let mut out = HashMap::new();
    for (id, b) in &mb.locals {
        if let Some(name) = names.get(id) {
            out.entry(name.clone()).or_insert(b.int_range);
        }
    }
    out
}

/// The proven interval of `name` covers `[lo, hi]` (or there is none).
fn covers(r: &HashMap<String, Option<(i64, i64)>>, name: &str, lo: i64, hi: i64) -> bool {
    match r.get(name) {
        Some(Some((a, b))) => *a <= lo && hi <= *b,
        Some(None) => true,
        None => panic!("no binding {name} in {r:?}"),
    }
}

#[test]
fn literals_and_operators_have_exact_intervals() {
    let r = ranges(
        "const x = 5; const y = x + 2; const z = y & 127; const w = (y * 3) % 4;\n\
         console.log(x, y, z, w);",
    );
    assert_eq!(r["x"], Some((5, 5)));
    assert_eq!(r["y"], Some((7, 7)));
    assert_eq!(r["z"], Some((0, 7)));
    assert!(covers(&r, "w", 1, 1));
}

#[test]
fn a_loop_test_bounds_its_counter() {
    let r = ranges(
        "function f(n: number): number {\n\
           let s = 0;\n\
           for (let i = 0; i < 100; i++) { if ((i & 8) !== 0) { i += 7; continue; } s += i * n; }\n\
           return s;\n\
         }\n\
         console.log(f(1), f(2));",
    );
    let (lo, hi) = r["i"].expect("the counter is bounded");
    assert!(lo <= 0 && hi >= 107, "i holds 0..=107: {:?}", r["i"]);
    // Widening climbs a threshold ladder; the test bound stops it well below
    // the int32 range.
    assert!(hi <= 1023, "the test bounds the climb: {:?}", r["i"]);
    // The accumulator has no bound.
    assert_eq!(r["s"], None);
}

#[test]
fn a_filter_follows_its_host_as_the_host_grows() {
    // The element interval grows while the loop counter's does; the `??`
    // result must cover the final one, not the first delivery.
    let r = ranges(
        "const arr: number[] = [];\n\
         for (let i = 0; i < 50; i++) arr.push(i & 15);\n\
         class B {\n\
           board: number[] = new Array<number>(128).fill(0);\n\
           set(i: number, v: number): void { this.board[i] = v; }\n\
           at(i: number): number { return this.board[i] ?? 0; }\n\
         }\n\
         const b = new B();\n\
         for (let k = 0; k < 100; k++) b.set(k & 127, (k & 7) - 3);\n\
         let t = 0;\n\
         for (let j = 0; j < 50; j++) {\n\
           const e = arr[j] ?? 0;\n\
           const p = b.at(j & 127);\n\
           const a = Math.abs(p);\n\
           const m = Math.min(7, p + 4);\n\
           t += e + p + a + m;\n\
         }\n\
         console.log(t);",
    );
    assert!(covers(&r, "e", 0, 15), "e: {:?}", r["e"]);
    assert!(covers(&r, "p", -3, 4), "p: {:?}", r["p"]);
    assert!(covers(&r, "a", 0, 4), "a: {:?}", r["a"]);
    assert!(covers(&r, "m", 1, 7), "m: {:?}", r["m"]);
}

#[test]
fn negative_zero_is_never_proven_an_integer() {
    let r = ranges(
        "function g(x: number): number { const n = -x; const z = x * -1; const q = -0; return n + z + q; }\n\
         console.log(g(0), g(3));",
    );
    assert_eq!(r["n"], None);
    assert_eq!(r["z"], None);
    assert_eq!(r["q"], None);
}

#[test]
fn a_parameter_covers_every_call() {
    let r = ranges(
        "function h(p: number): number { let acc = 0; for (let i = 0; i < p; i++) acc += i; return acc + p; }\n\
         console.log(h(3), h(40), h(-2));",
    );
    assert!(covers(&r, "p", -2, 40), "p: {:?}", r["p"]);
}
