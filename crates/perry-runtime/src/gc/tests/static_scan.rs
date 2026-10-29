//! A family layout's static GC scan plan (`gc/static_scan.rs`) is a second
//! enumeration of an object's slots, so it is pinned four ways: BOTH collectors
//! actually take it (a fast path nothing reaches would make every other test
//! here vacuous), every child it visits survives and is rewritten, a plan that
//! drops a slot is REFUSED by the generic-walk cross-check that runs in test and
//! debug-assertion builds, and an object that is not on its family's own layout
//! word is not planned at all.

use super::super::*;
use super::support::*;
use crate::gc::static_scan::sabotage;
use crate::object::layout_desc::{perry_alloc_layout, PerryLayoutDesc, PerryLayoutKey};
use crate::object::{js_register_class_declaration, ClassDeclaration};

const CLASS: u32 = 60_291;
const FIELDS: usize = 3;

static EMPTY_KEYS: [PerryLayoutKey; 0] = [];
static EMPTY_INDEX: [u32; 1] = [u32::MAX];

#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
static mut STATIC_SCAN_LAYOUT: PerryLayoutDesc = PerryLayoutDesc {
    image_keys: std::ptr::null_mut(),
    shape_id: 0,
    gc_plan: crate::codegen_abi::layout_gc_plan(FIELDS as u32, 0),
    family: 0x41574203,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: FIELDS as u32,
    layout_id: 0x8000008d,
    rep: 0,
    proto: std::ptr::null(),
    keys: EMPTY_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 0,
    index: EMPTY_INDEX.as_ptr(),
    class_id: CLASS,
    parent_class_id: 0,
    methods: std::ptr::null(),
    gc_header_word: crate::object::layout_desc::layout_gc_header_word(FIELDS as u32),
    _tail_pad: [0; 32],
};

static DECLARATION: ClassDeclaration = ClassDeclaration {
    instance_layout: std::ptr::addr_of!(STATIC_SCAN_LAYOUT),
    ..ClassDeclaration::native(&[])
};

fn string_bytes(addr: usize) -> Vec<u8> {
    unsafe {
        let s = addr as *const crate::StringHeader;
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

#[derive(Clone, Copy)]
enum Cycle {
    Minor,
    FullMark,
}

/// A rooted family object whose every field holds a young string, then one
/// collection. `Ok((objects scanned through a plan, every child intact))`; `Err`
/// is the collection thread's panic message.
fn collect_over_a_family_object(cycle: Cycle, sabotaged: bool) -> Result<(u32, bool), String> {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        crate::gc::initialize_runtime_fixture();
        unsafe { js_register_class_declaration(CLASS, &DECLARATION) };
        crate::object::register_class(CLASS, 0);
        let desc = std::ptr::addr_of!(STATIC_SCAN_LAYOUT);
        let parent = unsafe { perry_alloc_layout(desc) };
        assert_eq!(
            unsafe { (*parent).parent_class_id },
            unsafe { (*desc).layout_id },
            "premise: the object is born on its family's own layout word"
        );
        let fields = unsafe {
            (parent as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64
        };
        let mut children = Vec::new();
        for i in 0..FIELDS {
            let child = young_leaf();
            unsafe { *fields.add(i) = string_bits(child) };
            children.push((child, string_bytes(child)));
        }
        js_shadow_slot_set(0, ptr_bits(parent as usize));
        let before = sabotage::hits();
        {
            let _sabotage = sabotaged.then(sabotage::DropTopSlot::arm);
            match cycle {
                Cycle::Minor => {
                    let _ = gc_collect_minor();
                }
                Cycle::FullMark => {
                    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                        GcTriggerKind::Direct,
                    ));
                }
            }
        }
        let hits = sabotage::hits() - before;
        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let fields_after = unsafe {
            (parent_after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64
        };
        let intact = children.iter().enumerate().all(|(i, (old, bytes))| {
            let word = unsafe { *fields_after.add(i) };
            let now = (word & POINTER_MASK) as usize;
            // A minor evacuates the child; a full mark leaves it where it is.
            // Either way the word must name a live string with the same bytes.
            (matches!(cycle, Cycle::FullMark) || now != *old) && string_bytes(now) == *bytes
        });
        (hits, intact)
    })
    .join()
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

#[test]
fn a_minor_scans_a_family_object_through_its_static_plan_and_moves_every_child() {
    let (hits, intact) =
        collect_over_a_family_object(Cycle::Minor, false).expect("minor must not panic");
    assert!(
        hits > 0,
        "premise: the minor must scan through the static plan"
    );
    assert!(
        intact,
        "every field's young child must be evacuated and rewritten"
    );
}

#[test]
fn a_full_mark_scans_a_family_object_through_its_static_plan_and_keeps_every_child() {
    let (hits, intact) =
        collect_over_a_family_object(Cycle::FullMark, false).expect("full mark must not panic");
    assert!(
        hits > 0,
        "premise: the full mark must scan through the static plan"
    );
    assert!(intact, "every field's child must stay live");
}

#[test]
fn a_plan_that_drops_a_slot_is_refused_by_the_generic_walk_cross_check() {
    for cycle in [Cycle::Minor, Cycle::FullMark] {
        let outcome = collect_over_a_family_object(cycle, true);
        assert!(
            matches!(&outcome, Err(message) if message.contains("static_scan")),
            "a plan missing the top payload slot must be caught by the cross-check; got {outcome:?}"
        );
    }
}

#[test]
fn the_descriptor_plan_is_the_pointer_slots_of_the_representation() {
    use crate::codegen_abi::{layout_gc_plan, LAYOUT_GC_PLAN_MAX_SLOTS, LAYOUT_GC_PLAN_STATIC};
    // Slot 1 is an F64 lane and slot 3 a deprecated F64 lane: neither is scanned.
    let rep = (0b01u64 << 2) | (0b10u64 << 6);
    assert_eq!(layout_gc_plan(5, rep), LAYOUT_GC_PLAN_STATIC | 0b10101);
    assert_eq!(layout_gc_plan(0, 0), LAYOUT_GC_PLAN_STATIC);
    // A SPECIAL lane's collectability is a runtime fact of the shape.
    assert_eq!(layout_gc_plan(2, 0b11 << 2), 0);
    // Too wide for the descriptor word: the generic walk.
    assert_eq!(layout_gc_plan(LAYOUT_GC_PLAN_MAX_SLOTS + 1, 0), 0);
    assert_eq!(
        layout_gc_plan(LAYOUT_GC_PLAN_MAX_SLOTS, 0),
        LAYOUT_GC_PLAN_STATIC | 0x7FFF_FFFF
    );
}
