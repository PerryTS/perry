//! Int demand and the representation a binding gets (`demand`,
//! `bindings::Rep`, representation RFC §10.3).

use std::collections::HashMap;

use crate::bindings::Rep;

/// `name -> rep` for every `let`/parameter of `t.ts`.
fn reps(src: &str) -> HashMap<String, Rep> {
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
            out.entry(name.clone()).or_insert(b.rep);
        }
    }
    out
}

#[test]
fn an_index_or_bitwise_operand_is_i32_and_double_only_arithmetic_is_f64() {
    let r = reps(
        "const a: number[] = [1, 2, 3, 4];\n\
         let acc = 0.5;\n\
         for (let i = 0; i < 4; i++) { const k = i + 1; acc = acc * 1.5 + a[k & 3]; }\n\
         for (let j = 0; j < 4; j++) { acc = acc + j * 0.25; }\n\
         console.log(acc);",
    );
    assert_eq!(r["i"], Rep::I32, "{r:?}");
    assert_eq!(r["k"], Rep::I32, "{r:?}");
    // `j` feeds only double arithmetic: an i32 slot would only convert.
    assert_eq!(r["j"], Rep::F64, "{r:?}");
    assert_eq!(r["acc"], Rep::F64, "{r:?}");
}

#[test]
fn demand_crosses_a_call_into_the_argument() {
    let r = reps(
        "function on(n: number): boolean { return (n & 0x88) === 0; }\n\
         let hits = 0;\n\
         for (let s = 0; s < 128; s++) { const t = s + 17; if (on(t)) hits++; }\n\
         console.log(hits);",
    );
    assert_eq!(r["n"], Rep::I32, "{r:?}");
    assert_eq!(r["t"], Rep::I32, "{r:?}");
}

#[test]
fn a_binding_that_may_hold_a_non_number_is_boxed() {
    let r = reps(
        "let v: any = 1; if (Math.random() < 0.5) v = 'x';\n\
         const w = 3 & 1;\n\
         console.log(v, w);",
    );
    assert_eq!(r["v"], Rep::Boxed, "{r:?}");
    // An int32 no int consumer reads is a double: demand, not the
    // operator that made it, decides.
    assert_eq!(r["w"], Rep::F64, "{r:?}");
}

#[test]
fn demand_crosses_a_summarized_predicate_call() {
    // `onBoard` is a predicate summary (`bind_summarized`): its call adds no
    // argument-to-parameter copy edge, yet its `&` still demands `source`.
    let r = reps(
        "const KING = [-17, -16, -15, -1, 1, 15, 16, 17];\n\
         function onBoard(square: number): boolean { return square >= 0 && square < 128 && (square & 0x88) === 0; }\n\
         function ray(from: number): number {\n\
           let hits = 0;\n\
           for (const delta of KING) {\n\
             let source = from + delta;\n\
             while (onBoard(source)) { hits++; source += delta; }\n\
           }\n\
           return hits;\n\
         }\n\
         let n = 0;\n\
         for (let s = 0; s < 128; s++) n += ray(s);\n\
         console.log(n);",
    );
    assert_eq!(r["source"], Rep::I32, "{r:?}");
    assert_eq!(r["delta"], Rep::I32, "{r:?}");
    assert_eq!(r["from"], Rep::I32, "{r:?}");
}

/// The method call sites of `t.ts` whose result the facts prove a Number
/// (`ModuleBindings::number_calls`), by the source text after the call's
/// `.` (SWC offsets are 1-based; a site's offset is its property's).
fn number_call_at(src: &str, at: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &Default::default());
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let start = src.find(at).expect("call in source") as u32 + 1;
    mb.number_calls
        .iter()
        .any(|o| (start..start + at.len() as u32).contains(o))
}

#[test]
fn a_call_whose_every_callee_returns_a_number_is_a_number_call() {
    let src = "class B {\n\
                 board: number[] = new Array<number>(128).fill(0);\n\
                 at(square: number): number { return this.board[square] ?? 0; }\n\
                 name(k: number): number | string { return k > 3 ? 'x' : k; }\n\
                 sum(): number { let s = 0; for (let i = 0; i < 8; i++) s += this.at(i); return s; }\n\
                 show(): string { return String(this.name(5)); }\n\
               }\n\
               const b = new B(); console.log(b.sum(), b.show());";
    assert!(number_call_at(src, "this.at(i)"));
    assert!(!number_call_at(src, "this.name(5)"));
}
