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

/// `Row` and four subclasses that inherit `Row`'s methods, all reached
/// through the interface-typed tower.
fn inherited_tower_module() -> Module {
    let mut m = tower_site_module();
    for (k, name) in ["R1", "R2", "R3", "R4"].iter().enumerate() {
        let mut sub = class(110 + k as u32, name, Vec::new(), Vec::new());
        sub.extends = Some(103);
        sub.extends_name = Some("Row".to_string());
        m.classes.push(sub);
    }
    m
}

/// Classes that inherit the same body from the same holder under the same
/// proof are case values of ONE switch arm (one proof, one call); only the
/// class that declares the body keeps its own arm. Both with constant slots
/// and behind the holder shadow bits.
#[test]
fn inherited_bodies_share_one_switch_arm() {
    let count = |ir: &str| -> (usize, usize, usize) {
        let site = ir
            .split("\ndefine ")
            .find(|f| f.contains("pconst.selected"))
            .expect("the function holding the selecting site");
        let arms = site
            .lines()
            .filter(|l| l.starts_with("pconst.case"))
            .count();
        let calls: usize = site
            .lines()
            .filter(|l| l.contains("switch i32") && l.contains("label %idispatch.case"))
            .map(|l| l.matches("label %idispatch.case").count())
            .sum();
        let values: usize = site
            .lines()
            .filter(|l| l.contains("switch i32") && l.contains("label %pconst.case"))
            .map(|l| l.matches("label %pconst.case").count())
            .sum();
        (arms, calls, values)
    };
    let constant = emit_static(&inherited_tower_module(), false);
    let mut census = perry_hir::ProtoCensus::default();
    census.dynamic_write = true;
    let shadowed = crate::proto_constants::with_test_census(census, || {
        emit_static(&inherited_tower_module(), false)
    });
    for ir in [&constant, &shadowed] {
        // Two sites (`rescore`, `tag`), each: Row's own arm and one shared
        // arm for R1..R4, five case values.
        assert_eq!(count(ir), (4, 4, 10), "{ir}");
    }
}
