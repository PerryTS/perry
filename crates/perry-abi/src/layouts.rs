//! Dense per-agent layout ABI v1. Identities and receiver words are distinct.
pub const LAYOUT_TAG: u32 = 0x8000_0000;
pub const LAYOUT_SHADOW_BIT: u32 = 0x4000_0000;
pub const LAYOUT_BAND_SHIFT: u32 = 28;
pub const LAYOUT_BAND_MASK: u32 = 0x3000_0000;
pub const LAYOUT_KIND_SHIFT: u32 = 26;
pub const LAYOUT_KIND_MASK: u32 = 0x0c00_0000;
pub const LAYOUT_ROW_MASK: u32 = 0x03ff_ffff;
pub const LAYOUT_ROW_LIMIT: u32 = 1 << 26;
pub const LAYOUT_ORDINARY_END: u32 = 0x9000_0000;
pub const LAYOUT_SHADOWED_BASE: u32 = 0xc000_0000;
pub const LAYOUT_SHADOWED_END: u32 = 0xd000_0000;
pub const LAYOUT_PROBE_INDEX_MASK: u32 = 0xb3ff_ffff;
pub const LAYOUT_WORD_ABI_VERSION: u32 = 4;
pub const LAYOUT_KIND_PLAIN: u32 = 0;
pub const LAYOUT_KIND_WORD: u32 = 1;
pub const LAYOUT_KIND_NULL: u32 = 2;
pub const LAYOUT_KIND_MIXED: u32 = 3;
pub const LAYOUT_BAND_ORDINARY: u32 = 0;
pub const LAYOUT_BAND_DICTIONARY: u32 = 1;
pub const LAYOUT_BAND_EXOTIC: u32 = 2;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeId(pub u32);
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LayoutWord(pub u32);
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LayoutRow(pub u32);
impl core::fmt::LowerHex for ShapeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.0, f)
    }
}
impl core::fmt::LowerHex for LayoutWord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.0, f)
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishedShape {
    pub identity: ShapeId,
    pub word: LayoutWord,
}
impl LayoutWord {
    /// Encoding checks capacity; masking an overflowing row is forbidden.
    pub const fn new(row: LayoutRow, band: u32, kind: u32) -> Option<Self> {
        if row.0 >= LAYOUT_ROW_LIMIT || band >= 3 || kind >= 4 {
            None
        } else {
            Some(Self(
                LAYOUT_TAG | band << LAYOUT_BAND_SHIFT | kind << LAYOUT_KIND_SHIFT | row.0,
            ))
        }
    }
    pub const fn row(self) -> LayoutRow {
        LayoutRow(self.0 & LAYOUT_ROW_MASK)
    }
    /// This word with the shadow bit set.
    #[inline(always)]
    pub const fn shadowed(self) -> Self {
        LayoutWord(self.0 | LAYOUT_SHADOW_BIT)
    }
    pub const fn unshadowed(self) -> Self {
        Self(self.0 & !LAYOUT_SHADOW_BIT)
    }
    pub const fn band(self) -> u32 {
        (self.0 & LAYOUT_BAND_MASK) >> LAYOUT_BAND_SHIFT
    }
    pub const fn kind(self) -> u32 {
        (self.0 & LAYOUT_KIND_MASK) >> LAYOUT_KIND_SHIFT
    }
    /// Numerical admission only. A live carrier is still required before reading.
    pub const fn ordinary_row(self, published_rows: usize) -> Option<LayoutRow> {
        let rel = (self.0 & LAYOUT_PROBE_INDEX_MASK).wrapping_sub(LAYOUT_TAG);
        if (rel as u64) < published_rows as u64 {
            Some(LayoutRow(rel))
        } else {
            None
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PerryStaticLayoutRequestV1 {
    /// Process-wide identity reservation cell; runtime accesses atomically.
    pub shape_id: u32,
    /// Immutable compiler-authored static word. Never a presence test.
    pub layout_word: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PerryLayoutImageV1 {
    pub abi_version: u32,
    pub static_rows: u32,
    pub family_first: u32,
    pub family_count: u32,
    /// The program's table (WRT step 2, [`PerryProgramTableV1`]): its static
    /// literal seeds and its lazy descriptor span. Never null; the runtime's own
    /// image names an empty table.
    pub program: *const PerryProgramTableV1,
}
// The program table is immutable program data every agent reads.
unsafe impl Send for PerryLayoutImageV1 {}
unsafe impl Sync for PerryLayoutImageV1 {}
impl PerryLayoutImageV1 {
    pub const RUNTIME: Self = Self {
        abi_version: LAYOUT_WORD_ABI_VERSION,
        static_rows: NATIVE_LAYOUT_ROWS,
        family_first: NATIVE_LAYOUT_ROWS,
        family_count: 0,
        program: &PerryProgramTableV1::EMPTY,
    };
    /// The header's facts with its program table's span. Without a span the
    /// class families follow the native rows; a span lies in the static rows,
    /// apart from the families ([`PerryProgramTableV1::valid_span`]).
    ///
    /// # Safety
    /// `program` is null or names a program table.
    pub const unsafe fn valid(self) -> bool {
        if self.abi_version != LAYOUT_WORD_ABI_VERSION || self.program.is_null() {
            return false;
        }
        let table = &*self.program;
        self.static_rows >= NATIVE_LAYOUT_ROWS
            && self.static_rows <= LAYOUT_ROW_LIMIT
            && if table.lazy_count == 0 {
                self.family_first == NATIVE_LAYOUT_ROWS
                    && self.family_count <= self.static_rows - self.family_first
            } else {
                table.valid_span(self)
            }
    }
}

/// One static literal seed of the program (WRT step 2): the facts
/// `js_layout_seed_plain` used to take, plus the seed's canonical key list
/// as image data. When every key is an image atom, `keys` names the atoms
/// (`PerryLayoutKey` records of the runtime, `key_count` of them) and `list`
/// is the user address of a canonical key list cell in `perry_realm` holding
/// their strings; the runtime links that cell into the agent's key-list trie
/// and publishes the record without allocating. Otherwise both are null and
/// the runtime mints the seed from its packed names.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PerryStaticSeedV1 {
    pub rep: u64,
    pub request: *mut PerryStaticLayoutRequestV1,
    /// NUL-terminated key names, `packed_len` bytes.
    pub packed: *const u8,
    pub keys: *const u8,
    pub list: *const u8,
    pub constfn: *const ConstFnStaticEntry,
    pub packed_len: u32,
    pub key_count: u32,
    pub live: u32,
    pub constfn_count: u32,
}
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<PerryStaticSeedV1>() == 64);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<PerryStaticSeedV1>() == 48);

/// The program's table, named by [`PerryLayoutImageV1::program`]: `count`
/// static seeds follow the header. The header also names the program's lazy
/// descriptor span: contiguous static rows whose descriptor facts publish on
/// first use. The span indexes descriptor values directly, never pointers or
/// ids. Only the startup registrar and a row's first touch read it, so it
/// stays out of the agent's thread-local copy of the image header.
#[repr(C)]
pub struct PerryProgramTableV1 {
    pub count: u32,
    pub lazy_count: u32,
    pub lazy_first: u32,
    pub _pad: u32,
    /// Exclusive end of compiler-assigned prototype serials, reserved before births.
    pub prototype_serial_end: u64,
    /// `lazy_count` consecutive PerryLayoutDesc records, in row order.
    /// Runtime descriptor aliases and realm receivers name these same facts.
    pub lazy_layouts: *const u8,
    pub seeds: [PerryStaticSeedV1; 0],
}
// Immutable program data every agent reads.
unsafe impl Send for PerryProgramTableV1 {}
unsafe impl Sync for PerryProgramTableV1 {}
impl PerryProgramTableV1 {
    /// The table of an image without a program: no seeds, no lazy span.
    pub const EMPTY: Self = Self {
        count: 0,
        lazy_count: 0,
        lazy_first: 0,
        _pad: 0,
        prototype_serial_end: crate::intrinsic_serials::FIRST_DYNAMIC_PROTOTYPE_SERIAL,
        lazy_layouts: core::ptr::null(),
        seeds: [],
    };

    /// # Safety
    /// `self` is the program's table: `count` seeds follow the header.
    pub unsafe fn seeds(&self) -> &[PerryStaticSeedV1] {
        core::slice::from_raw_parts(self.seeds.as_ptr(), self.count as usize)
    }

    /// The span's facts under `image` (rows in u64, so no sum wraps): the
    /// span and the class families lie in the static rows and do not overlap.
    pub const fn valid_span(&self, image: PerryLayoutImageV1) -> bool {
        let rows = image.static_rows as u64;
        let (first, end) = (
            self.lazy_first as u64,
            self.lazy_first as u64 + self.lazy_count as u64,
        );
        let (family, family_end) = (
            image.family_first as u64,
            image.family_first as u64 + image.family_count as u64,
        );
        self.prototype_serial_end >= crate::intrinsic_serials::FIRST_DYNAMIC_PROTOTYPE_SERIAL
            && self.prototype_serial_end < (1 << 32)
            && !self.lazy_layouts.is_null()
            && first >= NATIVE_LAYOUT_ROWS as u64
            && end <= rows
            && family >= NATIVE_LAYOUT_ROWS as u64
            && family_end <= rows
            && (image.family_count == 0 || end <= family || first >= family_end)
    }
}

pub const SHAPE_RECORD_ID_OFFSET: usize = 120;
pub const SHAPE_RECORD_LAYOUT_WORD_OFFSET: usize = 124;
pub const SHAPE_RECORD_FAMILY_DESC_OFFSET: usize = 76;
pub const LAYOUT_DESC_WORD_OFFSET: usize = 12;
pub const LAYOUT_DESC_ID_OFFSET_LP64: usize = 72;
pub const LAYOUT_DESC_ID_OFFSET_ILP32: usize = 56;
pub const LAYOUT_DESC_SIZE_LP64: usize = 80;
pub const LAYOUT_DESC_SIZE_ILP32: usize = 64;
/// `PerryLayoutKey.off`: the key's absolute slot offset from the object (u16).
/// It follows `atom` (a pointer), `slot` (u32), `attrs` and `_pad` (u8 each).
pub const LAYOUT_KEY_OFF_OFFSET_LP64: usize = 14;
pub const LAYOUT_KEY_OFF_OFFSET_ILP32: usize = 10;
/// `PerryLayoutKey` is 16 bytes on LP64 (12 on ILP32).
pub const LAYOUT_KEY_SIZE_LP64: usize = 16;
pub const LAYOUT_KEY_SIZE_ILP32: usize = 12;
/// Offsets of the A1 precomputes (`gc_header_word`) appended after `image_keys`
/// (`i64` aligned to 8 on every target).
pub const LAYOUT_DESC_GC_HEADER_WORD_OFFSET_LP64: usize = 88;
pub const LAYOUT_DESC_GC_HEADER_WORD_OFFSET_ILP32: usize = 72;
/// The descriptor is padded to 128 bytes on LP64 (112 on ILP32): the runtime indexes the descriptor
/// sections by `(address - start) / size`, and every size but a power of two costs more
/// instructions than the old 88 did, which startup may not pay (exact Ir).
pub const LAYOUT_DESC_FULL_SIZE_LP64: usize = 128;
pub const LAYOUT_DESC_FULL_SIZE_ILP32: usize = 112;

/// Image metadata, never a closure or managed edge.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConstFnStaticEntry {
    pub slot: u32,
    pub info: *const crate::JsFunctionInfo,
}

/// All returned u32 values below are LayoutWords, never ShapeIds.
pub mod ffi {
    use super::{ConstFnStaticEntry, PerryLayoutImageV1, PerryStaticLayoutRequestV1};
    unsafe extern "C" {
        pub fn js_gc_init_with_layouts(image: *const PerryLayoutImageV1);
        pub fn js_object_layout_for_class_keys_static(
            keys: u64,
            key_count: u32,
            live: u32,
            class_id: u32,
            request: *mut PerryStaticLayoutRequestV1,
            rep: u64,
        ) -> u32;
        pub fn js_object_final_layout_for_class_keys_static_private(
            keys: u64,
            key_count: u32,
            live: u32,
            class_id: u32,
            request: *mut PerryStaticLayoutRequestV1,
            rep: u64,
            private_packed: *const u8,
            private_len: u32,
            brands: *const u64,
            brand_count: u32,
        ) -> u32;
        pub fn js_object_final_layout_for_class_keys_static_constfn(
            keys: u64,
            key_count: u32,
            live: u32,
            class_id: u32,
            request: *mut PerryStaticLayoutRequestV1,
            rep: u64,
            entries: *const ConstFnStaticEntry,
            entry_count: u32,
        ) -> u32;
    }
}

pub const FUNCTION_LAYOUT_WORD: u32 = 0xa400_0000;
pub const ASYNC_FUNCTION_LAYOUT_WORD: u32 = 0xa400_0001;
pub const FUNCTION_DICTIONARY_LAYOUT_WORD: u32 = 0xa000_0004;
pub const WEAKMAP_BIRTH_LAYOUT_WORD: u32 = 0x8000_0009;
pub const WEAKSET_BIRTH_LAYOUT_WORD: u32 = 0x8000_000a;
pub const fn intrinsic_function_layout_word(index: u32) -> u32 {
    assert!(index < 9);
    if index == 4 {
        FUNCTION_DICTIONARY_LAYOUT_WORD
    } else {
        0xa400_0000 | index
    }
}

/// Union of native descriptor names across target/cfg variants, sorted.
/// Rows 0..9 are intrinsic functions; rows 9 and 10 are weak collections.
pub const NATIVE_LAYOUT_DESCRIPTORS: &[&str] = &[
    "ARRAY_NAMED",
    "ARRAY_PROTOTYPE_NAMED",
    "CJS_MODULE_RECORD",
    "DUPLEX",
    "DUPLEX_PROTOTYPE",
    "EE_ASYNC_RESOURCE_PROTO",
    "EVENT_EMITTER",
    "EVENT_EMITTER_PROTOTYPE",
    "FUNCTION_PROTOTYPE",
    "IMMEDIATE",
    "INCOMING_MESSAGE_PROTOTYPE",
    "ITERATOR_RESULT",
    "ITERATOR_RESULT_DONE_FIRST",
    "LEGACY_STREAM",
    "NATIVE_CTOR_DUPLEX",
    "NATIVE_CTOR_EVENT_EMITTER",
    "NATIVE_CTOR_INCOMING_MESSAGE",
    "NATIVE_CTOR_OUTGOING_MESSAGE",
    "NATIVE_CTOR_PASSTHROUGH",
    "NATIVE_CTOR_READABLE",
    "NATIVE_CTOR_SERVER_RESPONSE",
    "NATIVE_CTOR_STREAM",
    "NATIVE_CTOR_TRANSFORM",
    "NATIVE_CTOR_WRITABLE",
    "OBJECT_PROTOTYPE",
    "OUTGOING_MESSAGE_PROTOTYPE",
    "PASSTHROUGH",
    "PASSTHROUGH_PROTOTYPE",
    "READABLE",
    "READABLE_PROTOTYPE",
    "READABLE_STATE",
    "READABLE_STATE_PROTO",
    "RECORD_F0_0",
    "RECORD_F13_0",
    "RECORD_F17_0",
    "RECORD_F20_0",
    "RECORD_F21_0",
    "RECORD_F28_0",
    "RECORD_F29_0",
    "RECORD_F30_0",
    "RECORD_F31_0",
    "RECORD_F327_0",
    "RECORD_F328_0",
    "RECORD_F37_0",
    "RECORD_F3_0",
    "RECORD_F411_0",
    "RECORD_F412_0",
    "RECORD_F413_0",
    "RECORD_F414_0",
    "RECORD_F418_0",
    "RECORD_F426_0",
    "RECORD_F427_0",
    "RECORD_F429_0",
    "RECORD_F430_0",
    "RECORD_F434_0",
    "RECORD_F470_0",
    "RECORD_F471_0",
    "RECORD_F473_0",
    "RECORD_F483_0",
    "RECORD_F487_0",
    "RECORD_F568_0",
    "RECORD_F6_0",
    "RECORD_F7_0",
    "RECORD_PCHECKED031",
    "RECORD_Q435_0",
    "RECORD_Q436_0",
    "RECORD_Q439_0",
    "RECORD_Q440_0",
    "RECORD_Q441_0",
    "RECORD_Q443_0",
    "RECORD_Q448_0",
    "RECORD_Q449_0",
    "RECORD_Q450_0",
    "RECORD_Q451_0",
    "RECORD_Q452_0",
    "RECORD_Q453_0",
    "RECORD_Q454_0",
    "RECORD_Q455_0",
    "RECORD_Q456_0",
    "RECORD_Q457_0",
    "RECORD_Q458_0",
    "RECORD_Q459_0",
    "RECORD_Q460_0",
    "RECORD_Q461_0",
    "RECORD_Q462_0",
    "RECORD_Q463_0",
    "RECORD_Q464_0",
    "RECORD_Q465_0",
    "RECORD_Q466_0",
    "RECORD_Q467_0",
    "RECORD_Q468_0",
    "RECORD_Q468_1",
    "RECORD_Q468_2",
    "RECORD_Q468_3",
    "RECORD_U610",
    "RECORD_U613",
    "RECORD_V026",
    "REGEX_EXEC_INDICES_NAMED",
    "REGEX_EXEC_NAMED",
    "REGEX_INDICES_GROUPS_NAMED",
    "SERVER_RESPONSE",
    "SERVER_RESPONSE_PROTOTYPE",
    "SPAWN_SYNC_RESULT",
    "SQLITE_COLUMN_METADATA",
    "STREAM_PROTOTYPE",
    "TIMEOUT",
    "TRANSFORM",
    "TRANSFORM_PROTOTYPE",
    "URL_COMPONENTS",
    "URL_SEARCH_PARAMS",
    "WRITABLE",
    "WRITABLE_PROTOTYPE",
    "WRITABLE_STATE",
    "WRITABLE_STATE_PROTO",
];
pub const NATIVE_LAYOUT_ROWS: u32 = 11 + NATIVE_LAYOUT_DESCRIPTORS.len() as u32;
/// Const lookup used only by native declaration initializers / compiler planning.
pub const fn native_layout_row(name: &str) -> LayoutRow {
    let mut i = 0;
    while i < NATIVE_LAYOUT_DESCRIPTORS.len() {
        let a = name.as_bytes();
        let b = NATIVE_LAYOUT_DESCRIPTORS[i].as_bytes();
        if a.len() == b.len() {
            let mut j = 0;
            while j < a.len() && a[j] == b[j] {
                j += 1;
            }
            if j == a.len() {
                return LayoutRow(11 + i as u32);
            }
        }
        i += 1;
    }
    panic!("native layout absent from ABI catalog");
}
pub const fn native_layout_word(name: &str, kind: u32) -> u32 {
    match LayoutWord::new(native_layout_row(name), LAYOUT_BAND_ORDINARY, kind) {
        Some(word) => word.0,
        None => panic!("invalid native layout kind"),
    }
}
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(
        size_of::<ShapeId>() == 4 && size_of::<LayoutWord>() == 4 && size_of::<LayoutRow>() == 4
    );
    assert!(size_of::<PerryStaticLayoutRequestV1>() == 8);
    assert!(align_of::<PerryStaticLayoutRequestV1>() == 4);
    assert!(offset_of!(PerryStaticLayoutRequestV1, layout_word) == 4);
    assert!(
        size_of::<PerryLayoutImageV1>() == 16 + size_of::<usize>()
            && align_of::<PerryLayoutImageV1>() == size_of::<usize>()
    );
    assert!(offset_of!(PerryLayoutImageV1, static_rows) == 4);
    assert!(offset_of!(PerryLayoutImageV1, family_first) == 8);
    assert!(offset_of!(PerryLayoutImageV1, family_count) == 12);
    assert!(offset_of!(PerryLayoutImageV1, program) == 16);
    assert!(offset_of!(PerryProgramTableV1, lazy_count) == 4);
    assert!(offset_of!(PerryProgramTableV1, lazy_first) == 8);
    assert!(offset_of!(PerryProgramTableV1, prototype_serial_end) == 16);
    assert!(offset_of!(PerryProgramTableV1, lazy_layouts) == 24);
    assert!(offset_of!(PerryProgramTableV1, seeds) == 32);
};
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_descriptor_span_is_checked_without_overflow() {
        let facts = core::ptr::without_provenance(4096);
        let table = |lazy_first, lazy_count, lazy_layouts| PerryProgramTableV1 {
            lazy_first,
            lazy_count,
            lazy_layouts,
            ..PerryProgramTableV1::EMPTY
        };
        let image =
            |program: &PerryProgramTableV1, family_first, family_count| PerryLayoutImageV1 {
                static_rows: NATIVE_LAYOUT_ROWS + 4,
                family_first,
                family_count,
                program,
                ..PerryLayoutImageV1::RUNTIME
            };
        let n = NATIVE_LAYOUT_ROWS;
        let span = table(n, 3, facts);
        unsafe {
            assert!(PerryLayoutImageV1::RUNTIME.valid());
            assert!(image(&span, n + 3, 1).valid());
            // A span after the families, as the unit-test runtime places it.
            assert!(image(&table(n + 1, 3, facts), n, 1).valid());
            assert!(!image(&span, n + 2, 1).valid());
            assert!(!image(&span, n + 3, 2).valid());
            assert!(!image(&table(n, 5, facts), n + 5, 0).valid());
            assert!(!image(&table(u32::MAX, 3, facts), n + 3, 1).valid());
            assert!(!image(&table(n, u32::MAX, facts), n + 3, 1).valid());
            assert!(!image(&table(n, 3, facts), u32::MAX, u32::MAX).valid());
            assert!(!image(&table(n, 3, core::ptr::null()), n + 3, 1).valid());
            // Without a span the families follow the native rows.
            let empty = PerryProgramTableV1::EMPTY;
            assert!(image(&empty, n, 4).valid());
            assert!(!image(&empty, n + 1, 0).valid());
            assert!(!PerryLayoutImageV1 {
                program: core::ptr::null(),
                ..PerryLayoutImageV1::RUNTIME
            }
            .valid());
        }
    }

    #[test]
    fn word_encoding_and_receiver_admission() {
        for band in 0..3 {
            for kind in 0..4 {
                let w = LayoutWord::new(LayoutRow(17), band, kind).unwrap();
                assert_eq!(w.row(), LayoutRow(17));
                assert_eq!(w.band(), band);
                assert_eq!(w.kind(), kind);
                assert_eq!(
                    w.ordinary_row(18),
                    if band == 0 { Some(LayoutRow(17)) } else { None }
                );
            }
        }
        for (row, band, kind, expected) in [
            (0, 0, 0, 0x80000000),
            (1, 0, 2, 0x88000001),
            (1, 0, 3, 0x8c000001),
            (1, 1, 1, 0x94000001),
            (0, 2, 1, 0xa4000000),
        ] {
            assert_eq!(
                LayoutWord::new(LayoutRow(row), band, kind).unwrap().0,
                expected
            );
        }
        assert_eq!(LayoutWord(0xc0000000).ordinary_row(1), Some(LayoutRow(0)));
        assert_eq!(LayoutWord(0).ordinary_row(1), None);
        assert_eq!(
            LayoutWord(u32::MAX).ordinary_row(LAYOUT_ROW_LIMIT as usize),
            None
        );
        assert_eq!(LayoutWord::new(LayoutRow(LAYOUT_ROW_LIMIT), 0, 0), None);
        assert_eq!(LayoutWord::new(LayoutRow(0), 3, 0), None);
        assert_eq!(LayoutWord::new(LayoutRow(0), 0, 4), None);
    }
    #[test]
    fn catalog_is_sorted_and_image_is_bounded() {
        assert!(NATIVE_LAYOUT_DESCRIPTORS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(NATIVE_LAYOUT_ROWS, 125);
        assert!(unsafe { PerryLayoutImageV1::RUNTIME.valid() });
        for (i, name) in NATIVE_LAYOUT_DESCRIPTORS.iter().enumerate() {
            assert_eq!(native_layout_row(name).0, 11 + i as u32);
        }
    }
}

/// OC3 001: static prototype birth word occupies the old declaration padding.
/// Nonzero exactly when birth_shape is nonzero; neither field proves publication.
pub const CLASS_DECL_BIRTH_LAYOUT_WORD_OFFSET_LP64: usize = 20;
pub const CLASS_DECL_BIRTH_LAYOUT_WORD_OFFSET_ILP32: usize = 16;
#[repr(C)]
pub struct PerryClassDeclarationPrefixV1 {
    pub members: *const core::ffi::c_void,
    pub member_count: u32,
    pub birth_shape: u32,
    pub flags: u32,
    pub birth_layout_word: u32,
}
const _: () = {
    assert!(
        core::mem::offset_of!(PerryClassDeclarationPrefixV1, birth_layout_word)
            == if cfg!(target_pointer_width = "64") {
                CLASS_DECL_BIRTH_LAYOUT_WORD_OFFSET_LP64
            } else {
                CLASS_DECL_BIRTH_LAYOUT_WORD_OFFSET_ILP32
            }
    );
};

/// `ClassMemberDecl::flags` (codegen's `{ ptr, ptr, ptr, i32, i32, i32, i8, i8,
/// i16 }`, the second `i8`): the member's name bytes are valid UTF-8. The
/// emitter sets it when it has proven the bytes (codegen: the emitted name has
/// no lone surrogate; a native class: a Rust `&'static str`), and registration
/// reads the name without validating it. Clear, the name is validated, and one
/// that is not UTF-8 (a WTF-8 name) names no member.
pub const CLASS_MEMBER_NAME_UTF8: u8 = 4;

/// Explicit dynamic mints: all returns and finalizer carriers are LayoutWords.
pub mod dynamic_layout_ffi {
    unsafe extern "C" {
        pub fn js_object_layout_for_keys(keys: u64, count: u32) -> u32;
        pub fn js_object_layout_for_class_keys(
            keys: u64,
            count: u32,
            class_id: u32,
            rep: u64,
        ) -> u32;
        pub fn js_object_layout_for_class_keys_live(
            keys: u64,
            count: u32,
            live: u32,
            class_id: u32,
            rep: u64,
        ) -> u32;
    }
}

/// A layout's prototype identity. Runtime-created prototypes leave the image
/// pointer null; image families name their immutable template address directly.
/// The pointer is raw so Mach-O can carry it in an ordinary chained fixup.
#[repr(C)]
pub struct PerryProtoFamily {
    pub serial: u64,
    pub image_prototype: *const u8,
}

const _: () = {
    assert!(core::mem::offset_of!(PerryProtoFamily, serial) == 0);
    assert!(core::mem::offset_of!(PerryProtoFamily, image_prototype) == 8);
    assert!(core::mem::size_of::<PerryProtoFamily>() == 16);
};
