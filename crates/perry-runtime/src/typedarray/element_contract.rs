//! `PERRY_CHECK_CONTRACTS`: the run-time witness of the declared-kind element
//! proof that codegen emits inline (`perry-codegen` `expr/typed_element.rs`).
//!
//! Compiled with the knob set, every access the inline proof admits calls this
//! first. It re-derives the admission from the runtime's own view of the cell
//! (ownership-checked header, owner role, element kind, live length) and
//! aborts if the inline proof admitted an access the runtime would not. It
//! reads only; it neither allocates nor collects.
use super::{type_for_kind, KIND_UINT8};

#[no_mangle]
pub extern "C" fn js_typed_element_contract(value: f64, kind: i32, index: i64) -> i32 {
    let bits = value.to_bits();
    let admitted = (bits >> 48 == 0x7FFD)
        .then(|| (bits & crate::value::POINTER_MASK) as usize)
        .and_then(|addr| Some((addr, crate::buffer::header::byte_cell_type(addr)?)))
        .is_some_and(|(addr, ty)| {
            let kind = kind as u8;
            let brand = ty == type_for_kind(kind)
                || (kind == KIND_UINT8 && ty == crate::gc::GC_TYPE_BUFFER);
            brand
                && !crate::buffer::store::is_view(addr)
                && index >= 0
                && (index as usize) < unsafe { crate::buffer::store::length(addr) }
        });
    if !admitted {
        contract_violated(bits, kind, index);
    }
    0
}

#[cold]
#[inline(never)]
fn contract_violated(bits: u64, kind: i32, index: i64) -> ! {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "perry: PERRY_CHECK_CONTRACTS: the inline typed-element proof admitted \
         value {bits:#018x} as an owner of kind {kind} at index {index}, which \
         the runtime rejects"
    );
    std::process::abort()
}
