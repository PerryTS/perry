//! Booleans (`booleans`): a binding or a field whose every value is `true`
//! or `false`, read off the solved value lattice.

use super::*;
use crate::FactsConfig;

/// `FieldFacts::booleans` of `class.key`.
fn field_fact(src: &str, class: &str, key: &str, cfg: FactsConfig) -> bool {
    let pf = facts_with(src, cfg);
    field(tmpl(&pf, class), key).booleans
}

/// `BindingFacts::booleans` of parameter `i` of function `func`.
fn param_fact(src: &str, func: &str, i: usize, cfg: FactsConfig) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let id = hir
        .functions
        .iter()
        .chain(
            hir.classes
                .iter()
                .flat_map(|c| c.methods.iter().chain(c.static_methods.iter())),
        )
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
        .booleans
}

const GAME: &str = r#"
class Game {
  done = false;
  note: boolean | undefined = undefined;
  mixed: boolean | number = false;
  n = 0;
  make(tracked: boolean, loose: boolean, count: number): void {
    if (tracked) this.n += count;
    if (loose) this.n++;
    this.done = tracked || this.n > 3;
  }
  flag(v: boolean): void { this.note = v; }
  mix(v: number): void { this.mixed = v; }
}
function opt(x?: boolean): number { return x ? 1 : 0; }
function any(x: any): number { return x ? 1 : 0; }
const g = new Game();
g.make(true, false, 2); g.make(false, 0 as any, 1); g.flag(true); g.mix(3);
console.log(g.done, g.note, g.mixed, g.n, opt(), opt(true), any(1), any(true));
"#;

#[test]
fn a_parameter_passed_only_booleans_holds_booleans() {
    assert!(param_fact(GAME, "make", 0, FactsConfig::default()));
    assert!(param_fact(GAME, "flag", 0, FactsConfig::default()));
}

#[test]
fn an_annotation_proves_nothing() {
    // `loose: boolean` is passed a Number, `x?: boolean` may be missing,
    // `x: any` is passed a Number and a Boolean.
    assert!(!param_fact(GAME, "make", 1, FactsConfig::default()));
    assert!(!param_fact(GAME, "opt", 0, FactsConfig::default()));
    assert!(!param_fact(GAME, "any", 0, FactsConfig::default()));
    // A Number parameter holds no Boolean at all.
    assert!(!param_fact(GAME, "make", 2, FactsConfig::default()));
}

#[test]
fn a_field_written_only_booleans_holds_booleans() {
    assert!(field_fact(GAME, "Game", "done", FactsConfig::default()));
}

#[test]
fn a_field_that_may_hold_anything_else_does_not() {
    assert!(!field_fact(GAME, "Game", "note", FactsConfig::default()));
    assert!(!field_fact(GAME, "Game", "mixed", FactsConfig::default()));
    assert!(!field_fact(GAME, "Game", "n", FactsConfig::default()));
}

#[test]
fn the_negative_control_claims_a_mixed_binding() {
    let cfg = || FactsConfig {
        sabotage: Some("bools".into()),
        ..Default::default()
    };
    assert!(param_fact(GAME, "make", 1, cfg()));
    assert!(param_fact(GAME, "opt", 0, cfg()));
    assert!(field_fact(GAME, "Game", "mixed", cfg()));
    // No Boolean at all: nothing to claim.
    assert!(!param_fact(GAME, "make", 2, cfg()));
}

#[test]
fn the_probe_lists_locals_and_fields() {
    let module = perry_parser::parse_typescript(GAME, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    let d = crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text();
    assert!(
        d.lines()
            .any(|l| l.starts_with("# booleans field ") && l.ends_with("Game.done")),
        "{d}"
    );
    assert!(
        d.lines()
            .any(|l| l.starts_with("# booleans module=0 local=")),
        "{d}"
    );
}
