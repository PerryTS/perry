//! A wide literal-shape class (at least [`WIDE`] fields) has no constructor
//! function. Its synthetic constructor would only marshal its arguments into
//! a buffer for the runtime's literal assignment loop
//! (`js_literal_shape_initialize`), so each birth calls that loop itself
//! ([`initialize`]) with the values it holds: all of them, or for a chunked
//! birth (`split_regions_literal`) only the head, whose chunks then store the
//! rest. A literal-shape class has no class value either (`({}).constructor`
//! is `Object`), so nothing can construct it by class id: no constructor is
//! emitted or registered for it.

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I64, PTR};

/// The width from which a literal-shape class has no constructor function.
const WIDE: usize = 32;

/// Whether `class` is a literal shape that has no constructor function.
pub(crate) fn has_no_constructor(class: &perry_hir::Class) -> bool {
    class.fields.len() >= WIDE && class.is_literal_shape()
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
    Some(crate::nanbox::double_literal(f64::from_bits(
        crate::nanbox::TAG_UNDEFINED,
    )))
}
