//! b1: a method call proven to be a field element read
//! (`field_read_calls`), and the controls that keep a call a call.
use super::*;

fn calls(src: &str) -> Vec<crate::FieldReadCall> {
    let pf = facts(src);
    pf.bindings
        .iter()
        .flat_map(|b| b.field_read_calls.iter().map(|(_, c)| c.clone()))
        .collect()
}

/// The chess shape: a private board, `at` read by every hot loop.
const BOARD: &str = r#"
class Board {
  private board: number[] = new Array<number>(128).fill(0);
  private at(square: number): number { return this.board[square] ?? 0; }
  set(square: number, v: number): void { if (square >= 0 && square < 128) this.board[square] = v; }
  count(n: number): number {
    let c = 0;
    for (let i = 0; i < n; i++) if (this.at(i & 127) === 1) c++;
    return c;
  }
}
const b = new Board();
b.set(3, 1);
console.log(b.count(1000));
"#;

#[test]
fn a_call_of_a_field_read_method_is_the_read() {
    let c = calls(BOARD);
    assert_eq!(c.len(), 1, "{:?}\n{}", c, {
        let module = perry_parser::parse_typescript(BOARD, "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
        format!(
            "{:?}",
            hir.classes[0]
                .methods
                .iter()
                .find(|f| f.name == "at")
                .map(|f| &f.body)
        )
    });
    assert_eq!(c[0].field, "board");
    assert_eq!(c[0].index_arg, 0);
    assert_eq!(
        c[0].default,
        Some(crate::FieldReadDefault::Number(0f64.to_bits()))
    );
}

#[test]
fn two_classes_inheriting_the_same_read_keep_the_fact() {
    // K8: a T0Set over two classes whose `board` sits in different slots.
    let src = r#"
class Base { pad = 1; board: number[] = [1, 2, 3, 4]; at(i: number): number { return this.board[i] ?? 0; } }
class Other extends Base { extra = 2; }
function pick(n: number): Base { return n > 2 ? new Other() : new Base(); }
const o = pick(process.argv.length);
let s = 0;
for (let i = 0; i < 4; i++) s += o.at(i & 3);
console.log(s);
"#;
    assert_eq!(calls(src).len(), 1);
}

fn assert_no_fact(name: &str, src: &str) {
    let pf = facts(src);
    let found: Vec<_> = pf
        .bindings
        .iter()
        .flat_map(|b| b.field_read_calls.iter())
        .collect();
    let board: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "board" || s.key == "at")
        .map(|s| {
            format!(
                "{}@{} call={} {:?} recv={:?}",
                s.key, s.byte_offset, s.is_call, s.verdict, s.receivers
            )
        })
        .collect();
    assert!(
        found.is_empty(),
        "{name}: the call must stay a call: {found:?}\n{}",
        board.join("\n")
    );
}

#[test]
fn controls_keep_the_call() {
    let base = |at: &str, extra: &str, call: &str| {
        format!(
            "class Board {{ board: number[] = new Array<number>(8).fill(0);\n{at}\n }}\n{extra}\nconst b = new Board();\nlet s = 0; for (let i = 0; i < 8; i++) {{ {call} }}\nconsole.log(s);\n"
        )
    };
    let at = "at(i: number): number { return this.board[i] ?? 0; }";
    let call = "if (b.at(i & 7) === 0) s++;";
    // K1: a subclass overrides `at`.
    assert_no_fact(
        "K1",
        &base(
            at,
            "class Sub extends Board { at(i: number): number { return 5; } }\nconst sb: Board = process.argv.length > 9 ? new Sub() : new Board(); sb.at(0);",
            "if ((process.argv.length > 9 ? new Sub() : b).at(i & 7) === 0) s++;",
        ),
    );
    // K2: a reassigned method.
    assert_no_fact(
        "K2",
        &base(
            at,
            "(Board.prototype as any).at = function () { return 9; };",
            call,
        ),
    );
    // K3: an accessor field.
    assert_no_fact(
        "K3",
        &base(
            "get board2(): number[] { return [1]; }\nat(i: number): number { return this.board2[i] ?? 0; }",
            "",
            call,
        ),
    );
    // K4: a second statement.
    assert_no_fact(
        "K4",
        &base(
            "n = 0; at(i: number): number { this.n++; return this.board[i] ?? 0; }",
            "",
            call,
        ),
    );
    // K5: an argument count mismatch.
    assert_no_fact(
        "K5",
        &base(at, "", "if ((b as any).at(i & 7, 1) === 0) s++;"),
    );
    // K6: an index key written on Object.prototype.
    assert_no_fact("K6", &base(at, "(Object.prototype as any)[5] = 3;", call));
    // K9: an index that is not proven an integer Number.
    assert_no_fact(
        "K9",
        &base(
            at,
            "const keys: any[] = [{ toString() { return '1'; } }];",
            "if (b.at(keys[0]) === 0) s++;",
        ),
    );
    // K11: F with another `this`.
    assert_no_fact(
        "K11",
        &base(at, "const g = b.at; g.call({ board: 'abc' }, 1);", call),
    );
    // K13: the field may hold undefined.
    assert_no_fact(
        "K13",
        &base(
            at,
            "if (process.argv.length > 9) (b as any).board = undefined;",
            call,
        ),
    );
}

/// `PERRY_FACTS_SABOTAGE=fieldreadcall` claims the fact at a call whose
/// method is overridden (K1's shape): the negative control.
#[test]
fn the_sabotage_claims_an_overridden_call() {
    let src = r#"
class Board { board: number[] = [1, 2, 3, 4]; at(i: number): number { return this.board[i] ?? 0; } }
class Sub extends Board { at(i: number): number { return 5; } }
const b: Board = process.argv.length > 9 ? new Sub() : new Board();
let s = 0;
for (let i = 0; i < 4; i++) s += b.at(i & 3);
console.log(s);
"#;
    let pf = facts_with(
        src,
        FactsConfig {
            sabotage: Some("fieldreadcall".into()),
            ..Default::default()
        },
    );
    assert!(pf.bindings.iter().any(|b| !b.field_read_calls.is_empty()));
}
