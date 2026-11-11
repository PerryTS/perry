//! Literal strings (`literal_strings`): a binding or a field whose every
//! String is a program string literal's value, read off `Node::strs`.

use super::*;
use crate::FactsConfig;

/// `FieldFacts::literal_strings` of `class.key`.
fn field_fact(src: &str, class: &str, key: &str, cfg: FactsConfig) -> bool {
    let pf = facts_with(src, cfg);
    field(tmpl(&pf, class), key).literal_strings
}

/// `BindingFacts::literal_strings` of parameter `i` of function `func`.
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
        .literal_strings
}

const SIDE: &str = r#"
type Color = "white" | "black";
function opposite(c: Color): Color { return c === "white" ? "black" : "white"; }
function sign(c: Color): number { return c === "white" ? 1 : -1; }
class Game {
  side: Color = "white";
  note: string = "start";
  n = 0;
  make(): void { this.side = opposite(this.side); this.n += sign(this.side); }
  load(s: string): void { this.side = s === "w" ? "white" : "black"; }
  copy(o: Game): void { this.side = o.side; }
  remark(k: number): void { this.note = "move " + k; }
}
const g = new Game();
g.make(); g.load("b"); g.copy(new Game()); g.remark(3);
console.log(g.side, g.note, g.n, sign(g.side));
"#;

#[test]
fn a_field_written_only_from_literals_holds_literal_strings() {
    // An initializer, a conditional of literals, another instance's field
    // and a function that returns literals: every writer is a literal.
    assert!(field_fact(SIDE, "Game", "side", FactsConfig::default()));
}

#[test]
fn a_parameter_passed_only_literals_holds_literal_strings() {
    assert!(param_fact(SIDE, "sign", 0, FactsConfig::default()));
    assert!(param_fact(SIDE, "opposite", 0, FactsConfig::default()));
}

#[test]
fn a_concatenation_is_no_literal() {
    assert!(!field_fact(SIDE, "Game", "note", FactsConfig::default()));
    // A Number field holds no String at all: the fact is about a field
    // that holds one.
    assert!(!field_fact(SIDE, "Game", "n", FactsConfig::default()));
}

#[test]
fn a_runtime_string_reaching_the_field_or_parameter_ends_the_fact() {
    // A method result, a JSON value and a template with a substitution.
    let src = r#"
    class Box { tag = "a"; raw = "b"; tpl = "c";
      set(s: string, k: number): void {
        this.tag = s.toUpperCase();
        this.raw = JSON.parse('"x"');
        this.tpl = `v${k}`;
      }
    }
    function show(s: string): number { return s.length; }
    const b = new Box(); b.set("q", 1);
    console.log(b.tag, b.raw, b.tpl, show("lit"), show(String(b.tag.length)));
    "#;
    for key in ["tag", "raw", "tpl"] {
        assert!(
            !field_fact(src, "Box", key, FactsConfig::default()),
            "{key}"
        );
    }
    assert!(!param_fact(src, "show", 0, FactsConfig::default()));
}

#[test]
fn an_escaped_instance_ends_the_field_fact() {
    // The instance reaches code the solver does not see: anything can be
    // stored into its field.
    let src = r#"
    class Box { tag = "a"; }
    const b = new Box();
    (globalThis as any).sink(b);
    console.log(b.tag);
    "#;
    assert!(!field_fact(src, "Box", "tag", FactsConfig::default()));
}

#[test]
fn the_sabotage_claims_every_string_a_literal() {
    let cfg = || FactsConfig {
        sabotage: Some("litstr".to_string()),
        ..Default::default()
    };
    assert!(field_fact(SIDE, "Game", "note", cfg()));
    // Still nothing for a field that holds no String.
    assert!(!field_fact(SIDE, "Game", "n", cfg()));
}

#[test]
fn the_probe_names_the_field_with_its_constants() {
    let pf = facts_with(
        SIDE,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let line = pf
        .rep_probe
        .iter()
        .find(|l| l.starts_with("# lit_strings field ") && l.contains("Game.side "))
        .unwrap_or_else(|| panic!("no side line in {:#?}", pf.rep_probe));
    assert!(
        line.ends_with("set=[\"black\",\"white\"]") || line.ends_with("set=[\"white\",\"black\"]"),
        "{line}"
    );
}

/// The probe lines of `src`.
fn probe(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# lit_strings "))
        .cloned()
        .collect()
}

#[test]
fn two_methods_choosing_among_the_same_literals_keep_the_fact() {
    // A selection of literals in one method and a flip that reads the
    // field in another.
    let src = r#"
    const many = process.argv.length > 100;
    class B {
      side: string = "white";
      load(s: string): void { this.side = s === "w" ? "white" : "black"; }
      flip(): void { this.side = this.side === "white" ? "black" : "white"; }
    }
    const b = new B(); b.load(many ? "w" : "b"); b.flip(); console.log(b.side);
    "#;
    assert!(
        field_fact(src, "B", "side", FactsConfig::default()),
        "{:#?}",
        probe(src)
    );
}

#[test]
fn the_probe_says_what_ends_a_string_fields_fact() {
    let lines = probe(SIDE);
    let note = lines
        .iter()
        .find(|l| l.contains("Game.note "))
        .unwrap_or_else(|| panic!("no note line in {lines:#?}"));
    assert!(note.contains("refused=any_string:n"), "{note}");
}
