use super::*;

/// Object model step 3: the class-id tower selects its body by the
/// receiver's class id against the classes whose method is a prototype
/// constant; no runtime proof, epoch, own-property probe or holder-directory
/// read remains, and release code carries no contract check.
#[test]
fn tower_selects_by_prototype_constant() {
    let ir = emit_static(&tower_site_module(), false);
    assert!(ir.contains("idispatch.case"), "{ir}");
    assert!(ir.contains("pconst.selected") && !ir.contains("certify"));
    assert!(!ir.contains("@PERRY_PROTO_VALIDITY"));
    let site = ir
        .split("\ndefine ")
        .find(|f| f.contains("pconst.selected"))
        .expect("the function holding the selecting site");
    assert!(
        !site.contains("@js_object_get_own_field_or_undef("),
        "{site}"
    );
    assert!(!site.contains("holder.tower"), "{site}");
    assert!(!site.contains("call void @js_proto_constant_contract_check("));
}

/// A program with a dynamic write may write any prototype slot: the tower
/// keeps its compiled cases, each behind its holders' shadow bits
/// (`proto_constants::holder_constant`), so a written holder selects nothing
/// and its receivers take the generic site.
#[test]
fn a_dynamic_write_cases_behind_the_holder_shadow_bits() {
    let mut census = perry_hir::ProtoCensus::default();
    census.dynamic_write = true;
    let ir = crate::proto_constants::with_test_census(census, || {
        emit_static(&tower_site_module(), false)
    });
    let site = ir
        .split("\ndefine ")
        .find(|f| f.contains("pconst.selected"))
        .expect("the function holding the selecting site");
    assert!(site.contains("pconst.case"), "{site}");
    assert!(site.contains("holder.page"), "{site}");
    // Without the dynamic write the slots are constants: no holder test.
    let ir = emit_static(&tower_site_module(), false);
    assert!(!ir.contains("holder.page"), "{ir}");
}
