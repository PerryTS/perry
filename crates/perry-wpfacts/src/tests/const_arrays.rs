//! Fact IM (`const_arrays`): an Array nothing writes after its birth is
//! immutable (`ArrRep::im`), and a binding that holds only immutable
//! literals of the same Numbers has constant contents
//! (`BindingFacts::const_elems`).

use super::*;
use crate::bindings::Rep;
use crate::FactsConfig;
use std::collections::HashMap;

/// `name -> (im, constant contents)` of every `let`/`const` of `t.ts`.
fn consts_with(src: &str, cfg: FactsConfig) -> HashMap<String, (bool, Option<Vec<f64>>)> {
    let pf = facts_with(src, cfg);
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let text = format!("{hir:?}");
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Let { id: ") {
        rest = &rest[i + "Let { id: ".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let (Ok(id), Some(j)) = (num.parse::<u32>(), rest.find("name: \"")) {
            let name: String = rest[j + 7..].chars().take_while(|c| *c != '"').collect();
            names.entry(id).or_insert(name);
        }
    }
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let mut out = HashMap::new();
    for (id, b) in &mb.locals {
        if let Some(name) = names.get(id) {
            let im = matches!(b.rep, Rep::Arr(r) if r.im);
            let c = b
                .const_elems
                .as_ref()
                .map(|c| c.iter().map(|b| f64::from_bits(*b)).collect());
            out.entry(name.clone()).or_insert((im, c));
        }
    }
    out
}

fn consts(src: &str) -> HashMap<String, (bool, Option<Vec<f64>>)> {
    consts_with(src, FactsConfig::default())
}

fn im(src: &str, name: &str) -> bool {
    consts(src)
        .get(name)
        .unwrap_or_else(|| panic!("no binding {name}"))
        .0
}

fn elems(src: &str, name: &str) -> Option<Vec<f64>> {
    consts(src)
        .get(name)
        .unwrap_or_else(|| panic!("no binding {name}"))
        .1
        .clone()
}

#[test]
fn a_number_literal_nothing_writes_has_constant_contents() {
    let src = "const K = [-33, -31, 14.5, 33];\nlet s = 0;\nfor (const d of K) s += d;\nconsole.log(s, K[1], K.length, K.indexOf(14.5), K.slice(1).length);\n";
    assert!(im(src, "K"));
    assert_eq!(elems(src, "K"), Some(vec![-33.0, -31.0, 14.5, 33.0]));
}

#[test]
fn the_for_of_source_temp_has_the_contents() {
    let src = "const K = [1, 2, 3];\nfunction f(): number {\n  let s = 0;\n  for (const d of K) s += d;\n  return s;\n}\nconsole.log(f());\n";
    let all = consts(src);
    let temp = all
        .iter()
        .find(|(n, _)| n.starts_with("__forof_lit_src_"))
        .unwrap_or_else(|| panic!("no source temp in {all:?}"));
    assert_eq!(temp.1, &(true, Some(vec![1.0, 2.0, 3.0])));
}

#[test]
fn an_index_store_makes_it_mutable() {
    for write in [
        "K[1] = 9;",
        "const a = K; a[0] = 9;",
        "function w(x: number[]): void { x[2] = 9; }\nw(K);",
        "K[0]++;",
        "K[1] += 2;",
        "(K as any).tag = 1;",
        "[K[0], K[1]] = [K[1], K[0]];",
    ] {
        let src = format!("const K = [1, 2, 3];\n{write}\nconsole.log(K[0]);\n");
        assert!(!im(&src, "K"), "{write}");
        assert_eq!(elems(&src, "K"), None, "{write}");
    }
}

#[test]
fn an_in_place_method_makes_it_mutable() {
    for write in [
        "K.sort();",
        "K.reverse();",
        "K.fill(0);",
        "K.copyWithin(0, 1);",
        "K.push(4);",
        "K.pop();",
        "K.shift();",
        "K.unshift(0);",
        "K.splice(0, 1, 9);",
        "K.length = 2;",
        "Array.prototype.sort.call(K);",
        "Array.prototype.fill.call(K, 7);",
        "Object.assign(K, { 1: 9 });",
        "Object.defineProperty(K, 1, { value: 9 });",
        "delete K[1];",
        "Reflect.set(K, 1, 9);",
    ] {
        let src = format!("const K = [1, 2, 3];\n{write}\nconsole.log(K[0]);\n");
        assert!(!im(&src, "K"), "{write}");
        assert_eq!(elems(&src, "K"), None, "{write}");
    }
}

#[test]
fn a_reading_method_keeps_it_immutable() {
    for read in [
        "K.indexOf(2)",
        "K.includes(2)",
        "K.join(',')",
        "K.slice(1)",
        "K.map((x) => x + 1)",
        "K.filter((x) => x > 1)",
        "K.reduce((a, b) => a + b, 0)",
        "K.toSorted()",
        "K.toReversed()",
        "K.with(0, 9)",
        "K.at(-1)",
        "K.concat([4])",
        "K.some((x) => x > 2)",
    ] {
        let src = format!("const K = [1, 2, 3];\nconsole.log({read});\n");
        assert!(im(&src, "K"), "{read}");
        assert_eq!(elems(&src, "K"), Some(vec![1.0, 2.0, 3.0]), "{read}");
    }
}

#[test]
fn a_callback_that_writes_its_receiver_makes_it_mutable() {
    let src = "const K = [1, 2, 3];\nK.forEach((x, i, a) => { a[i] = x + 1; });\nconsole.log(K[0]);\n";
    assert!(!im(src, "K"));
}

#[test]
fn an_escape_makes_it_mutable() {
    let src = "const K = [1, 2, 3];\nconst f: (a: number[]) => void = (globalThis as any).hook;\nf(K);\nconsole.log(K[0]);\n";
    assert!(!im(src, "K"));
    assert_eq!(elems(src, "K"), None);
}

#[test]
fn only_number_literals_have_contents() {
    for lit in [
        "['a', 'b']",
        "[1, 'x']",
        "[1, , 3]",
        "[{}, {}]",
        "[1, Math.random()]",
        "[1, process.argv.length]",
        "[true, false]",
        "[null]",
        "new Array(3)",
    ] {
        let src = format!("const K = {lit};\nconsole.log(K.length);\n");
        assert_eq!(elems(&src, "K"), None, "{lit}");
    }
    // Immutable all the same: nothing writes it.
    let src = "const K = ['a', 'b'];\nconsole.log(K.length);\n";
    assert!(im(src, "K"));
}

#[test]
fn a_constant_number_binding_is_a_constant_element() {
    let src = "const N = -16;\nconst E = 1 << 4;\nconst Z = 0;\nconst ROOK = [N, -N, 1, -1, E, -E, -Z];\nconsole.log(ROOK.length);\n";
    let got = elems(src, "ROOK").expect("constant contents");
    assert_eq!(got[..6], [-16.0, 16.0, 1.0, -1.0, 16.0, -16.0]);
    assert_eq!(got[6].to_bits(), (-0.0f64).to_bits(), "-Z of Z = 0 is -0");
}

#[test]
fn a_binding_that_is_no_constant_number_is_no_constant_element() {
    for decl in [
        "let N = 1; N = 2;",
        "const N = Math.random() < 0.5 ? 1 : 2;",
        "const N = 'a';",
        "function g(): number { return 3; }\nconst N = g() + process.argv.length;",
    ] {
        let src = format!("{decl}\nconst K = [N, 1];\nconsole.log(K.length, N);\n");
        assert_eq!(elems(&src, "K"), None, "{decl}");
        assert!(im(&src, "K"), "{decl}");
    }
    let off = FactsConfig {
        no_const_numbers: true,
        ..FactsConfig::default()
    };
    let src = "const N = 1 << 4;\nconst K = [N, 1];\nconsole.log(K.length);\n";
    assert_eq!(consts_with(src, off)["K"], (true, None));
}

#[test]
fn a_join_of_different_literals_is_immutable_without_contents() {
    let src = "const A = [1, 2];\nconst B = [3, 4, 5];\nfunction pick(c: boolean): number {\n  const d = c ? A : B;\n  return d.length;\n}\nconsole.log(pick(true), pick(false));\n";
    assert!(im(src, "d"));
    assert_eq!(elems(src, "d"), None);
    assert_eq!(elems(src, "A"), Some(vec![1.0, 2.0]));
}

#[test]
fn a_join_of_equal_literals_keeps_the_contents() {
    let src = "const A = [1, 2];\nconst B = [1, 2];\nfunction pick(c: boolean): number {\n  const d = c ? A : B;\n  return d.length;\n}\nconsole.log(pick(true), pick(false));\n";
    assert_eq!(elems(src, "d"), Some(vec![1.0, 2.0]));
}

#[test]
fn a_join_with_a_written_array_is_mutable() {
    let src = "const A = [1, 2];\nconst B = [1, 2];\nB[0] = 5;\nfunction pick(c: boolean): number {\n  const d = c ? A : B;\n  return d.length;\n}\nconsole.log(pick(true), pick(false));\n";
    assert!(!im(src, "d"));
    assert_eq!(elems(src, "d"), None);
    assert!(im(src, "A"));
}

#[test]
fn the_switch_turns_the_fact_off() {
    let src = "const K = [1, 2, 3];\nconsole.log(K[0]);\n";
    let cfg = FactsConfig {
        no_const_arrays: true,
        ..FactsConfig::default()
    };
    let all = consts_with(src, cfg);
    assert_eq!(all["K"], (false, None));
    let cfg = FactsConfig {
        no_array_reps: true,
        ..FactsConfig::default()
    };
    let all = consts_with(src, cfg);
    assert_eq!(all["K"], (false, None));
}

#[test]
fn the_sabotage_keeps_a_written_literal_constant() {
    let src = "const K = [1, 2, 3];\nK[1] = 9;\nconsole.log(K[0]);\n";
    let cfg = FactsConfig {
        sabotage: Some("arrconst".into()),
        ..FactsConfig::default()
    };
    let all = consts_with(src, cfg);
    assert_eq!(all["K"], (true, Some(vec![1.0, 2.0, 3.0])));
}

#[test]
fn the_dump_names_the_contents() {
    let src = "const K = [1, -2.5];\nconsole.log(K[0]);\n";
    let pf = facts_with(src, FactsConfig::default());
    let text = pf.dump_text();
    assert!(text.contains("elems=[1,-2.5]"), "{text}");
    assert!(text.contains(",fixed,im>"), "{text}");
}
