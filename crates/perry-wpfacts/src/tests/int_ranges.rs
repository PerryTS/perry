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

#[test]
fn an_array_literal_keeps_its_length_until_something_moves_it() {
    let r = ranges(
        "const K = [-33, -31, -18, -14, 14, 18, 31, 33];\n\
         const G: number[] = [1, 2, 3];\n\
         function f(sq: number): number {\n\
           let s = 0;\n\
           for (let j = 0; j < K.length; j++) { const kd = K[j]; s += kd * sq; }\n\
           for (let h = 0; h < G.length; h++) { const gd = G[h]; s += gd; }\n\
           return s;\n\
         }\n\
         G.push(4);\n\
         K[3] = 7;\n\
         console.log(f(2), f(3));",
    );
    // The in-range store `K[3] = 7` keeps K's length; widening stops the
    // climb at the ladder's next step.
    let (lo, hi) = r["j"].expect("j is bounded by K.length");
    assert!(lo <= 0 && (8..=15).contains(&hi), "j: {:?}", r["j"]);
    // `push` moves G's length: its counter has no bound.
    assert_eq!(r["h"], None, "h: {:?}", r["h"]);
}

#[test]
fn an_index_store_past_the_birth_length_moves_it() {
    let r = ranges(
        "const K = [1, 2, 3, 4];\n\
         function g(i: number): number { let s = 0; for (let j = 0; j < K.length; j++) s += K[j]; return s + i; }\n\
         function put(i: number): void { K[i] = 0; }\n\
         put(2); put(9);\n\
         console.log(g(1));",
    );
    assert_eq!(r["j"], None, "j: {:?}", r["j"]);
}

#[test]
fn a_length_store_or_an_escape_moves_it() {
    let r = ranges(
        "const K = [1, 2, 3, 4];\n\
         const L = [1, 2, 3, 4];\n\
         function g(): number { let s = 0; for (let j = 0; j < K.length; j++) s += j; for (let q = 0; q < L.length; q++) s += q; return s; }\n\
         K.length = 100;\n\
         Object.defineProperty(L, \"length\", { value: 1 });\n\
         console.log(g());",
    );
    assert_eq!(r["j"], None, "j: {:?}", r["j"]);
    assert_eq!(r["q"], None, "q: {:?}", r["q"]);
}

#[test]
fn an_array_unknown_code_holds_has_no_length() {
    let r = ranges(
        "const M = [1, 2, 3, 4];\n\
         function g(): number { let s = 0; for (let w = 0; w < M.length; w++) s += w; return s; }\n\
         (globalThis as any).someHostHook?.(M);\n\
         setTimeout(() => console.log(g()), 0);",
    );
    assert_eq!(r["w"], None, "w: {:?}", r["w"]);
}

#[test]
fn reflection_and_aliases_move_the_length() {
    let r = ranges(
        "const A = [1, 2, 3, 4];\n\
         const B = [1, 2, 3, 4];\n\
         const C = [1, 2, 3, 4];\n\
         const D = [1, 2, 3, 4];\n\
         const E = [1, 2];\n\
         function lens(): number {\n\
           let s = 0;\n\
           for (let a = 0; a < A.length; a++) s += a;\n\
           for (let b = 0; b < B.length; b++) s += b;\n\
           const alias: number[] = C;\n\
           for (let c = 0; c < C.length; c++) s += c;\n\
           for (let d = 0; d < D.length; d++) s += d;\n\
           const pick = s > 3 ? E : A;\n\
           for (let e = 0; e < pick.length; e++) s += e;\n\
           alias.splice(0, 0, 9, 9);\n\
           return s;\n\
         }\n\
         Reflect.set(A, \"length\", 50);\n\
         Object.assign(B, [0, 0, 0, 0, 0, 0, 0]);\n\
         const k: string = String(Date.now() % 2);\n\
         (D as any)[k + \"0\"] = 1;\n\
         console.log(lens());",
    );
    assert_eq!(r["a"], None, "Reflect.set length: {:?}", r["a"]);
    assert_eq!(r["b"], None, "Object.assign: {:?}", r["b"]);
    assert_eq!(r["c"], None, "splice through an alias: {:?}", r["c"]);
    assert_eq!(r["d"], None, "computed-key store: {:?}", r["d"]);
    assert_eq!(
        r["e"], None,
        "a holder of two arrays, one moved: {:?}",
        r["e"]
    );
}

#[test]
fn a_holder_of_two_fixed_arrays_joins_their_lengths() {
    let r = ranges(
        "const P = [1, 2];\n\
         const Q = [1, 2, 3, 4, 5, 6, 7, 8];\n\
         function f(x: number): number { const pick = x > 0 ? Q : P; let s = 0; for (let e = 0; e < pick.length; e++) s += pick[e]; return s; }\n\
         console.log(f(1), f(-1));",
    );
    let (lo, hi) = r["e"].expect("e is bounded by both lengths");
    assert!(lo <= 0 && hi >= 8, "e covers 0..=8: {:?}", r["e"]);
}

#[test]
fn a_pop_moves_the_length() {
    let r = ranges(
        "const S: number[] = [1, 2, 3, 4, 5];\n\
         function f(): number { let s = 0; for (let p = 0; p < S.length; p++) { s += p; S.pop(); } return s; }\n\
         console.log(f());",
    );
    // Without the pop S's length would bound `p` to 0..=5.
    assert!(r["p"].is_none_or(|(_, hi)| hi > 5), "p: {:?}", r["p"]);
}
