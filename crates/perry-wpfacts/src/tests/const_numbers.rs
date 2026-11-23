//! Module bindings that hold one Number (`const_numbers`, N6): each test
//! names the values a binding takes at run time and checks that a constant
//! is exported only when every one of them is that Number.

use std::collections::HashMap;

/// `name -> exported constant` for the top-level bindings of `t.ts`.
fn consts_with(src: &str, cfg: &crate::FactsConfig) -> HashMap<String, i64> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let mut out = HashMap::new();
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, name, .. } = s {
            if let Some((_, v)) = mb.const_numbers.iter().find(|(i, _)| i == id) {
                out.insert(name.clone(), *v);
            }
        }
    }
    out
}

fn consts(src: &str) -> HashMap<String, i64> {
    consts_with(src, &Default::default())
}

#[test]
fn a_shifted_flag_const_is_its_value() {
    let c = consts(
        "const EP = 1 << 17; const CASTLE = 1 << 18; const NEG = -(1 << 20);\n\
         const SUM = EP + CASTLE;\n\
         function has(m: number): boolean { return (m & EP) !== 0 || (m & CASTLE) !== 0; }\n\
         console.log(has(3), NEG, SUM);",
    );
    assert_eq!(c.get("EP"), Some(&131072));
    assert_eq!(c.get("CASTLE"), Some(&262144));
    assert_eq!(c.get("NEG"), Some(&-1048576));
    assert_eq!(c.get("SUM"), Some(&393216));
}

const TWO_VALUES: &str = "function pick(n: number): number { return n > 100 ? 1 : 2; }\n\
     const K = pick(process.argv.length);\n\
     function get(): number { return K; }\n\
     console.log(get());";

#[test]
fn a_const_of_two_possible_numbers_is_no_constant() {
    assert_eq!(consts(TWO_VALUES).get("K"), None);
}

#[test]
fn the_sabotage_claims_the_lower_bound() {
    let cfg = crate::FactsConfig {
        sabotage: Some("constnum".into()),
        ..Default::default()
    };
    assert_eq!(consts_with(TWO_VALUES, &cfg).get("K"), Some(&1));
}

#[test]
fn the_switch_turns_the_exact_intervals_and_the_export_off() {
    let cfg = crate::FactsConfig {
        no_const_numbers: true,
        ..Default::default()
    };
    let src = "const EP = 1 << 17; const FIVE = 5 + 0;\n\
               function has(m: number): boolean { return (m & EP) !== 0; }\n\
               console.log(has(3), FIVE);";
    assert!(consts_with(src, &cfg).is_empty());
    assert_eq!(consts(src).get("EP"), Some(&131072));
    assert_eq!(consts(src).get("FIVE"), Some(&5));
}

/// The HIR does not tell `let` from `var`; neither is exported, even when
/// the one value it is given is a Number.
#[test]
fn a_let_or_var_is_not_exported() {
    let c = consts(
        "let A = 7; var V = 9;\n\
         function get(): number { return A + V; }\n\
         console.log(get());",
    );
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn a_var_read_before_its_initializer_is_not_exported() {
    let c = consts(
        "function early() { return X; }\n\
         console.log(early());\n\
         var X = 5;\n\
         console.log(early());",
    );
    assert_eq!(c.get("X"), None);
}

#[test]
fn a_let_without_initializer_holds_undefined() {
    let c = consts(
        "let L: number | undefined;\n\
         function get() { return L; }\n\
         console.log(get());\n\
         L = 5;\n\
         console.log(get());",
    );
    assert_eq!(c.get("L"), None);
}

#[test]
fn a_non_integer_or_non_number_binding_is_no_constant() {
    let c = consts(
        "const HALF = 1 / 2; const S = \"s\"; const B = true; const BIG = 10n;\n\
         const MIXED = Math.random() < 2 ? 1 : \"one\";\n\
         const UNKNOWN = Math.floor(Math.random() * 4);\n\
         function get() { return [HALF, S, B, BIG, MIXED, UNKNOWN]; }\n\
         console.log(get());",
    );
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn a_function_local_binding_is_not_exported() {
    let c = consts(
        "function make() { const INNER = 1 << 4; return () => INNER; }\n\
         console.log(make()());",
    );
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn a_const_computed_by_a_call_is_its_value_when_the_intervals_pin_it() {
    let c = consts(
        "function width(): number { return 8; }\n\
         const W = width(); const AREA = W * W;\n\
         function get(): number { return AREA + W; }\n\
         console.log(get());",
    );
    // Sound either way: when exported, it is the run-time value.
    if let Some(v) = c.get("W") {
        assert_eq!(*v, 8);
    }
    if let Some(v) = c.get("AREA") {
        assert_eq!(*v, 64);
    }
}
