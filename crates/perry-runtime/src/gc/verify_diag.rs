//! Failure-only attribution for the evacuation verifier.
//!
//! The verifier's passing slot closure deliberately stays free of the page,
//! registry, type-name and descriptor re-walks below.  A stale edge is already
//! fatal, so that path can spend the extra work needed to name the owner and
//! the collection coverage which failed to visit it.

use super::*;
use std::cell::Cell;

#[derive(Clone, Copy)]
pub(super) struct EvacuationVerifyCycleContext<'a> {
    pub(super) minor: u64,
    pub(super) trigger: GcTriggerKind,
    pub(super) after_budgeted_step: bool,
    pub(super) dirty_snapshot: Option<&'a RememberedDirtySnapshot>,
}

crate::perry_thread_local! {
    static VERIFY_MINOR_ORDINAL: Cell<u64> = const { Cell::new(0) };
    static LAST_VERIFY_BUDGETED_COMPLETIONS: Cell<u64> = const { Cell::new(0) };
}

pub(super) fn begin_evacuation_verify_cycle(
    trigger: GcTriggerKind,
    dirty_snapshot: Option<&RememberedDirtySnapshot>,
) -> EvacuationVerifyCycleContext<'_> {
    let minor = VERIFY_MINOR_ORDINAL.with(|ordinal| {
        let next = ordinal.get().saturating_add(1);
        ordinal.set(next);
        next
    });
    let completed = super::instruments::incremental_completions_on_current_thread();
    let after_budgeted_step = LAST_VERIFY_BUDGETED_COMPLETIONS.with(|last| {
        let after = completed > last.get();
        last.set(completed);
        after
    });
    EvacuationVerifyCycleContext {
        minor,
        trigger,
        after_budgeted_step,
        dirty_snapshot,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EvacuationVerifyStats {
    pub(super) parents: usize,
    pub(super) slots: usize,
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn type_name(obj_type: u8) -> &'static str {
    gc_type_info(obj_type).map_or("unknown", |info| info.name)
}

fn decoded_addr(bits: u64) -> usize {
    decode_root_word(bits)
        .map(|word| word.addr())
        .unwrap_or((bits & POINTER_MASK) as usize)
}

unsafe fn object_type_at(verifier: EvacuationVerifier<'_>, addr: usize) -> &'static str {
    if addr <= GC_HEADER_SIZE || !verifier.valid_ptrs.contains(&addr) {
        return "unknown";
    }
    type_name((*header_from_user_ptr(addr as *const u8)).obj_type)
}

unsafe fn object_space(header: *mut GcHeader, user: usize) -> &'static str {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_PINNED != 0 {
        return "pinned";
    }
    if flags & GC_FLAG_ARENA == 0 {
        return "malloc";
    }
    match crate::arena::classify_heap_space(user) {
        crate::arena::HeapSpace::PromotedYoung => "promoted_in_place_this_cycle",
        crate::arena::HeapSpace::NurseryEden => "nursery_from",
        space if space == crate::arena::active_survivor_space() => "nursery_from",
        space if space == crate::arena::inactive_survivor_space() => "nursery_to",
        crate::arena::HeapSpace::Old | crate::arena::HeapSpace::Longlived => "old_page",
        crate::arena::HeapSpace::Unknown
        | crate::arena::HeapSpace::Survivor0
        | crate::arena::HeapSpace::Survivor1 => "old_page",
    }
}

unsafe fn forwarded_target_space(verifier: EvacuationVerifier<'_>, addr: usize) -> &'static str {
    if addr <= GC_HEADER_SIZE || !verifier.valid_ptrs.contains(&addr) {
        return "old_page";
    }
    object_space(header_from_user_ptr(addr as *const u8), addr)
}

fn layout_visitor_name(kind: GcLayoutSlotKind) -> &'static str {
    match kind {
        GcLayoutSlotKind::None => "GcMutableSlotDescriptor",
        GcLayoutSlotKind::ArrayElements => "ArrayElements",
        GcLayoutSlotKind::ObjectFields => "ObjectFields",
        GcLayoutSlotKind::RegExpFields => "RegExpFields",
        GcLayoutSlotKind::ClosureCaptures => "ClosureCaptures",
        GcLayoutSlotKind::ObjectMeta => "ObjectMeta",
    }
}

fn rewrite_visitor_name(kind: GcRewriteDescriptorKind) -> &'static str {
    match kind {
        GcRewriteDescriptorKind::Leaf => "GcMutableSlotDescriptor",
        GcRewriteDescriptorKind::Array => "ArrayFields",
        GcRewriteDescriptorKind::Object => "ObjectSideFields",
        GcRewriteDescriptorKind::RegExp => "RegExpFields",
        GcRewriteDescriptorKind::Closure => "ClosureSideFields",
        GcRewriteDescriptorKind::Promise => "PromiseFields",
        GcRewriteDescriptorKind::Error => "ErrorFields",
        GcRewriteDescriptorKind::Map => "MapEntries",
        GcRewriteDescriptorKind::LazyArray => "LazyArrayFields",
        GcRewriteDescriptorKind::Set => "SetElements",
        GcRewriteDescriptorKind::NativeTypedView => "NativeTypedViewFields",
        GcRewriteDescriptorKind::NativePodView => "NativePodViewFields",
        GcRewriteDescriptorKind::ObjectMeta => "ObjectMeta",
        GcRewriteDescriptorKind::MetaOnly => "MetaOnlyFields",
        GcRewriteDescriptorKind::Buffer => "BufferBacking",
    }
}

unsafe fn descriptor_slot_index(
    descriptor: GcMutableSlotDescriptor,
    wanted: usize,
) -> Option<usize> {
    match descriptor {
        GcMutableSlotDescriptor::Slot(slot) => (slot.slot as usize == wanted).then_some(0),
        GcMutableSlotDescriptor::Range { range, .. } => {
            let start = range.slots() as usize;
            let offset = wanted.checked_sub(start)?;
            (offset % std::mem::size_of::<u64>() == 0
                && offset / std::mem::size_of::<u64>() < range.slot_count())
            .then_some(offset / std::mem::size_of::<u64>())
        }
        GcMutableSlotDescriptor::PointerFreeRange(_) => None,
    }
}

unsafe fn descriptor_slot_count(descriptor: GcMutableSlotDescriptor) -> usize {
    match descriptor {
        GcMutableSlotDescriptor::Slot(_) => 1,
        GcMutableSlotDescriptor::Range { range, .. } => range.slot_count(),
        GcMutableSlotDescriptor::PointerFreeRange(_) => 0,
    }
}

unsafe fn describe_parent_slot(
    header: *mut GcHeader,
    slot_addr: usize,
) -> (Option<usize>, &'static str) {
    let layout_kind = gc_type_layout_slot_kind((*header).obj_type);
    let mut layout_match = None;
    let mut layout_base = 0usize;
    visit_gc_layout_slot_descriptors(header, &mut |descriptor| {
        if layout_match.is_none() {
            layout_match = descriptor_slot_index(descriptor, slot_addr)
                .map(|index| layout_base.saturating_add(index));
        }
        layout_base = layout_base.saturating_add(descriptor_slot_count(descriptor));
    });
    if let Some(index) = layout_match {
        return (Some(index), layout_visitor_name(layout_kind));
    }

    let rewrite_kind = gc_type_rewrite_descriptor_kind((*header).obj_type);
    let mut rewrite_match = None;
    let mut rewrite_base = 0usize;
    visit_gc_rewrite_slot_descriptors(header, |descriptor| {
        if rewrite_match.is_none() {
            rewrite_match = descriptor_slot_index(descriptor, slot_addr)
                .map(|index| rewrite_base.saturating_add(index));
        }
        rewrite_base = rewrite_base.saturating_add(descriptor_slot_count(descriptor));
    });
    (rewrite_match, rewrite_visitor_name(rewrite_kind))
}

#[cold]
pub(super) fn panic_stale_forwarded_reference_detailed(
    verifier: EvacuationVerifier<'_>,
    surface: &str,
    slot_addr: usize,
    old_bits: u64,
    new_bits: u64,
) -> ! {
    let surface_token = surface
        .chars()
        .map(|ch| if ch.is_ascii_whitespace() { '_' } else { ch })
        .collect::<String>();
    let old_addr = decoded_addr(old_bits);
    let new_addr = decoded_addr(new_bits);
    let (child_type, child_space) = unsafe {
        (
            object_type_at(verifier, old_addr).or_else_unknown(object_type_at(verifier, new_addr)),
            forwarded_target_space(verifier, new_addr),
        )
    };
    let (minor, trigger, after_budgeted_step) = verifier.context.map_or_else(
        || ("n/a".to_owned(), "n/a".to_owned(), "n/a"),
        |context| {
            (
                context.minor.to_string(),
                format!("{:?}", context.trigger),
                yes_no(context.after_budgeted_step),
            )
        },
    );

    if let Some(parent_header) = verifier.parent_header {
        unsafe {
            let parent = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            let parent_space = object_space(parent_header, parent);
            let (slot_index, visitor) = describe_parent_slot(parent_header, slot_addr);
            let slot_index = slot_index.map_or_else(|| "n/a".to_owned(), |i| i.to_string());
            let coverage_expected = matches!(
                parent_space,
                "old_page" | "malloc" | "promoted_in_place_this_cycle"
            );
            let (remembered, dirty_snapshot) = if coverage_expected {
                let remembered_snapshot = remembered_dirty_snapshot();
                let remembered = old_young_slot_covered(
                    &remembered_snapshot,
                    parent_header as usize,
                    slot_addr as *mut u64,
                );
                let dirty = verifier
                    .context
                    .and_then(|context| context.dirty_snapshot)
                    .map(|snapshot| {
                        old_young_slot_covered(
                            snapshot,
                            parent_header as usize,
                            slot_addr as *mut u64,
                        )
                    });
                (
                    yes_no(remembered),
                    dirty.map_or("n/a(no_cycle_snapshot)", yes_no),
                )
            } else {
                ("n/a(nursery_parent)", "n/a(nursery_parent)")
            };
            panic!(
                "gc evacuation verification failed: stale forwarded pointer in {surface}: surface={surface_token} parent=0x{parent:x} parent_type={} parent_space={parent_space} slot=0x{slot_addr:x} slot_index={slot_index} visitor={visitor} old=0x{old_bits:x} forwarded_to=0x{new_bits:x} child_type={child_type} child_space={child_space} remembered={remembered} young_logged=n/a(heap_parent_uses_remembered_set) dirty_snapshot={dirty_snapshot} minor={minor} trigger={trigger} after_budgeted_step={after_budgeted_step}",
                type_name((*parent_header).obj_type),
            );
        }
    }

    panic!(
        "gc evacuation verification failed: stale forwarded pointer in {surface}: surface={surface_token} parent=n/a(root) parent_type=n/a(root) parent_space=n/a(root) slot=0x{slot_addr:x} slot_index=n/a(root) visitor={surface_token} old=0x{old_bits:x} forwarded_to=0x{new_bits:x} child_type={child_type} child_space={child_space} remembered=n/a(root) young_logged=n/a(root_scanner_does_not_expose_owner_key) dirty_snapshot=n/a(root) minor={minor} trigger={trigger} after_budgeted_step={after_budgeted_step}"
    );
}

trait UnknownTypeFallback {
    fn or_else_unknown(self, fallback: Self) -> Self;
}

impl UnknownTypeFallback for &'static str {
    fn or_else_unknown(self, fallback: Self) -> Self {
        if self == "unknown" {
            fallback
        } else {
            self
        }
    }
}

pub(super) fn report_evacuation_success(
    context: EvacuationVerifyCycleContext<'_>,
    stats: EvacuationVerifyStats,
    old_young_edges: usize,
) {
    if gc_diag_enabled() {
        eprintln!(
            "[gc-verify] minor={} evacuation_ok parents={} slots={} old_young_edges={old_young_edges}",
            context.minor, stats.parents, stats.slots,
        );
    }
}
