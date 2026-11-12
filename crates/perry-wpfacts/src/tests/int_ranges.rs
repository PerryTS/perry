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

/// `name` has no finite bound: no interval, or one reaching
/// `range::BIG`.
fn unbounded(r: &HashMap<String, Option<(i64, i64)>>, name: &str) -> bool {
    use crate::model::range::BIG;
    match r.get(name) {
        Some(Some((lo, hi))) => *lo <= -BIG || *hi >= BIG,
        Some(None) => true,
        None => panic!("no binding {name} in {r:?}"),
    }
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
    // Two single values: the mask's exact result (`range::eval_exact`).
    assert_eq!(r["z"], Some((7, 7)));
    assert!(covers(&r, "w", 1, 1));
}

/// A mask of an interval that is not one value keeps the mask's bound.
#[test]
fn a_mask_of_an_interval_is_bounded_by_the_mask() {
    let r = ranges(
        "function f(n: number): number { let s = 0; for (let i = 0; i < 9; i++) { const z = (i + n) & 127; s += z; } return s; }\n\
         console.log(f(1), f(2));",
    );
    assert_eq!(r["z"], Some((0, 127)));
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
    assert!(unbounded(&r, "s"), "s: {:?}", r["s"]);
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
    assert!(lo <= 0 && (8..=127).contains(&hi), "j: {:?}", r["j"]);
    // `push` raises G's length to any array length (the length floor,
    // `solve::arr_len`): its counter's bound is the largest index.
    assert!(covers(&r, "h", 0, u32::MAX as i64 - 1), "h: {:?}", r["h"]);
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
    // The length floor (`solve::arr_len`): the length is `[4, 10]`.
    assert!(covers(&r, "j", 0, 9), "j: {:?}", r["j"]);
}

#[test]
fn an_index_string_key_past_the_birth_length_moves_it() {
    // The key holds Number 1 first and the constant "9" only later (through
    // calls): the element store is already there when "9" arrives, so the
    // index string itself must name its index (`Cons::LenIndex`).
    let r = ranges(
        "const K = [1, 2, 3, 4];\n\
         function g(): number { let s = 0; for (let j = 0; j < K.length; j++) s += K[j]; return s; }\n\
         function id(x: any): any { return x; }\n\
         function put(k: any): void { K[k] = 0; }\n\
         put(1); put(id(id(id(\"9\"))));\n\
         console.log(g());",
    );
    assert!(covers(&r, "j", 0, 9), "j: {:?}", r["j"]);
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
    assert!(unbounded(&r, "j"), "j: {:?}", r["j"]);
    assert!(unbounded(&r, "q"), "q: {:?}", r["q"]);
}

#[test]
fn an_array_unknown_code_holds_has_no_length() {
    let r = ranges(
        "const M = [1, 2, 3, 4];\n\
         function g(): number { let s = 0; for (let w = 0; w < M.length; w++) s += w; return s; }\n\
         (globalThis as any).someHostHook?.(M);\n\
         setTimeout(() => console.log(g()), 0);",
    );
    assert!(unbounded(&r, "w"), "w: {:?}", r["w"]);
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
    assert!(unbounded(&r, "a"), "Reflect.set length: {:?}", r["a"]);
    assert!(unbounded(&r, "b"), "Object.assign: {:?}", r["b"]);
    assert!(unbounded(&r, "c"), "splice through an alias: {:?}", r["c"]);
    assert!(unbounded(&r, "d"), "computed-key store: {:?}", r["d"]);
    assert!(
        unbounded(&r, "e"),
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
fn a_guard_narrows_the_code_it_guards() {
    let r = ranges(
        "function f(x: number, k: number): number {\n\
           const i = x | 0;\n\
           let s = 0;\n\
           if (i >= 0 && i < 128) { const a = i; s += a; }\n\
           if (i < 0 || i > 63) { s += 1; } else { const b = i; s += b; }\n\
           if (i >= 100) return s;\n\
           const c = i;\n\
           let t = k | 0;\n\
           while (t >= 0 && t < 10) { const d = t; s += d; t += 3; }\n\
           const e = i > 5 ? i : 5;\n\
           const g = i < 0 && i > -4 ? i : 0;\n\
           return s + c + e + g;\n\
         }\n\
         console.log(f(3, 1), f(-7, 2), f(200, -1), f(70, 4));",
    );
    assert_eq!(r["a"], Some((0, 127)), "a: {:?}", r["a"]);
    assert_eq!(r["b"], Some((0, 63)), "b: {:?}", r["b"]);
    assert_eq!(r["c"].map(|(_, hi)| hi), Some(99), "c: {:?}", r["c"]);
    assert_eq!(r["d"], Some((0, 9)), "d: {:?}", r["d"]);
    assert!(covers(&r, "e", 5, 99), "e: {:?}", r["e"]);
    assert!(covers(&r, "g", -3, 0), "g: {:?}", r["g"]);
}

#[test]
fn a_guard_never_outlives_a_write_or_a_closure() {
    let r = ranges(
        "function f(x: number): number {\n\
           let i = x | 0;\n\
           let s = 0;\n\
           if (i < 10) { i = i + 1000; const a = i; s += a; }\n\
           let j = x | 0;\n\
           const bump = () => { j = 5000; };\n\
           if (j < 10) { bump(); const b = j; s += b; }\n\
           let m = x | 0;\n\
           if (m < 10) { for (let q = 0; q < 3; q++) { const c = m; s += c; m += 100; } }\n\
           return s;\n\
         }\n\
         console.log(f(3), f(-2));",
    );
    assert!(covers(&r, "a", 1003, 1003), "a: {:?}", r["a"]);
    assert!(covers(&r, "b", 5000, 5000), "b: {:?}", r["b"]);
    assert!(covers(&r, "c", 103, 203), "c: {:?}", r["c"]);
}

#[test]
fn a_predicate_call_narrows_its_local_arguments() {
    let r = ranges(
        "function onBoard(s: number): boolean { return s >= 0 && s < 128 && (s & 0x88) === 0; }\n\
         function sneaky(s: number): boolean { return (s = 5) > 3 && s < 10; }\n\
         function f(x: number): number {\n\
           const i = x | 0;\n\
           let t = 0;\n\
           if (onBoard(i)) { const a = i; t += a; }\n\
           if (sneaky(i)) { const c = i; t += c; }\n\
           if (!onBoard(i)) return t;\n\
           const b = i;\n\
           return t + b;\n\
         }\n\
         console.log(f(3), f(-7), f(500));",
    );
    assert_eq!(r["a"], Some((0, 127)), "a: {:?}", r["a"]);
    // What follows `if (!onBoard(i)) return` runs only where it holds.
    assert_eq!(r["b"], Some((0, 127)), "b: {:?}", r["b"]);
    // A predicate that writes its parameter implies nothing.
    assert!(covers(&r, "c", -7, 500), "c: {:?}", r["c"]);
}

#[test]
fn a_digit_tested_character_converts_to_its_value() {
    let r = ranges(
        "function parse(rank: string): number {\n\
           let file = 0;\n\
           let last = 0;\n\
           for (let i = 0; i < rank.length; i++) {\n\
             const c = rank.charAt(i);\n\
             if (c >= \"1\" && c <= \"8\") { const d = Number(c); file += d; continue; }\n\
             const w = Number(c);\n\
             if (file >= 8) throw new Error(\"bad\");\n\
             const sq = 16 + file;\n\
             last = sq + w * 0;\n\
             file++;\n\
           }\n\
           return last;\n\
         }\n\
         console.log(parse(\"3pppk2\"), parse(\"8\"));",
    );
    assert_eq!(r["d"], Some((1, 8)), "d: {:?}", r["d"]);
    // Outside the test `c` may be any character: `Number` may be NaN.
    assert_eq!(r["w"], None, "w: {:?}", r["w"]);
    // `file >= 8` throws: `sq` is bounded (widening may round its upper
    // bound up to the next threshold).
    let (lo, hi) = r["sq"].expect("sq is bounded");
    assert!(lo == 16 && (23..=143).contains(&hi), "sq: {:?}", r["sq"]);
}

#[test]
fn a_pop_moves_the_length() {
    let r = ranges(
        "const S: number[] = [1, 2, 3, 4, 5];\n\
         function f(): number { let s = 0; for (let p = 0; p < S.length; p++) { s += p; S.pop(); } return s; }\n\
         console.log(f());",
    );
    assert!(
        unbounded(&r, "p") || r["p"].is_some_and(|(_, hi)| hi > 4),
        "p: {:?}",
        r["p"]
    );
}

#[test]
fn a_conjunction_and_an_expression_bound_a_for_counter() {
    let r = ranges(
        "function passed(square: number, up: boolean): number {\n\
           const file = square & 7;\n\
           let hits = 0;\n\
           for (let rr = (square >> 4) + (up ? 1 : -1); rr >= 0 && rr < 8; rr += up ? 1 : -1) {\n\
             for (let ff = Math.max(0, file - 1); ff <= Math.min(7, file + 1); ff++) {\n\
               const idx = rr * 16 + ff;\n\
               hits += idx;\n\
             }\n\
           }\n\
           return hits;\n\
         }\n\
         console.log(passed(0x34, true), passed(0x61, false));",
    );
    assert!(
        r["idx"].is_some_and(|(lo, hi)| lo >= 0 && hi <= 127),
        "idx: {:?}",
        r["idx"]
    );
}

/// A product past 2^60 is no empty interval (`Iv::int`, the bounds of
/// doubles): the binding it is assigned to leaves the int32 range. `h` holds
/// 1406932606, 1.98e18 and 3.9e36; `s` holds 2147483647 and -2147483648.
#[test]
fn a_binding_assigned_a_product_past_2_60_is_unbounded() {
    let r = ranges(
        "let h = 1406932606; h *= h; h *= h;\n\
         let c = 1406932606; c = c * c + 1;\n\
         let n = -3037000500; n = n * 3037000500;\n\
         let s = 2147483647; s <<= 31;\n\
         let k = 46340; k *= k;\n\
         function sq(p: number): number { let q = p; q = q * q; return q; }\n\
         console.log(~h, ~c, ~n, s, k, sq(4294967296));",
    );
    for name in ["h", "c", "n", "q"] {
        assert!(unbounded(&r, name), "{name}: {:?}", r[name]);
    }
    assert!(covers(&r, "s", -2147483648, 2147483647), "{:?}", r["s"]);
    // The control: a product inside int32 keeps its exact interval.
    assert!(
        r["k"].is_some_and(|(lo, hi)| lo >= 0 && (2147395600..=2147483647).contains(&hi)),
        "k: {:?}",
        r["k"]
    );
}
