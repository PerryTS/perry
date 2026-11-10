//! `Rep::Arr` (DESIGN-p4l1 §2.1-2.6, PR-B): plain Array births, the fused
//! fill, the representation of a binding or field that holds only plain
//! program Arrays, and the one join of `Rep`.

use super::*;
use crate::bindings::{ArrRep, ElemRep, Enc, Rep};
use crate::model::TmplKind;
use crate::{ElementOp, FactsConfig};
use std::collections::HashMap;

/// `name -> rep` of every `let`/`const` of `t.ts` under `cfg`.
fn reps_with(src: &str, cfg: FactsConfig) -> HashMap<String, Rep> {
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
            out.entry(name.clone()).or_insert(b.rep);
        }
    }
    out
}

fn rep(src: &str, name: &str) -> Rep {
    rep_with(src, name, FactsConfig::default())
}

fn rep_with(src: &str, name: &str, cfg: FactsConfig) -> Rep {
    let r = reps_with(src, cfg);
    *r.get(name)
        .unwrap_or_else(|| panic!("no binding {name} in {r:?}"))
}

fn arr(src: &str, name: &str) -> ArrRep {
    match rep(src, name) {
        Rep::Arr(a) => a,
        other => panic!("{name} is {other:?}, not Arr"),
    }
}

const SAMPLES: [Rep; 7] = [
    Rep::Boxed,
    Rep::F64,
    Rep::I32,
    Rep::Arr(ArrRep {
        elem: ElemRep::I32,
        floor: 8,
        dense: true,
        fixed: true,
    }),
    Rep::Arr(ArrRep {
        elem: ElemRep::F64,
        floor: 3,
        dense: false,
        fixed: true,
    }),
    Rep::Arr(ArrRep {
        elem: ElemRep::Boxed,
        floor: 0,
        dense: false,
        fixed: false,
    }),
    Rep::Arr(ArrRep {
        elem: ElemRep::I32,
        floor: 128,
        dense: true,
        fixed: false,
    }),
];

#[test]
fn the_join_is_a_lattice_join() {
    for a in SAMPLES {
        assert_eq!(Rep::join(a, a), a, "idempotent at {a:?}");
        for b in SAMPLES {
            assert_eq!(Rep::join(a, b), Rep::join(b, a), "commutes at {a:?} {b:?}");
            for c in SAMPLES {
                assert_eq!(
                    Rep::join(Rep::join(a, b), c),
                    Rep::join(a, Rep::join(b, c)),
                    "associates at {a:?} {b:?} {c:?}"
                );
            }
        }
    }
}

#[test]
fn a_number_and_an_array_join_to_boxed() {
    let a = SAMPLES[3];
    assert_eq!(Rep::join(Rep::F64, a), Rep::Boxed);
    assert_eq!(Rep::join(Rep::I32, a), Rep::Boxed);
    assert_eq!(Rep::join(Rep::Boxed, a), Rep::Boxed);
    assert_eq!(Rep::join(Rep::I32, Rep::F64), Rep::F64);
}

#[test]
fn two_arrays_join_to_what_both_have() {
    let j = Rep::join(SAMPLES[3], SAMPLES[6]);
    assert_eq!(
        j,
        Rep::Arr(ArrRep {
            elem: ElemRep::I32,
            floor: 8,
            dense: true,
            fixed: false,
        })
    );
    let j = Rep::join(SAMPLES[3], SAMPLES[4]);
    assert_eq!(
        j,
        Rep::Arr(ArrRep {
            elem: ElemRep::F64,
            floor: 3,
            dense: false,
            fixed: true,
        })
    );
}

#[test]
fn an_array_slot_is_encoded_boxed() {
    for r in SAMPLES {
        let want = match r {
            Rep::F64 => Enc::F64,
            Rep::I32 => Enc::I32,
            Rep::Boxed | Rep::Arr(_) => Enc::Boxed,
        };
        assert_eq!(r.encoding(), want);
    }
    assert_eq!(SAMPLES[3].name(), "arr<i32,8,dense,fixed>");
    assert_eq!(SAMPLES[5].name(), "arr<boxed,0>");
}

const READ: &str =
    "\nlet s = 0;\nfor (let i = 0; i < 4; i++) { const k = i; s += K[k] ?? 0; }\nconsole.log(s);";

#[test]
fn a_const_literal_is_a_fixed_dense_array() {
    let src = format!("const K = [1, 2, 3, 4, 5, 6, 7, 8];{READ}");
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (8, true, true));
    assert_ne!(a.elem, ElemRep::Boxed);
}

#[test]
fn new_array_is_plain_but_not_dense() {
    let src = format!("const K: number[] = new Array(8);\nK[0] = 1;{READ}");
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (8, false, true));
}

#[test]
fn the_fused_fill_birth_is_dense() {
    let src = format!("const K = new Array(8).fill(0);{READ}");
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (8, true, true));
    assert_ne!(a.elem, ElemRep::Boxed);
    // Two arguments are no fused fill: index 0 stays a hole.
    let src = format!("const K = new Array(8).fill(0, 1);{READ}");
    assert!(!arr(&src, "K").dense);
    // Nor is a fill on a binding.
    let src = format!("const K = new Array(8);\nK.fill(0);{READ}");
    assert!(!arr(&src, "K").dense);
}

#[test]
fn the_fused_fill_is_the_native_member() {
    // §2.6: the lock makes the call the native `fill`; the site verdict
    // must agree (a defence, not the soundness argument).
    let src = format!("const K = new Array(8).fill(0);{READ}");
    let pf = facts(&src);
    let fills: Vec<_> = pf.sites.iter().filter(|s| s.key == "fill").collect();
    assert!(!fills.is_empty(), "{}", pf.dump_text());
    assert!(
        fills
            .iter()
            .all(|s| matches!(s.verdict, Verdict::NativeMember)),
        "{fills:?}"
    );
}

#[test]
fn an_appended_array_has_no_floor() {
    let src = format!("const K: number[] = [];\nK.push(1);{READ}");
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (0, false, false));
}

#[test]
fn index_growth_keeps_the_floor_but_not_fixed() {
    let src = format!(
        "const K = [1, 2, 3, 4];\n\
         function put(k: number): void {{ if (k >= 0 && k < 10) K[k] = 1; }}\n\
         put(9);{READ}"
    );
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (4, true, false));
}

#[test]
fn a_stored_undefined_makes_the_elements_boxed() {
    let src = format!("const K = [1, undefined, 3, 4];{READ}");
    assert_eq!(arr(&src, "K").elem, ElemRep::Boxed);
}

#[test]
fn a_json_array_is_not_plain() {
    let src = "const J = JSON.parse(\"[1, 2, 3]\");\nconsole.log(J[1]);";
    assert_eq!(rep(src, "J"), Rep::Boxed);
}

#[test]
fn a_runtime_born_array_is_not_plain() {
    let src = "const o = { a: 1, b: 2 };\nconst K = Object.keys(o);\nconsole.log(K[0]);";
    assert_eq!(rep(src, "K"), Rep::Boxed);
    let src = "const L = [3, 1, 2];\nconst K = L.slice();\nconsole.log(K[0]);";
    assert_eq!(rep(src, "K"), Rep::Boxed);
}

#[test]
fn an_escaped_or_reshaped_array_is_boxed() {
    let src = format!("const K = [1, 2, 3, 4];\n(globalThis as any).hostTake(K);{READ}");
    assert_eq!(rep(&src, "K"), Rep::Boxed);
    let src = format!("const K: any = [1, 2, 3, 4];\nK.tag = 1;{READ}");
    assert_eq!(rep(&src, "K"), Rep::Boxed);
    let src = format!("const K = [1, 2, 3, 4];\ndelete K[1];{READ}");
    assert!(!matches!(rep(&src, "K"), Rep::Arr(a) if a.dense || a.floor > 0));
}

/// REVIEW-p4l1 R1: one call site whose callee holds both `Array` and
/// `JSON.parse` births one template per kind; the JSON one is not plain.
#[test]
fn one_site_of_two_builtins_keeps_the_json_array_apart() {
    let src = r#"const f: any = Math.random() > 0.5 ? Array : JSON.parse;
const x = f("[1, 2]");
const y = x[1];
console.log(y);"#;
    let pf = facts(src);
    let offset = src.find("x[1]").expect("site") as u32 + 1;
    let site = pf
        .element(0, offset, ElementOp::Read)
        .unwrap_or_else(|| panic!("read site: {:#?}", pf.elements));
    let kinds: Vec<_> = site
        .receivers
        .iter()
        .filter_map(|v| match &v.template_kind {
            Some(TmplKind::Arr { plain, .. }) => Some(*plain),
            _ => None,
        })
        .collect();
    assert!(kinds.contains(&true), "the Array birth is plain: {site:?}");
    assert!(kinds.contains(&false), "the JSON array is not: {site:?}");
    assert_eq!(rep(src, "x"), Rep::Boxed);
}

#[test]
fn the_switch_off_gives_no_array_rep() {
    let off = || FactsConfig {
        no_array_reps: true,
        ..Default::default()
    };
    let src = format!("const K = new Array(8).fill(0);{READ}");
    assert_eq!(rep_with(&src, "K", off()), Rep::Boxed);
    let src = format!("const K = [1, 2, 3, 4];{READ}");
    assert_eq!(rep_with(&src, "K", off()), Rep::Boxed);
}

#[test]
fn a_definite_field_takes_the_join_of_its_writes() {
    let src = r#"class B {
  board: number[] = new Array(16).fill(0);
  at(i: number): number { return this.board[i] ?? 0; }
}
const b = new B();
console.log(b.at(3));"#;
    let pf = facts(src);
    let f = field(tmpl(&pf, "B"), "board");
    assert!(
        matches!(f.rep, Rep::Arr(a) if a.floor == 16 && a.dense && a.fixed),
        "{:?}",
        f.rep
    );
    // A reassignment with a shorter array joins: the least floor.
    let src = r#"class B {
  board: number[] = new Array(16).fill(0);
  reset(): void { this.board = new Array(8).fill(1); }
  at(i: number): number { return this.board[i] ?? 0; }
}
const b = new B();
b.reset();
console.log(b.at(3));"#;
    let pf = facts(src);
    let f = field(tmpl(&pf, "B"), "board");
    assert!(
        matches!(f.rep, Rep::Arr(a) if a.floor == 8 && a.dense),
        "{:?}",
        f.rep
    );
}

#[test]
fn a_field_that_may_also_hold_null_or_be_deleted_is_boxed() {
    let src = r#"class B {
  board: number[] | null = new Array(16).fill(0);
  drop(): void { this.board = null; }
}
const b = new B();
b.drop();
console.log(b.board);"#;
    let pf = facts(src);
    assert_eq!(field(tmpl(&pf, "B"), "board").rep, Rep::Boxed);
    let src = r#"class B {
  board: number[] = new Array(16).fill(0);
}
const b: any = new B();
delete b.board;
console.log(b.board);"#;
    let pf = facts(src);
    assert_eq!(field(tmpl(&pf, "B"), "board").rep, Rep::Boxed);
}

/// DESIGN-p4l1 §4.4: an `Arr` parameter is a Boxed slot. A function whose
/// other parameter is a Number stays typed, and its call sites stay typed
/// (P2's `admits` asks the encoding).
#[test]
fn an_array_parameter_keeps_its_function_s_typed_sites() {
    let src = r#"function f(a: number[], x: number): number { return (a[0] ?? 0) + x; }
const K = [1, 2, 3, 4];
let s = 0;
for (let i = 0; i < 4; i++) { s += f(K, i); }
console.log(s);"#;
    let on = facts(src);
    let off = facts_with(
        src,
        FactsConfig {
            no_array_reps: true,
            ..Default::default()
        },
    );
    let typed = |pf: &ProgramFacts| {
        pf.bindings_for("t.ts")
            .expect("bindings")
            .typed_calls
            .clone()
    };
    assert!(!typed(&off).is_empty(), "control: the site is typed");
    assert_eq!(typed(&on), typed(&off));
    let sig = |pf: &ProgramFacts| {
        let mb = pf.bindings_for("t.ts").expect("bindings");
        assert_eq!(mb.signatures.len(), 1, "{:?}", mb.signatures);
        mb.signatures[0].1.clone()
    };
    assert!(matches!(sig(&on).params[0], Rep::Arr(_)));
    assert_eq!(sig(&off).params[0], Rep::Boxed);
    assert_eq!(sig(&on).params[1], sig(&off).params[1]);
}

/// REVIEW-p4l1 R2: a body packet's birth carries both bits of its Array
/// template.
#[test]
fn a_packet_birth_names_a_plain_array() {
    use crate::summary::packet::BirthKind;
    let src = "function make() { return [1, 2, 3]; }\nconsole.log(make()[0]);";
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let id = hir.functions.iter().find(|f| f.name == "make").unwrap().id;
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.m.array_reps = true;
    g.register_all();
    let f = g.mods[0].funcs[&id];
    g.walk_all();
    let packet = &g.m.funcs[f as usize].body_packet;
    assert!(
        packet.births.iter().any(|b| b.kind
            == Some(BirthKind::Array {
                hole_free: true,
                plain: true,
            })),
        "{:?}",
        packet.births
    );
}

/// PR-C2: the key interval of the read whose object starts at `at`
/// (`ModuleBindings::element_keys`; SWC offsets are 1-based).
fn key_at(src: &str, at: &str, cfg: FactsConfig) -> Option<(i64, i64)> {
    let offset = src.find(at).expect("read in source") as u32 + 1;
    let pf = facts_with(src, cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.element_keys
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, k)| *k)
}

const TABLE: &str =
    "const K = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];\n";

#[test]
fn an_element_read_exports_its_key_interval() {
    // A masked key is an integer in the mask's interval.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i & 7] ?? 0; }}\n\
         console.log(f(3) + f(12));"
    );
    assert_eq!(
        key_at(&src, "K[i & 7]", FactsConfig::default()),
        Some((0, 7))
    );
    // A key that may be a fraction is no integer interval.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i / 2] ?? 0; }}\n\
         console.log(f(3));"
    );
    assert_eq!(key_at(&src, "K[i / 2]", FactsConfig::default()), None);
    // A string key is no Number.
    let src = format!(
        "{TABLE}export function f(k: string): unknown {{ return (K as any)[k]; }}\n\
         console.log(f(\"length\"));"
    );
    assert_eq!(key_at(&src, "(K as any)[k]", FactsConfig::default()), None);
}

#[test]
fn a_key_interval_is_the_hull_over_every_caller() {
    // The parameter joins both callers' arguments, and may be negative.
    let src = format!(
        "{TABLE}function at(i: number): number {{ return K[i] ?? 0; }}\n\
         console.log(at(2) + at(-1) + at(19));"
    );
    assert_eq!(key_at(&src, "K[i]", FactsConfig::default()), Some((-1, 19)));
}

#[test]
fn the_switch_off_exports_no_key_interval() {
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i & 7] ?? 0; }}\n\
         console.log(f(3));"
    );
    let off = FactsConfig {
        no_array_reps: true,
        ..Default::default()
    };
    assert_eq!(key_at(&src, "K[i & 7]", off), None);
}

#[test]
fn a_shift_takes_the_floor_away_unless_sabotaged() {
    // `shift` moves the front of the storage: the length is no longer
    // defined, so the rep keeps no floor (the tier then addresses the slot
    // through the header). The negative control `arrfront` keeps it.
    let src = format!("{TABLE}K.shift();{READ}");
    let a = arr(&src, "K");
    assert_eq!((a.floor, a.dense, a.fixed), (0, false, false));
    let sabotaged = FactsConfig {
        sabotage: Some("arrfront".into()),
        ..Default::default()
    };
    match rep_with(&src, "K", sabotaged) {
        Rep::Arr(a) => assert_eq!((a.floor, a.fixed), (20, true)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_redefined_or_deleted_element_gives_no_array_rep() {
    // The tier reads the slot itself: an element descriptor or a deleted
    // element must leave no `Arr` (a hard marker, `arr_rep`).
    for stmt in [
        "Object.defineProperty(K, 0, { get() { return 7; } });",
        "delete (K as any)[1];",
        "Object.freeze(K);",
    ] {
        let src = format!("{TABLE}{stmt}{READ}");
        assert_eq!(rep(&src, "K"), Rep::Boxed, "{stmt}");
    }
}

// ---- PR-C3 (DESIGN-p4l1 §3, §2.8): the field's rep and the in-floor call ----

/// The chess shape: a board born dense with 128 elements, stored into at
/// keys that leave it, and read behind `at` by calls with their own keys.
const FIELD_BOARD: &str = r#"
class Chess {
  board: number[] = new Array(128).fill(0);
  ep: number = -1;
  at(square: number): number { return this.board[square] ?? 0; }
  set(square: number, piece: number): void { this.board[square] = piece; }
  ranks(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f);
    return s;
  }
  masked(move: number): number { return this.at(move & 127); }
  capture(move: number): number { return this.at((move & 127) - 16); }
  past(): number { return this.at(this.ep + 16); }
}
const c = new Chess();
for (let i = -16; i < 144; i++) c.set(i, i & 7);
c.ep = process.argv.length > 100 ? 40 : 20;
console.log(c.ranks(), c.masked(process.argv.length), c.capture(process.argv.length), c.past());
"#;

/// The field read calls of `src` by the source text of their call.
fn b1(src: &str, cfg: FactsConfig) -> Vec<(String, crate::FieldReadCall)> {
    let pf = facts_with(src, cfg);
    pf.bindings
        .iter()
        .flat_map(|b| b.field_read_calls.iter())
        .map(|(offset, c)| {
            let rest = &src[*offset as usize..];
            let end = rest.find(')').map_or(rest.len(), |e| e + 1);
            (rest[..end].to_string(), c.clone())
        })
        .collect()
}

#[test]
fn a_stored_into_board_field_keeps_its_dense_floor() {
    let pf = facts(FIELD_BOARD);
    let f = field(tmpl(&pf, "Chess"), "board");
    // Growable: a store key reaches past the birth length.
    assert!(
        matches!(f.rep, Rep::Arr(a) if a.floor == 128 && a.dense && !a.fixed && a.elem != ElemRep::Boxed),
        "{:?}",
        f.rep
    );
}

#[test]
fn a_field_read_call_is_in_the_floor_by_its_own_key() {
    let calls = b1(FIELD_BOARD, FactsConfig::default());
    let of = |text: &str| {
        calls
            .iter()
            .find(|(t, _)| t.starts_with(text))
            .unwrap_or_else(|| panic!("no call {text}: {calls:?}"))
            .1
            .clone()
    };
    // Every call reads from the same arrays.
    for (_, c) in &calls {
        assert!(
            matches!(c.array, Some(a) if a.floor == 128 && a.dense),
            "{c:?}"
        );
    }
    assert!(of("this.at(r * 16 + f").in_floor);
    assert!(of("this.at(move & 127)").in_floor);
    // `[-16, 111]` and `[36, 56]`... only the first leaves the floor below.
    assert!(!of("this.at((move & 127) - 16").in_floor);
    assert!(of("this.at(this.ep + 16").in_floor);
}

#[test]
fn a_key_past_the_floor_is_not_in_it_unless_sabotaged() {
    let src = FIELD_BOARD.replace("this.at(this.ep + 16)", "this.at(this.ep + 100)");
    let past = |cfg| {
        b1(&src, cfg)
            .into_iter()
            .find(|(t, _)| t.starts_with("this.at(this.ep + 100"))
            .expect("the call is a field read")
            .1
            .in_floor
    };
    assert!(!past(FactsConfig::default()));
    assert!(past(FactsConfig {
        sabotage: Some("arrb1".into()),
        ..Default::default()
    }));
}

#[test]
fn the_fields_switch_off_gives_no_field_rep_and_no_floor() {
    let off = || FactsConfig {
        no_array_fields: true,
        ..Default::default()
    };
    let pf = facts_with(FIELD_BOARD, off());
    assert_eq!(field(tmpl(&pf, "Chess"), "board").rep, Rep::Boxed);
    let calls = b1(FIELD_BOARD, off());
    assert!(!calls.is_empty());
    for (_, c) in &calls {
        assert!(c.array.is_none() && !c.in_floor, "{c:?}");
    }
    // A local keeps its rep: the switch is the field's.
    let pf = facts_with("const K = [1, 2, 3];\nconsole.log(K[1]);", off());
    assert!(pf
        .bindings
        .iter()
        .flat_map(|b| b.locals.iter())
        .any(|(_, b)| matches!(b.rep, Rep::Arr(_))));
}

#[test]
fn a_call_into_a_holey_or_undefined_holding_field_is_not_in_the_floor() {
    // No fused fill: holes below the length.
    let holey = FIELD_BOARD.replace("new Array(128).fill(0)", "new Array(128)");
    for (_, c) in b1(&holey, FactsConfig::default()) {
        assert!(!c.in_floor, "{c:?}");
    }
    // A `delete` of an element makes the array hard: no rep at all.
    let deleted = FIELD_BOARD.replace("c.ep = ", "delete c.board[3];\nc.ep = ");
    for (_, c) in b1(&deleted, FactsConfig::default()) {
        assert!(c.array.is_none() && !c.in_floor, "{c:?}");
    }
    // A length store takes the floor away.
    let cut = FIELD_BOARD.replace("c.ep = ", "c.board.length = 4;\nc.ep = ");
    for (_, c) in b1(&cut, FactsConfig::default()) {
        assert!(!c.in_floor, "{c:?}");
    }
    // A stored `undefined` is a present element: the call stays in the
    // floor, and the elements are Boxed (the `??` stays in codegen).
    let undef = FIELD_BOARD.replace("c.ep = ", "(c.board as any)[5] = undefined;\nc.ep = ");
    let calls = b1(&undef, FactsConfig::default());
    assert!(calls.iter().any(|(_, c)| c.in_floor));
    for (_, c) in &calls {
        assert!(
            matches!(c.array, Some(a) if a.elem == ElemRep::Boxed),
            "{c:?}"
        );
    }
}

#[test]
fn the_arrfield_control_ignores_a_null_in_the_field() {
    let src = r#"class B {
  items: number[] | null = null;
  fill(): void { this.items = [1, 2, 3, 4]; }
}
const b = new B();
b.fill();
console.log(b.items);"#;
    assert_eq!(field(tmpl(&facts(src), "B"), "items").rep, Rep::Boxed);
    let pf = facts_with(
        src,
        FactsConfig {
            sabotage: Some("arrfield".into()),
            ..Default::default()
        },
    );
    assert!(matches!(field(tmpl(&pf, "B"), "items").rep, Rep::Arr(_)));
}

#[test]
fn the_dump_prints_each_field_read_call_with_its_key_and_verdict() {
    let pf = facts(FIELD_BOARD);
    let dump = pf.dump_text();
    let lines: Vec<&str> = dump
        .lines()
        .filter(|l| l.starts_with("# field_read_call "))
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("key=[0,127]") && l.ends_with("in_floor=1")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("key=[-16,111]") && l.ends_with("in_floor=0")),
        "{lines:#?}"
    );
    assert!(lines.iter().all(|l| l.contains("rep=arr<")), "{lines:#?}");
}
