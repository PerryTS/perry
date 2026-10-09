/// Which registry a mutable root slot came from.
///
/// Native statepoint slots and globals carry JSValues at rest. Shadow frames
/// remain the documented untyped platform fallback. Mark, copy and rewrite
/// must use the same producer encoding for each kind (#6910).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) enum MutableRootSlotKind {
    #[cfg_attr(perry_native_stack_maps, allow(dead_code))]
    ShadowStack,
    NativeStack,
    GlobalRoot,
}

impl MutableRootSlotKind {
    /// Label for the pin-latch abort's `copying walk phase` line.
    pub(in crate::gc) fn walk_phase_name(self) -> &'static str {
        match self {
            Self::ShadowStack => "mutable_root_slots/shadow_stack",
            Self::NativeStack => "mutable_root_slots/native_stack",
            Self::GlobalRoot => "mutable_root_slots/global_root",
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::gc) struct MutableRootSlot {
    pub(in crate::gc) kind: MutableRootSlotKind,
    pub(in crate::gc) ptr: *mut u64,
}

impl MutableRootSlot {
    #[inline]
    pub(in crate::gc) unsafe fn read(self) -> u64 {
        *self.ptr
    }

    #[inline]
    pub(in crate::gc) unsafe fn write(self, bits: u64) {
        *self.ptr = bits;
    }
}

/// Generated native slots and globals are JSValues at rest. Legacy shadow
/// frames on unsupported targets (including WASI) retain the untyped decoder.
/// Provenance remains a telemetry concern; this adapter expresses the actual
/// producer ABI, then feeds the same precise marker as runtime visitors.
#[inline]
pub(in crate::gc) fn mark_mutable_slot(
    slot: MutableRootSlot,
    bits: u64,
    valid_ptrs: &super::super::ValidPointerSet,
) {
    match slot.kind {
        MutableRootSlotKind::ShadowStack => super::super::mark_mutable_root_bits(bits, valid_ptrs),
        MutableRootSlotKind::NativeStack | MutableRootSlotKind::GlobalRoot => {
            super::super::mark_precise_root(super::super::PreciseRoot::JSValue(bits), valid_ptrs);
        }
    }
}

impl MutableRootSlot {
    /// A word the source encoding admits as a GC reference. Keep this decision
    /// shared by mark, preflight, copy, rewrite and verification.
    #[inline]
    pub(in crate::gc) fn pointer_word(self, bits: u64) -> Option<super::super::RootWord> {
        match self.kind {
            MutableRootSlotKind::ShadowStack => super::super::decode_root_word(bits),
            MutableRootSlotKind::NativeStack | MutableRootSlotKind::GlobalRoot => {
                super::super::decode_nanboxed_root_word(bits)
            }
        }
    }
}
