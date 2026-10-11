//! Generator methods use the callable installed on this class evaluation.
use super::*;

pub(super) unsafe fn explicit_generator_method(
    class_id: u32,
    sym_key: usize,
    object: f64,
    symbol: f64,
    receiver: f64,
) -> Option<f64> {
    let (_, _, _, entry) =
        crate::object::lookup_class_symbol_method_in_chain(class_id, sym_key, false)?;
    if entry == 0
        || !crate::closure::info_has(
            &*(entry as *const crate::closure::JsFunctionInfo),
            crate::closure::FN_GENERATOR,
        )
    {
        return None;
    }
    // Default class instances use the declaration's prototype; the caller
    // materializes that existing surface. An evaluation/override shape proves
    // that its actual chain supersedes the declaration table.
    if !crate::object::prototype_chain::object_has_individual_class_prototype(
        crate::value::js_nanbox_get_pointer(object) as usize,
    ) {
        return None;
    }
    // The declaration table spans evaluations; an absent key on this actual
    // prototype remains absent, including deletion and a different Symbol.
    Some(
        resolve_explicit_object_prototype_symbol(object, symbol, receiver)
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
    )
}
