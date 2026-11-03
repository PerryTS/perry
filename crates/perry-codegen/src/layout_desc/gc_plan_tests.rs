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
