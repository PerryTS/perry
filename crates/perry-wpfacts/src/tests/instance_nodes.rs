//! Instance nodes (`bindings::BindingNodes`): a binding of a summarized
//! body is its shared node and every instance copy of it.

use super::*;
use crate::bindings::Rep;
use crate::FactsConfig;

/// The facts of the top-level `let` named `local` of function `func`.
fn local(pf: &ProgramFacts, src: &str, func: &str, local: &str) -> crate::bindings::BindingFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let f = hir
        .functions
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func}"));
    let id = f
        .body
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let { id, name, .. } if name == local => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("local {local} of {func}"));
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id} of {func}"))
        .1
        .clone()
}

/// The facts of the one `let` named `name` anywhere in the program.
fn local_in(pf: &ProgramFacts, src: &str, name: &str) -> crate::bindings::BindingFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let text = format!("{hir:?}");
    let at = text
        .find(&format!("name: \"{name}\""))
        .unwrap_or_else(|| panic!("local {name}"));
    let head = &text[..at];
    let id: u32 = head[head.rfind("Let { id: ").expect("a let") + "Let { id: ".len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("an id");
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id}"))
        .1
        .clone()
}

fn off() -> FactsConfig {
    FactsConfig {
        sabotage: Some("instlit".into()),
        ..FactsConfig::default()
    }
}

fn probed() -> FactsConfig {
    FactsConfig {
        diagnostics: true,
        ..FactsConfig::default()
    }
}

// `grown` and `kept` are solved per call site (the Array parameter gives
// them a slice), so `other` is one Array per instance. `fill` and `count`
// hold a loop: the HIR does not inline them.
const SRC: &str = r#"
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}
function count(a: number[]): number {
  let n = 0;
  for (let i = 0; i < a.length; i++) n++;
  return n;
}
function grown(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) { s += v + other.length; fill(other, 1); }
  const n = other.length;
  return s + n;
}
function kept(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) { s += v + other.length + count(other); }
  const n = other.length;
  return s + n;
}
console.log(grown([1, 4, 7, 10, 13]), grown([]), kept([1, 2]), kept([]));
"#;

#[test]
fn a_literal_a_callee_grows_has_no_constant_length() {
    let pf = facts_with(SRC, FactsConfig::default());
    let b = local(&pf, SRC, "grown", "other");
    let Rep::Arr(r) = b.rep else {
        panic!("`other` is an Array: {:?}", b.rep)
    };
    // The push lands on each instance's Array: the length moves.
    assert!(!r.fixed && !r.im, "{r:?}");
    assert_eq!(r.floor, 3, "{r:?}");
    assert_eq!(b.const_length, None);
    assert!(!b.immutable_array);
    assert_eq!(b.const_elems, None);
    // What the body derives from the length is the instances' too.
    let n = local(&pf, SRC, "grown", "n");
    assert_ne!(n.int_range, Some((3, 3)), "{n:?}");
}

#[test]
fn a_literal_only_read_keeps_its_birth_state() {
    // The negative control: nothing changes `other`, in any instance.
    let pf = facts_with(SRC, FactsConfig::default());
    let b = local(&pf, SRC, "kept", "other");
    let Rep::Arr(r) = b.rep else {
        panic!("`other` is an Array: {:?}", b.rep)
    };
    assert!(r.fixed && r.im && r.dense, "{r:?}");
    assert_eq!(r.floor, 3, "{r:?}");
    assert_eq!(b.const_length, Some(3));
    let n = local(&pf, SRC, "kept", "n");
    assert_eq!(n.int_range, Some((3, 3)), "{n:?}");
}

#[test]
fn the_shared_node_alone_claims_the_birth_state() {
    // `PERRY_FACTS_SABOTAGE=instlit` (and `PERRY_FACTS_INSTANCE_NODES=0`)
    // read the shared node alone, whose template token no push reaches:
    // the wrong claim check mode must catch.
    let pf = facts_with(SRC, off());
    let b = local(&pf, SRC, "grown", "other");
    let Rep::Arr(r) = b.rep else {
        panic!("`other` is an Array: {:?}", b.rep)
    };
    assert!(r.fixed && r.im, "{r:?}");
    assert_eq!(b.const_length, Some(3));
    let n = local(&pf, SRC, "grown", "n");
    assert_eq!(n.int_range, Some((3, 3)), "{n:?}");
}

#[test]
fn the_probe_names_the_locals_whose_copies_change_the_facts() {
    let pf = facts_with(SRC, probed());
    let lines: Vec<&String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# inst_nodes "))
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("shared=arr<f64,3,dense,fixed,im>")
                && l.contains("shared_len=3")
                && l.contains(" rep=arr<f64,3,dense>")
                && l.contains(" len=none")),
        "{lines:#?}"
    );
    // The control prints no line with the switch off.
    let pf = facts_with(
        SRC,
        FactsConfig {
            diagnostics: true,
            ..off()
        },
    );
    assert!(
        !pf.rep_probe.iter().any(|l| l.starts_with("# inst_nodes ")),
        "{:#?}",
        pf.rep_probe
    );
}

#[test]
fn a_guard_that_no_value_of_an_instance_passes_adds_nothing() {
    // `f(-7)` and `f(200)` never enter the guarded block: their copies of
    // `a` hold no Number (an empty interval beside the prims). The binding
    // keeps the interval of the instances that do.
    let src = "function f(x: number): number {\n\
           const i = x | 0;\n\
           let s = 0;\n\
           if (i >= 0 && i < 128) { const a = i; s += a; }\n\
           return s;\n\
         }\n\
         console.log(f(3), f(-7), f(200), f(70));";
    let pf = facts_with(src, FactsConfig::default());
    assert_eq!(local_in(&pf, src, "a").int_range, Some((0, 127)));
}
