//! Kind decoding and plain-birth eligibility from the packed record word.
use super::*;

impl ShapeRecord {
    #[inline]
    pub(in crate::object) fn object_kind(&self) -> ShapeObjectKind {
        let code = (self.flags_and_kind & RECORD_KIND_MASK) >> RECORD_KIND_SHIFT;
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
            _ => ShapeObjectKind::CallableOrdinary,
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
}
