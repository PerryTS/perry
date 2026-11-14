//! Literal atoms (`literal_strings::atoms`): a binding or a field whose
//! every String is a literal the string pool atomises.

use super::*;
use crate::FactsConfig;

/// `FieldFacts::literal_atoms` of `class.key`.
fn field_fact(src: &str, class: &str, key: &str, cfg: FactsConfig) -> bool {
    let pf = facts_with(src, cfg);
    field(tmpl(&pf, class), key).literal_atoms
}

/// `BindingFacts::literal_atoms` of parameter `i` of function `func`.
fn param_fact(src: &str, func: &str, i: usize, cfg: FactsConfig) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func}"))
        .params[i]
        .id;
    let pf = facts_with(src, cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.locals
        .iter()
        .find(|(l, _)| *l == id)
        .unwrap_or_else(|| panic!("local {id} of {func}"))
        .1
        .literal_atoms
}

const SIDE: &str = r#"
type Color = "white" | "black";
function opposite(c: Color): Color { return c === "white" ? "black" : "white"; }
function sign(c: Color): number { return c === "white" ? 1 : -1; }
function blank(c: string): number { return c === "" ? 0 : 1; }
function long(c: string): number { return c.length; }
function built(c: string): number { return c === "white" ? 1 : 0; }
class Game {
  side: Color = "white";
  note: string = "start";
  empty: string = "x";
  make(): void { this.side = opposite(this.side); }
  remark(k: number): void { this.note = "move " + k; }
  clear(): void { this.empty = ""; }
}
const g = new Game();
g.make(); g.remark(3); g.clear();
console.log(g.side, g.note, g.empty, sign(g.side), blank(""), blank("a"),
  long("0123456789012345678901234567890123456789012345678901234567890123456789"),
  built("wh" + "ite".slice(0)), built("black"));
"#;

#[test]
fn a_set_of_short_literals_holds_literal_atoms() {
    assert!(field_fact(SIDE, "Game", "side", FactsConfig::default()));
    assert!(param_fact(SIDE, "sign", 0, FactsConfig::default()));
    assert!(param_fact(SIDE, "opposite", 0, FactsConfig::default()));
}

#[test]
fn an_empty_or_a_long_literal_is_no_atom() {
    // `""` and a literal past 64 bytes are not atomised by the pool.
    assert!(!param_fact(SIDE, "blank", 0, FactsConfig::default()));
    assert!(!param_fact(SIDE, "long", 0, FactsConfig::default()));
    assert!(!field_fact(SIDE, "Game", "empty", FactsConfig::default()));
}

#[test]
fn a_built_string_is_no_atom() {
    assert!(!param_fact(SIDE, "built", 0, FactsConfig::default()));
    assert!(!field_fact(SIDE, "Game", "note", FactsConfig::default()));
}

#[test]
fn the_negative_control_claims_a_built_string() {
    let cfg = FactsConfig {
        sabotage: Some("litstr".into()),
        ..Default::default()
    };
    assert!(param_fact(SIDE, "built", 0, cfg));
}

#[test]
fn the_probe_lists_the_atom_holders() {
    let module = perry_parser::parse_typescript(SIDE, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    let d = crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text();
    let field = |k: &str| {
        d.lines()
            .any(|l| l.starts_with("# lit_atoms field ") && l.ends_with(k))
    };
    assert!(field("Game.side"), "{d}");
    assert!(!field("Game.note"), "{d}");
    assert!(
        d.lines()
            .any(|l| l.starts_with("# lit_atoms module=0 local=")),
        "{d}"
    );
}
