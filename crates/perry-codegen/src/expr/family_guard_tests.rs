//! A class born on a family carries the family descriptor's layout word, not
//! the static id of its keys global (SHC3: no compiled object carries that id).
//! The guards that admit a receiver by layout must compare the word the object
//! really carries: the dispatch tower proven-this clone recheck (a class-method call
//! on an interface-typed receiver) and the checked field read's F4 receiver set
//! (a literal family keeps its late keys and names class 0).

use super::class_field_barrier_tests::ir_opts;
use crate::compile_module;
use perry_hir::Module;

fn module(src: &str) -> Module {
    let ast = perry_parser::parse_typescript(src, "main.ts").unwrap();
    perry_hir::lower_module(&ast, "main.ts", "main.ts").unwrap()
}

/// The module's IR with its program layouts and static ids computed the way the
/// driver does.
fn ir(src: &str) -> String {
    let m = module(src);
    let mut opts = ir_opts();
    opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
        [("main.ts", &m)],
        true,
        &Default::default(),
    ));
    let births = crate::module_birth_shapes(&m, opts.clone()).unwrap();
    let ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    opts.static_shape_ids = ids.into_iter().collect();
    String::from_utf8(compile_module(&m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

fn function_body(ir: &str, name_contains: &str) -> String {
    let at = ir
        .lines()
        .position(|l| l.starts_with("define") && l.contains(name_contains))
        .unwrap_or_else(|| panic!("no `define` containing {name_contains:?} in:\n{ir}"));
    ir.lines()
        .skip(at)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

const CH3: &str = r#"
interface Kinged { k(): number }
class G {
  private side: string = "white";
  private wk = -1;
  constructor(s: string) { this.wk = s.length; }
  private king(c: string): number { return c === "white" ? this.wk : 0; }
  k(): number { return this.king(this.side); }
}
function probe(r: Kinged): number { return r.k(); }
console.log(probe(new G("..K..k")));
"#;

const TM1: &str = r#"
const L: any = { basic: { u: 1 }, lambert: { u: 2 }, phong: { u: 3 }, standard: { u: 4 }, toon: { u: 5 } };
L.physical = { u: L.standard.u + 10 };
console.log(L.standard.u, L.physical.u, Object.keys(L).length);
"#;

/// The facts of `TM1` as the solver proves them: `L` may hold anything (checked)
/// but when it is the literal, `standard` is the definite birth field at slot 3.
fn tm1_facts(m: &Module) -> crate::codegen::ProgramFactsView {
    use crate::codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv};
    let class = m
        .classes
        .iter()
        .find(|c| c.fields.iter().any(|f| f.name == "standard"))
        .expect("the outer literal's shape class");
    let local = m
        .init
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let { id, name, .. } if name == "L" => Some(*id),
            _ => None,
        })
        .expect("local L");
    let field = FactsField {
        key: "standard".into(),
        slot: 3,
        readonly: false,
        kind_mask: 0x80,
        ta_stable: false,
        ta_kind: None,
        declared: false,
        reserved: false,
        fns: Vec::new(),
        classes: Vec::new(),
        nullable: false,
    };
    let mut elision = ElisionFacts::default();
    elision.locals.insert(
        local,
        FactsRecv {
            class_ids: vec![class.id],
            nullable: false,
            checked: true,
        },
    );
    elision.classes.insert(
        class.id,
        FactsClass {
            display: class.name.clone(),
            fixed_layout: true,
            fields: vec![field],
        },
    );
    crate::codegen::ProgramFactsView {
        elision: std::sync::Arc::new(elision),
        ..Default::default()
    }
}

/// Compile `m` with the layouts computed the way the driver does, and `facts`.
fn ir_with_facts(m: &Module, facts: crate::codegen::ProgramFactsView) -> String {
    let mut opts = ir_opts();
    opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
        [("main.ts", m)],
        true,
        &Default::default(),
    ));
    opts.program_facts = facts;
    String::from_utf8(compile_module(m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

/// The text of the basic blocks whose label starts with `prefix`.
fn blocks_labelled(ir: &str, prefix: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.ends_with(':') && !line.starts_with(' ') {
            inside = line.starts_with(prefix);
        } else if line == "}" {
            inside = false;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Does `text` compare an i32 against a family descriptor's `layout_id` word?
/// (`getelementptr i8, ptr @perry_layout_class_<n>, i64 12`, loaded, then used
/// as an operand of `icmp eq i32`.)
fn compares_a_family_layout_id(text: &str) -> bool {
    let layout_id_gep =
        |l: &str| l.contains("getelementptr i8, ptr @perry_layout_class_") && l.ends_with("i64 12");
    let mut ptr_regs = Vec::new();
    let mut id_regs = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some((reg, rhs)) = line.split_once(" = ") else {
            continue;
        };
        if layout_id_gep(rhs) {
            ptr_regs.push(reg.to_string());
        } else if let Some(p) = rhs.strip_prefix("load i32, ptr ") {
            if ptr_regs.iter().any(|r| r == p) {
                id_regs.push(reg.to_string());
            }
        }
    }
    text.lines().map(str::trim).any(|l| {
        l.contains("icmp eq i32 ") && id_regs.iter().any(|r| l.ends_with(&format!(", {r}")))
    })
}

/// SHC3 item 1: the tower recheck of a class born on a family
/// compares the family word (`@perry_layout_class_<n>` `layout_id`), beside the
/// birth id. Anti-vacuity: the case is a real tower case with a recheck.
#[test]
fn tower_recheck_compares_the_family_word_the_instance_carries() {
    let ir = ir(CH3);
    let probe = function_body(&ir, "probe");
    assert!(
        probe.contains("idispatch.case0.pshape") && probe.contains("idispatch.case0.generic"),
        "the fixture no longer lowers a guarded tower case:\n{probe}"
    );
    assert!(
        compares_a_family_layout_id(&probe),
        "the recheck must accept the family word an instance carries:\n{probe}"
    );
}

/// SHC3 item 2: a checked field read admits a literal family's object by its
/// layout word (the descriptor names class 0, so its class id never matches).
#[test]
fn checked_read_admits_a_literal_family_by_its_layout_word() {
    let m = module(TM1);
    let ir = ir_with_facts(&m, tm1_facts(&m));
    let cid_blocks = blocks_labelled(&ir, "wpf.gget.cid");
    assert!(
        !cid_blocks.is_empty(),
        "the fixture no longer lowers a checked field read:\n{ir}"
    );
    assert!(
        compares_a_family_layout_id(&cid_blocks),
        "the receiver class test must admit the literal family's word:\n{cid_blocks}"
    );
}

/// SHC6: a class whose fields are all defined before anyone can observe the
/// instance. The first initializer runs code (`new Array().fill(0)`) but never
/// names the receiver, so the family holds the birth layout from birth on.
const CH_FREE: &str = r#"
interface Kinged { k(): number }
class G {
  private board: number[] = new Array<number>(8).fill(0);
  private side: string = "white";
  private wk = -1;
  constructor(s: string) { this.wk = s.length; }
  private king(c: string): number { return c === "white" ? this.wk + this.board.length : 0; }
  k(): number { return this.king(this.side); }
}
function probe(r: Kinged): number { return r.k(); }
console.log(probe(new G("..K..k")));
"#;

/// SHC6: the first initializer calls a method on the half-built instance:
/// `seed` runs while `side` and `wk` are still holes of the family layout.
const CH_OBSERVED: &str = r#"
interface Kinged { k(): number }
class G {
  private board: number = this.seed();
  private side: string = "white";
  private wk = -1;
  constructor(s: string) { this.wk = s.length; }
  private seed(): number { return 8; }
  private king(c: string): number { return c === "white" ? this.wk + this.board : 0; }
  k(): number { return this.king(this.side); }
}
function probe(r: Kinged): number { return r.k(); }
console.log(probe(new G("..K..k")));
"#;

/// Facts proving the receiver of `k`'s `this.king(...)` (the tower site: `this`
/// in `G.k`) a `G` whose layout is fixed. `probe`'s interface call takes the
/// slow method call, so the site the tower lowers is the one inside `G__k`.
fn tower_facts(m: &Module) -> crate::codegen::ProgramFactsView {
    use crate::codegen::{ElisionFacts, FactsClass, FactsRecv};
    let class = m.classes.iter().find(|c| c.name == "G").expect("class G");
    let k = class
        .methods
        .iter()
        .find(|f| f.name == "k")
        .expect("method G.k");
    let mut elision = ElisionFacts::default();
    elision.this_of.insert(
        k.id,
        FactsRecv {
            class_ids: vec![class.id],
            nullable: false,
            checked: false,
        },
    );
    elision.classes.insert(
        class.id,
        FactsClass {
            display: class.name.clone(),
            fixed_layout: true,
            fields: Vec::new(),
        },
    );
    crate::codegen::ProgramFactsView {
        elision: std::sync::Arc::new(elision),
        ..Default::default()
    }
}

/// `ir_with_facts` with the static shape ids assigned the way the driver does:
/// the tower cases a class only when its prototype layout is static
/// (`static_prototype::chain`).
fn tower_ir(m: &Module) -> String {
    let mut opts = ir_opts();
    opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
        [("main.ts", m)],
        true,
        &Default::default(),
    ));
    let births = crate::module_birth_shapes(m, opts.clone()).unwrap();
    let ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    opts.static_shape_ids = ids.into_iter().collect();
    opts.program_facts = tower_facts(m);
    String::from_utf8(compile_module(m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

fn g_family_init_count(m: &Module) -> (u32, usize) {
    family_init_count(m, "G")
}

fn family_init_count(m: &Module, name: &str) -> (u32, usize) {
    let layouts = crate::layout_families::compute([("main.ts", m)], true, &Default::default());
    let class = m.classes.iter().find(|c| c.name == name).expect("class");
    let fam = layouts.families.get(&class.id).expect("G has a family");
    (fam.init_count, fam.keys.len())
}

/// Field definitions after a receiver-free initializer are still the
/// initialization (`FamilyLayout::init_count`); one that names the receiver
/// ends it at its own key.
#[test]
fn init_count_counts_definitions_no_observer_can_precede() {
    assert_eq!(g_family_init_count(&module(CH_FREE)), (3, 3));
    assert_eq!(g_family_init_count(&module(CH_OBSERVED)), (0, 3));
}

/// The facts prove `fixed_layout` and the family holds the birth layout:
/// the tower case is the unchecked `$pshape` call.
#[test]
fn a_proven_tower_case_calls_the_clone_unchecked_when_the_family_holds_the_birth_layout() {
    let m = module(CH_FREE);
    let ir = tower_ir(&m);
    let k = function_body(&ir, "G__k(");
    assert!(
        k.contains("idispatch.case0") && k.contains("G__king$pshape"),
        "the fixture no longer routes the tower case to the clone:\n{k}"
    );
    assert!(
        !k.contains("idispatch.case0.generic"),
        "a proven case whose family holds the birth layout needs no recheck:\n{k}"
    );
}

/// SHC6 (a): an instance observed before its fields are defined holds the
/// family's holes, which the clone compiled for the birth pair reads raw.
/// `fixed_layout` alone does not license the unchecked call: the case keeps
/// its recheck (a family-born instance then takes the public body).
#[test]
fn a_proven_tower_case_keeps_its_recheck_when_the_family_does_not_hold_the_birth_layout() {
    let m = module(CH_OBSERVED);
    let ir = tower_ir(&m);
    let k = function_body(&ir, "G__k(");
    assert!(
        k.contains("idispatch.case0.pshape")
            && k.contains("G__king$pshape")
            && k.contains("idispatch.case0.generic"),
        "the case must recheck before it enters the clone:\n{k}"
    );
}

/// SHC6: a parent constructor that hands the receiver to code (`reg(this)`)
/// lets the receiver-free initializer `a = peek()` observe the instance
/// before `p` is defined (node reads `undefined`): the child's keys are no
/// initialization. The same child under a parent that only stores keeps
/// them.
#[test]
fn init_count_stops_after_a_parent_constructor_hands_out_the_receiver() {
    let src = |parent_body: &str| {
        format!(
            r#"
let seen: any = null;
function reg(o: any): void {{ seen = o; }}
function peek(): any {{ return seen === null ? 0 : seen.p; }}
class P {{ q: number; constructor() {{ this.q = 1; {parent_body} }} }}
class C extends P {{
  a: any = peek();
  p: any = 5;
  constructor() {{ super(); }}
}}
console.log(String(new C().a));
"#
        )
    };
    let leaking = family_init_count(&module(&src("reg(this);")), "C");
    let sealed = family_init_count(&module(&src("")), "C");
    assert_eq!(sealed, (3, 3), "a sealed parent keeps the child's definitions");
    assert_eq!(leaking.1, 3);
    assert!(
        leaking.0 <= 1,
        "keys defined after the parent handed out the receiver are no initialization: {leaking:?}"
    );
}
