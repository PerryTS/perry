use super::*;
use crate::layout_families::FamilyKey;

/// A class family descriptor carries its static GC scan plan: bit 31 and one
/// bit per pointer-bearing slot (every slot of a class family, whose rep is 0).
#[test]
fn a_class_family_descriptor_emits_its_static_gc_plan() {
    // Collecting mode: the descriptor word is the image plan's business, not
    // this test's.
    crate::layout_ordinals::begin_module("gcplan", Default::default(), true);
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    let mut strings = StringPool::with_prefix("gcplan".into());
    let mut emitted = BTreeMap::new();
    let mut keys = Vec::new();
    for (slot, name) in ["left", "right"].into_iter().enumerate() {
        let atom = atom(&mut m, &mut strings, &mut emitted, name).unwrap();
        keys.push((FamilyKey::Name(name.into()), atom, slot as u32));
    }
    emit_desc(
        &mut m,
        "perry_layouts",
        ("gcplan_keys", "gcplan_index", "gcplan_desc"),
        0x1234,
        &keys,
        2,
        (7, 0),
        "null",
        0,
    );
    let ir = m.to_ir();
    let descriptor = ir
        .lines()
        .find(|line| line.starts_with("@gcplan_desc = global"))
        .expect("class family descriptor must be defined");
    let plan = crate::runtime_abi::layout_gc_plan(2, 0);
    assert_eq!(plan, crate::runtime_abi::LAYOUT_GC_PLAN_STATIC | 0b11);
    assert!(
        descriptor.contains(&format!("ptr null, i32 0, i32 {}, ptr", plan as i32)),
        "the descriptor's plan word must be the two-slot plan: {descriptor}"
    );
}

/// An array creation family's named layout lists the keys its store sites
/// declare ahead of their first store: each carries `LAYOUT_KEY_OPTIONAL`
/// (absent while its slot holds the hole), as a class family's late key does.
/// The leading iterator key keeps attribute 0, and so does every key of a
/// layout that is not an array's.
#[test]
fn an_array_family_descriptor_marks_its_declared_keys_optional() {
    crate::layout_ordinals::begin_module("arrlate", Default::default(), true);
    let mut m = LlModule::new("x86_64-unknown-linux-gnu");
    let mut strings = StringPool::with_prefix("arrlate".into());
    let mut emitted = BTreeMap::new();
    let range = atom(&mut m, &mut strings, &mut emitted, "rangeEnd").unwrap();
    let keys = vec![String::new(), "rangeEnd".to_string()];
    let atoms = vec!["perry_atom_well_known_iterator".to_string(), range.clone()];
    for (suffix, array_named) in [("arrlate_fam", true), ("litlate_fam", false)] {
        emit_descriptor(
            &mut m,
            "perry_layouts",
            &format!("{suffix}_desc"),
            suffix,
            &keys,
            &atoms,
            0x55,
            2,
            0,
            LAYOUT_ID_CARRIED,
            Some(1),
            "0",
            array_named,
        );
    }
    let ir = m.to_ir();
    let list = |suffix: &str| {
        ir.lines()
            .find(|line| line.starts_with(&format!("@perry_layout_keys_{suffix} = ")))
            .unwrap_or_else(|| panic!("key list {suffix} must be defined"))
            .to_string()
    };
    let optional = crate::runtime_abi::LAYOUT_KEY_OPTIONAL as i8;
    let array = list("arrlate_fam");
    assert!(
        array.contains("ptr @perry_atom_well_known_iterator, i32 0, i8 0,"),
        "the iterator key keeps attribute 0: {array}"
    );
    assert!(
        array.contains(&format!("ptr @{range}, i32 1, i8 {optional},")),
        "a declared late key is optional: {array}"
    );
    let literal = list("litlate_fam");
    assert!(
        literal.contains(&format!("ptr @{range}, i32 1, i8 0,")),
        "a key of a layout that is not an array's carries no attribute: {literal}"
    );
}
