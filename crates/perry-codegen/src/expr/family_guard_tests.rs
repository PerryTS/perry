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
