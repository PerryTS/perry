//! The bounded-index element read (`bidx.*`, `lower_bounded_array_index_get_checked`),
//! read from the emitted IR.
//!
//! ```ts
//! function f(bs: any[]) {
//!   let s: any = 0;
//!   for (let i = 0; i < bs.length; i++) { s = bs[i]; }
//!   return s;
//! }
//! ```
//!
//! The counter loop proves `i < bs.length`, so `bs[i]` takes this tier. Its
//! receiver is only DECLARED an array, and the declaration is a hint.
//!
//! The proof is the length hoist, which keeps `bs.length` only when the
//! loop's sites run no program code (#14537, `crate::quiet`). That is the
//! whole-program solve's word, and a unit test runs no solve: the module is
//! compiled with the quiet-site facts `perry compile` gives this program
//! (`PERRY_FACTS_DUMP`: `bs.length` a quiet read with a primitive value,
//! `bs[i]` a quiet element site with a primitive value; [`quiet_facts`]).
//! Without them the hoist is refused and `bs[i]` takes no `bidx.*` tier.
//!
//! * #11876: a hole read `undefined` straight from the slot, skipping the
//!   prototype chain (`Array.prototype[2]` never showed through). A hole must
//!   take the generic read, which resolves it through the chain.

#![cfg(test)]

use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};

const BS: u32 = 1;
const S: u32 = 2;
const I: u32 = 3;

/// The quiet-site facts of this program by the whole-program solve
/// (`perry compile` of the module docs' program, `PERRY_FACTS_DUMP`):
///
/// ```text
/// # quiet kind=read      key=length verdict=quiet   (bs.length)
/// # quiet kind=value     key=length verdict=quiet
/// # quiet kind=element              verdict=quiet   (bs[i])
/// # quiet kind=elemvalue            verdict=quiet
/// ```
///
/// The HIR here has offset 0 at both sites, so they are keyed by 0.
fn quiet_facts() -> ProgramFactsView {
    let mut elision = ElisionFacts::default();
    elision.quiet_reads.insert((0, "length".to_string()));
    elision.primitive_reads.insert((0, "length".to_string()));
    elision.quiet_elements.insert(0);
    elision.primitive_elements.insert(0);
    ProgramFactsView {
        elision: std::sync::Arc::new(elision),
        ..Default::default()
    }
}

/// The IR of the module docs' program; `quiet`: with [`quiet_facts`].
fn module_ir(quiet: bool) -> String {
    let mut module = Module::new("bounded_array_index");
    module.init.push(Stmt::Expr(Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)], None)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    let counter = Stmt::Let {
        id: I,
        name: "i".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    };
    let condition = Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(I)),
        right: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::LocalGet(BS)),
            property: "length".to_string(),
            byte_offset: 0,
        }),
    };
    let update = Expr::Update {
        id: I,
        op: UpdateOp::Increment,
        prefix: false,
    };
    let body = vec![Stmt::Expr(Expr::LocalSet(
        S,
        Box::new(Expr::IndexGet {
            element_offset: 0,
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(BS)),
            index: Box::new(Expr::LocalGet(I)),
        }),
    ))];
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: BS,
            name: "bs".to_string(),
            ty: Type::Array(Box::new(Type::Any)),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: S,
                name: "s".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(counter)),
                condition: Some(condition),
                update: Some(update),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(S))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        program_facts: if quiet {
            quiet_facts()
        } else {
            ProgramFactsView::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// The body of the block whose label starts with `prefix` (up to the next
/// blank line).
fn block<'a>(ir: &'a str, prefix: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{prefix}"))
        .unwrap_or_else(|| panic!("premise: no block `{prefix}*` in\n{ir}"));
    let rest = &ir[start + 1..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

fn label_of(block_text: &str) -> &str {
    block_text.split(':').next().expect("block label")
}

#[test]
fn a_hole_takes_the_generic_read() {
    let ir = module_ir(true);
    let fast = block(&ir, "bidx.fast.");
    let lazy = block(&ir, "bidx.lazy.");
    assert!(
        lazy.contains("@js_array_get_f64"),
        "premise: the generic arm calls the full element read\n{lazy}"
    );
    assert!(
        fast.contains(crate::nanbox::TAG_HOLE_I64),
        "premise: the fast arm tests the slot for a hole\n{fast}"
    );
    assert!(
        !fast.contains(crate::nanbox::TAG_UNDEFINED_I64),
        "#11876: a hole must not read as `undefined` without the prototype chain\n{fast}"
    );
    assert!(
        fast.contains(&format!("label %{}", label_of(lazy))),
        "#11876: a hole must branch to the generic read\n{fast}"
    );
}

/// The twin: without the quiet-site facts (no solve, or a `bs` that may be
/// a Proxy or carry an index getter) the loop's sites may run program code,
/// the hoist is refused (#14537) and no `bidx.*` block is emitted: `bs[i]`
/// takes the guarded generic read, whose fallback is the full element read.
#[test]
fn without_the_quiet_facts_the_read_takes_no_bounded_tier() {
    let ir = module_ir(false);
    assert!(
        !ir.contains("\nbidx."),
        "without the facts' word on the loop's sites no `bidx.*` tier may be emitted\n{ir}"
    );
    assert!(
        ir.contains("call double @perry_array_read_cold_shared("),
        "premise: `bs[i]` takes the generic read with its full fallback\n{ir}"
    );
}
