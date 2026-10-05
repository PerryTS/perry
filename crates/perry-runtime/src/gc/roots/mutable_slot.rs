/// Which registry a mutable root slot came from.
///
/// The kind selects a *telemetry bucket* only — it must never select a
/// different pointer decoding. All kinds are marked by
/// `mark_mutable_root_bits` and rewritten by `try_rewrite_value`, and all
/// therefore accept a heap reference either NaN-boxed or bare. That symmetry
/// is the #6910 invariant; see `gc::root_words`.
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
