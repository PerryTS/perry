//! A literal-shape class has no constructor function. Its synthetic
//! constructor would only assign its arguments to its keys, in order. A birth
//! the compiler completes (`static_constfn::birth_literal`, the layout birth)
//! never calls it; any other birth calls the runtime's literal assignment loop
//! (`js_literal_shape_initialize`) itself ([`initialize`]) with the values it
//! holds: all of them, or for a chunked birth (`split_regions_literal`) only
//! the head, whose chunks then store the rest. A literal-shape class has no
//! class value either (`({}).constructor` is `Object`), so nothing can
//! construct it by class id: no constructor is emitted or registered for it.
//!
//! Measured on tssrc (LIT3, 2026-10-23): 1,830 constructor functions
//! (409k IR instructions) for 3,121 call sites, nearly all in arms LLVM folds
//! away; the loop that replaces them runs 1 to 9 times per witness program.

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Whether `class` is a literal shape, which has no constructor function.
pub(crate) fn has_no_constructor(class: &perry_hir::Class) -> bool {
    class.is_literal_shape()
}

/// The constructor method of a class without a constructor function: emit
/// nothing for it.
pub(super) fn try_compile(class: &perry_hir::Class, method: &perry_hir::Function) -> bool {
    has_no_constructor(class) && method.name == format!("{}_constructor", class.name)
}

/// Initialize the birth `receiver` of `class` with `values`, its first
/// `values.len()` keys in order, as its constructor would have; the
/// constructor's result (`undefined`). `None` when `class` has a constructor
/// function.
pub(crate) fn initialize(
    ctx: &mut FnCtx<'_>,
    class: &perry_hir::Class,
    receiver: &str,
    values: &[String],
) -> Option<String> {
    if !has_no_constructor(class) {
        return None;
    }
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    if values.is_empty() {
        // Nothing to assign: the birth is already complete.
        return Some(undefined);
    }
    let keys = ctx.class_keys_globals.get(&class.name)?.clone();
    let buffer = ctx.func.alloca_entry_array(DOUBLE, values.len().max(1));
    let blk = ctx.block();
    for (i, value) in values.iter().enumerate() {
        let slot = blk.gep(DOUBLE, &buffer, &[(I64, &i.to_string())]);
        blk.store(DOUBLE, value, &slot);
    }
    // No allocating operation lies between the stores and this call, and the
    // runtime roots the receiver and the buffer's values before it can run a
    // setter: the buffer needs no GC slots.
    blk.call_void(
        "js_literal_shape_initialize",
        &[
            (DOUBLE, receiver),
            (PTR, &format!("@{keys}")),
            (PTR, &buffer),
            (I32, &values.len().to_string()),
        ],
    );
    Some(undefined)
}
