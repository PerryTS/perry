//! Wide synthetic constructors keep their existing boxed ABI, but marshal
//! arguments once for a shared runtime assignment loop. This also bounds the
//! constructors emitted for shapes whose literal sites use descriptors.

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR};

pub(super) fn try_compile(
    module: &mut LlModule,
    class: &perry_hir::Class,
    method: &perry_hir::Function,
    name: &str,
    keys_global: Option<&String>,
) -> bool {
    if class.fields.len() < 32
        || !class.is_literal_shape()
        || method.name != format!("{}_constructor", class.name)
        || method.params.len() != class.fields.len()
    {
        return false;
    }
    let Some(keys) = keys_global else {
        return false;
    };
    let mut params = vec![(DOUBLE, "%this_arg".to_string())];
    params.extend(
        method
            .params
            .iter()
            .map(|p| (DOUBLE, format!("%arg{}", p.id))),
    );
    let function = module.define_function(name, DOUBLE, params);
    function.no_inline = true;
    function.create_block("entry");
    let values = function.alloca_entry_array(DOUBLE, method.params.len());
    let block = function.block_mut(0).unwrap();
    for (i, param) in method.params.iter().enumerate() {
        let slot = block.gep(DOUBLE, &values, &[(I64, &i.to_string())]);
        block.store(DOUBLE, &format!("%arg{}", param.id), &slot);
    }
    // No allocating operation precedes this call. The runtime roots the
    // receiver and complete buffer before it can run a setter; no managed
    // operand is used here after the call, so this frame needs no GC slots.
    block.call_void(
        "js_literal_shape_initialize",
        &[
            (DOUBLE, "%this_arg"),
            (PTR, &format!("@{keys}")),
            (PTR, &values),
            (I32, &method.params.len().to_string()),
        ],
    );
    block.ret(
        DOUBLE,
        &crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
    );
    true
}
