//! User-visible method receivers are boxed values, including positive
//! subnormals whose bits resemble a raw heap address. The pointer gate must
//! dominate every class-header read, independent of source annotations.
use super::*;

#[test]
fn packed_class_selection_proves_the_boxed_pointer_band_before_dereferencing() {
    let ir = emit_static(&polymorphic_array_site_module(), false);
    let caller = function_body(&ir, "__probe(");
    let gate = blocks(&caller)
        .into_iter()
        .find(|(_, body)| {
            body.iter()
                .any(|line| line.contains("br i1") && line.contains("label %method_probe.deref"))
        })
        .expect("class selection must gate its first dereference");
    let triple = crate::codegen::helpers::default_target_triple();
    let floor = crate::target_layout::heap_addr_lower_bound_inclusive(&triple);
    let ceiling = crate::target_layout::heap_addr_upper_bound_exclusive(&triple);
    let bias = (crate::nanbox::POINTER_TAG | floor).to_string();
    let span = (ceiling - floor).to_string();
    assert!(
        gate.1
            .iter()
            .any(|line| line.contains("sub i64") && line.ends_with(&bias)),
        "the range must include the POINTER tag, not merely mask it off: {:?}",
        gate.1
    );
    assert!(gate
        .1
        .iter()
        .any(|line| line.contains("icmp ult i64") && line.ends_with(&span)));
    assert!(
        !gate
            .1
            .iter()
            .any(|line| line.contains("icmp eq i64") && line.ends_with(", 0")),
        "a tag-zero Number must never enter the class-header read: {:?}",
        gate.1
    );
}
