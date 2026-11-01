//! `this.played.push(x)`: the HIR folds a push on a receiver it proved an
//! array into the native `array.push_single` call. Its argument is the
//! array's new element, as for `Expr::ArrayPush`, not an argument handed to
//! unknown native code: what a later `pop()` reads keeps its class.

use super::*;

const ENG: &str = r#"
class U { a = 1; b = 2; }
class Eng {
  played: U[] = [];
  push(): void { this.played.push(new U()); }
  pop(): number { const u = this.played.pop(); return u ? u.a + u.b : 0; }
}
const e = new Eng();
e.push();
console.log(e.pop());
"#;

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

#[test]
fn a_field_array_push_stores_its_element() {
    for pf in [facts(ENG), units(ENG)] {
        for k in ["a", "b"] {
            assert_eq!(verdicts(&pf, k), ["T0Mono"], "{k}\n{}", pf.dump_text());
        }
    }
}

/// What the pushed value is still reaches the reader: a value of unknown
/// code pushed beside the instance refuses the reads.
#[test]
fn a_field_array_push_of_an_unknown_value_refuses() {
    let src = ENG.replace(
        "push(): void { this.played.push(new U()); }",
        "push(): void { this.played.push(new U()); this.played.push(JSON.parse(\"{}\")); }",
    );
    for pf in [facts(&src), units(&src)] {
        for k in ["a", "b"] {
            assert!(verdicts(&pf, k).iter().all(|v| !v.starts_with("T0")), "{k}\n{}", pf.dump_text());
        }
    }
}
