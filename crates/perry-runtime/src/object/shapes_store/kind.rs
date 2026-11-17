//! Kind decoding and plain-birth eligibility from the packed record word.
use super::*;

impl ShapeRecord {
    #[inline]
    pub(in crate::object) fn object_kind(&self) -> ShapeObjectKind {
        let code = (self.flags_and_kind & RECORD_KIND_MASK) >> RECORD_KIND_SHIFT;
        // The field is written only by `ShapeRecord::new` from
        // `ShapeObjectKind::code()`, whose largest code is
        // RECORD_KIND_MAX_CODE; the codes are the declaration order, so the
        // match below is the identity and costs nothing (no clamp of the
        // unused top codes).
        // SAFETY: the only writer stores `code()` (checked there).
        unsafe { core::hint::assert_unchecked(code <= RECORD_KIND_MAX_CODE) };
        match code {
            0 => ShapeObjectKind::Ordinary,
            1 => ShapeObjectKind::Class,
            2 => ShapeObjectKind::Function,
            3 => ShapeObjectKind::FunctionDictionary,
            4 => ShapeObjectKind::OrdinaryUnmarked,
            5 => ShapeObjectKind::OrdinaryNumericProof,
            6 => ShapeObjectKind::NativeNamespace,
            7 => ShapeObjectKind::FunctionBoundCall,
            8 => ShapeObjectKind::FunctionBoundApply,
            9 => ShapeObjectKind::FunctionBound,
            10 => ShapeObjectKind::OrdinaryNativeAlias,
            11 => ShapeObjectKind::CallableOrdinary,
            12 => ShapeObjectKind::OrdinaryUnmarkedNativeAlias,
            13 => ShapeObjectKind::Arguments,
            _ => ShapeObjectKind::ArgumentsStrict,
        }
    }

    /// A plain birth must name a present ordinary record of exactly this
    /// width. Presence and kind share one word; test them together instead
    /// of decoding a kind enum after a separate presence check. EMPTY fails
    /// even for a zero-slot birth. Other liveness/summary bits are irrelevant.
    #[inline(always)]
    pub(in crate::object) fn admits_plain_birth(&self, field_count: u32) -> bool {
        self.flags_and_kind & (RECORD_KIND_MASK | u32::from(RECORD_FLAG_PRESENT))
            == u32::from(RECORD_FLAG_PRESENT)
            && self.live_inline_slot_count == field_count
    }

    /// "This record is of kind `kind`", answered on the stored field: one masked
    /// compare against the encoded code, no decode (the hot tests fold with the
    /// neighbouring flag bits, as `present() && kind_is(Ordinary)` does).
    #[inline(always)]
    pub(in crate::object) fn kind_is(&self, kind: ShapeObjectKind) -> bool {
        self.flags_and_kind & RECORD_KIND_MASK == (kind.code() as u32) << RECORD_KIND_SHIFT
    }

    /// "Both records are of the same kind", on the stored fields (no decode).
    #[inline(always)]
    pub(in crate::object) fn same_kind_as(&self, other: &ShapeRecord) -> bool {
        (self.flags_and_kind ^ other.flags_and_kind) & RECORD_KIND_MASK == 0
    }
}
