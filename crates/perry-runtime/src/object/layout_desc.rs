//! Object model step 0 (OBJECT_MODEL_2026-10-13.md §6a): compiler-owned
//! layout descriptors.
//!
//! Codegen emits one [`PerryLayoutDesc`] per layout into the section
//! `perry_layouts`, and one [`StaticAtom`] per program-named key into
//! `perry_atoms`. Before any user code runs in an agent,
//! [`register_static_layouts`]:
//!
//! 1. pre-seeds the agent's atom table with every static atom, so a key built
//!    from data interns to the same string object as the program's literal;
//! 2. registers every CARRIED descriptor (one that generated code births or
//!    stamps; codegen emits its `layout_id` as [`LAYOUT_ID_CARRIED`]), in
//!    section order, into the existing shape table under
//!    `LAYOUT_BASE + index`, so a carried layout IS a ShapeId and every shape
//!    reader keeps working (coexistence). Identical facts share one id: a
//!    by-facts hit (an earlier descriptor, or a hash-band static seed) keeps
//!    the existing id. The id the descriptor resolved to is written into
//!    `desc.layout_id`; generated code loads it from there.
//!
//! A descriptor no object carries (`layout_id` 0) is static data only: the
//! key-index probe reads it in place and the shape table never sees it. The
//! choice is the compiler's and is fixed in the binary; nothing is decided at
//! first use. Step 0 registered all of them, which cost fastify 8.2M
//! instructions at startup (1,292 canonical key lists and mints) for layouts
//! no object carried.
//!
//! The layout word is `ObjectHeader.parent_class_id`: the LayoutId is the
//! full ShapeId value, and bit 30 ([`LAYOUT_SHADOW_BIT`]) is the shadow bit,
//! which no ShapeId carries (object model §3.6; set by `object::shadow`).

mod literal_birth;
pub use literal_birth::perry_alloc_literal_layout;
mod optional_family;

use super::ObjectHeader;
mod define_field;
pub(crate) mod methods;
use crate::codegen_abi::{LayoutWord, ShapeId};
use crate::string::StringHeader;
pub(crate) use define_field::family_define_field;

/// Bit 30 of the layout word: THE shadow bit (object model §3.6, §6a). One
/// bit, one meaning, one stamping rule, shared by step 3 and step 4.
///
/// **Meaning.** Set: the object's own contents may differ from what its
/// layout id and the program's static facts say. Clear: they do not; the
/// layout alone answers every own-key question (presence, absence,
/// attributes) and the object's [[Prototype]] is the one its birth recorded.
///
/// **Who sets it** (never cleared; a delete may leave it set: one wasted
/// probe, never a wrong answer):
/// * `overflow::install`: the object gets an overflow (§3.4, step 4): own
///   keys extend past the layout, or a fixed key was deleted;
/// * `object::shadow` (step 3): a dynamic write adds, deletes or redefines a
///   source-named key on the object, or replaces a marked prototype's
///   member in place; a [[Prototype]] relink; any write to an intrinsic
///   prototype (`builtin_proto`);
/// * every restamp carries it ([`carry_shadow`]); no store of a layout word
///   bypasses that rule.
///
/// **What each reader concludes:**
/// * `overflow::table` / `has_overflow`: clear proves no overflow (one load,
///   no meta); set means *load* `meta.overflow`, which may still be null (the
///   bit was set by a step-3 write). Nothing concludes "overflow" from the
///   bit alone.
/// * Step-3 prototype-constant sites and inherited loads (`proto_constants`,
///   `builtin_proto`, `UNSHADOWED_WORD_END`): clear proves the receiver has no
///   own copy of the bound name and every holder still holds its declared
///   member; set takes the generic call/load.
/// * Runtime caches key on `shapes::object_shape_stamp` /
///   [`ObjectHeader::shape_word`]: with an overflow installed that is the
///   full word, so every way answering ABSENT or INHERITED keys misses (an
///   overflow entry may shadow them); with the bit alone it is the layout id,
///   which is exact there (every own-key or [[Prototype]] change of an object
///   without an overflow still changes its layout id). Ways answering an OWN
///   fixed slot compare [`layout_id_of_word`] in every case (`read_confirm`:
///   the key's slot is the same with or without the bit; a deleted key leaves
///   the hole marker, which misses).
/// * Compiled inline guards compare the full word (unmasked): a shadowed
///   object takes the generic path (step 5 replaces those guards).
/// * Readers that only ask "may the own keys extend past the layout?"
///   (`overflow::shadowed`: closure-bag own reads, descriptor-key snapshots,
///   the IC slow position guess, symbol-absence proofs) decline on the bit
///   alone: conservative, one load.
pub const LAYOUT_SHADOW_BIT: u32 = 1 << 30;
/// The LayoutId of a layout word: a shadowed word ([`word_is_shadowed`])
/// names the ShapeId below its bit 30; every other word (an unshadowed
/// ShapeId, a class id an allocation wrote before stamping, 0) is itself.
#[inline(always)]
pub const fn layout_id_of_word(word: u32) -> u32 {
    // Every unshadowed ShapeId and every small class id lies below the
    // shadowed band: one compare on the common path.
    if word < SHADOWED_WORD_BASE || word >= SHADOWED_WORD_END {
        word
    } else {
        word & !LAYOUT_SHADOW_BIT
    }
}

/// Shadowed words: an ordinary-band ShapeId (`is_site_matchable_layout_word`)
/// with bit 30 set, `[0xC000_0000, 0xF000_0000)`. Only those ids carry the
/// bit, so the band is exact: the word never holds a class id from it
/// (class ids are small counters, `0x7FFF_FFxx` or `0xFFFF_xxxx`; synthetic
/// class ids live only in `ObjectHeader.class_id`).
pub const SHADOWED_WORD_BASE: u32 = super::shapes::SHAPE_ID_BASE | LAYOUT_SHADOW_BIT;
pub const SHADOWED_WORD_END: u32 = crate::codegen_abi::LAYOUT_SHADOWED_END;
const _: () = assert!(SHADOWED_WORD_BASE == 0xC000_0000 && SHADOWED_WORD_END == 0xD000_0000);
/// The emitted site test (`perry-codegen` `proto_constants`): a layout word
/// below this is an unshadowed ordinary layout or a word no write stamped.
/// Shadowed and exotic words all lie at or above it.
pub const UNSHADOWED_WORD_END: u32 = crate::codegen_abi::LAYOUT_ORDINARY_END;
const _: () = assert!(UNSHADOWED_WORD_END == crate::codegen_abi::LAYOUT_ORDINARY_END);

/// Is `word` a shadowed ordinary layout word?
#[inline(always)]
pub const fn word_is_shadowed(word: u32) -> bool {
    word.wrapping_sub(SHADOWED_WORD_BASE) < SHADOWED_WORD_END - SHADOWED_WORD_BASE
}

impl ObjectHeader {
    /// The ShapeId whose layout describes ALL of the object's own keys: the
    /// word itself when unshadowed; the layout id below bit 30 when the bit
    /// was set by a step-3 write alone (every own-key change of such an object
    /// still changes its layout id); and the raw shadowed word, which no
    /// `is_layout_word` accepts, when an overflow is installed (no layout
    /// describes its own keys, so a shape reader declines, as it did before
    /// step 3). `shapes::object_shape_stamp` agrees with it on every shadowed
    /// word; readers that handle the overflow themselves use
    /// [`layout_id_of_word`]. Restamps keep the bit ([`carry_shadow`]). One
    /// range compare on every unshadowed word; a shadowed one also reads
    /// `meta.overflow`.
    #[inline(always)]
    pub fn shape_word(&self) -> u32 {
        let w = self.parent_class_id;
        if word_is_shadowed(w) && !self.overflow_installed() {
            w & !LAYOUT_SHADOW_BIT
        } else {
            w
        }
    }

    /// Does this ordinary object carry an overflow (`overflow::install`)? Only
    /// meaningful for a word in the shadowed band, which only ordinary
    /// objects carry, so `meta` is theirs.
    #[inline(always)]
    pub fn overflow_installed(&self) -> bool {
        // SAFETY: only meaningful for a shadowed-band word, which only meta-form objects carry.
        let meta = unsafe {
            crate::meta_read!(crate::meta_obj_from_kind!(
                self as *const Self as *mut _,
                ()
            ))
        };
        !meta.is_null() && unsafe { (*meta).overflow } != 0
    }
}

/// The word a restamp from `previous` to `id` publishes: the only rule for
/// carrying [`LAYOUT_SHADOW_BIT`]. The bit summarizes the object's own
/// history (its overflow included: `overflow::install` sets it), so it
/// survives every restamp to an ordinary layout. A birth word that is not a
/// layout word yet (a class id, which may have bit 30 set) carries nothing.
/// Only ordinary-band objects carry the bit: overflows and dynamic writes
/// reach `GC_TYPE_OBJECT` receivers with ordinary layouts only.
#[inline(always)]
pub const fn carry_shadow(previous: u32, id: LayoutWord) -> LayoutWord {
    if word_is_shadowed(previous)
        && id.0 >= super::shapes::SHAPE_ID_BASE
        && id.0 < crate::codegen_abi::LAYOUT_ORDINARY_END
    {
        LayoutWord(id.0 | crate::codegen_abi::LAYOUT_SHADOW_BIT)
    } else {
        id
    }
}
// Every ShapeId lies in [0x8000_0000, 0xC000_0000): bit 30 is always clear.
const _: () = assert!(super::shapes::SHAPE_ID_BASE == 0x8000_0000);
const _: () = assert!(super::shapes::SHAPE_ID_END <= 0xC000_0000);

/// Section-ordered layout ids start here: the upper half of the static band.
/// The content-hash band (`assign_static_shape_ids`) stays in the lower half.
pub const LAYOUT_BASE: u32 =
    super::shapes::SHAPE_ID_BASE + crate::codegen_abi::STATIC_SHAPE_ID_COUNT / 2;
const _: () = assert!(LAYOUT_BASE == crate::codegen_abi::SHAPE_ID_BASE + (1 << 19));

/// `PerryLayoutDesc.proto`: a tag (address below [`LAYOUT_PROTO_TAG_END`]),
/// `%Object.prototype%` or null, or a `PerryProtoFamily`, the one prototype
/// every object of the layout inherits from: the singleton of an
/// `Object.create(P)` family (step 1), or a runtime prototype
/// (`runtime_layouts::*_PROTO_FAMILY`, whose serial is a reserved intrinsic one).
pub const LAYOUT_PROTO_OBJECT: *const PerryProtoFamily = std::ptr::null();
pub const LAYOUT_PROTO_NULL: *const PerryProtoFamily = std::ptr::without_provenance(1);
/// `%Object.prototype%` for a class family whose instances do not inherit
/// from the class (a compiler declared-keys record, `exports`); `OBJECT` (0)
/// there means the class's own prototype. A class family tagged `NULL` keeps
/// the class's shape identity and is born with the null-prototype flag
/// (`mark_layout_object_facts`).
pub const LAYOUT_PROTO_OBJECT_EXPLICIT: *const PerryProtoFamily = std::ptr::without_provenance(3);
/// `PerryLayoutDesc.proto` addresses at or above this are a
/// `*const PerryProtoFamily`.
pub const LAYOUT_PROTO_TAG_END: usize = 4096;

/// Object model step 1 (§3.2, §3.6): the prototype of an `Object.create(P)`
/// family whose `P` is a module's singleton literal (a top-level `const`
/// bound once per agent). Codegen emits one per such binding, writable, and
/// points the family's descriptors' `proto` at it.
///
/// `serial` is the prototype identity (`ObjectMeta::proto_serial`, the
/// receiver shapes' `proto_id`) every object of the family names: drawn once
/// from the dynamic serial counter by [`register_static_layouts`], before any
/// user code runs, so the family's layouts are registered with it. `P` takes
/// it at its birth ([`perry_bind_family_prototype`]); a birth of the family
/// ([`perry_create_family`]) checks the prototype it is handed carries it,
/// one compare, and otherwise takes the ordinary `Object.create`.
///
/// A runtime prototype's family is static data with its reserved intrinsic
/// serial already in place (registration keeps a serial it finds); the
/// runtime gives the prototype that serial when it builds it
/// (`builtin_proto::seal_runtime_prototype`, `name_runtime_prototype`), so an
/// object born on the layout needs no link.
pub use crate::codegen_abi::PerryProtoFamily;

/// `PerryLayoutDesc.variant` of a carried prototype layout (object model
/// §3.6): the layout `P` is born on ([`perry_bind_family_prototype`]), a
/// marked prototype's, registered under a semantic generation of its own.
pub const PROTOTYPE_IMAGE_VARIANT: u16 = crate::codegen_abi::LAYOUT_VARIANT_PROTOTYPE_IMAGE;

/// The family's prototype serial, drawing it on first use (registration).
unsafe fn proto_family_serial(fam: *const PerryProtoFamily) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    let word = AtomicU64::from_ptr(std::ptr::addr_of!((*fam).serial).cast_mut());
    let current = word.load(Ordering::Acquire);
    if current != 0 {
        return current;
    }
    let fresh = super::proto_validity::next_prototype_serial();
    match word.compare_exchange(0, fresh, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => fresh,
        Err(other) => other,
    }
}

/// The serial registration drew for `fam`, or 0 when no family layout of
/// this program names it.
#[inline]
unsafe fn proto_family_serial_registered(fam: *const PerryProtoFamily) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    AtomicU64::from_ptr(std::ptr::addr_of!((*fam).serial).cast_mut()).load(Ordering::Acquire)
}


/// One key of a layout's key sequence (enumeration order).
#[repr(C)]
pub struct PerryLayoutKey {
    pub atom: *const StaticAtom,
    pub slot: u32,
    pub attrs: u8,
    pub _pad: u8,
    /// The slot's absolute byte offset from the object (`16 + 8 * slot` today),
    /// so a reader adds it to the object instead of composing a slot base.
    pub off: u16,
}

/// `PerryLayoutKey.off` of slot `slot`; the same value codegen emits
/// (`target_layout::layout_key_off`).
pub const fn layout_key_off(slot: u32) -> u16 {
    let off = super::header::meta_form_alloc_bytes(slot as usize) as u32;
    assert!(off <= u16::MAX as u32);
    off as u16
}

/// `PerryLayoutDesc.slots_end` (and `alloc_bytes`) of a layout of `slot_count` slots.
pub const fn layout_slots_end(slot_count: u32) -> u32 {
    super::header::meta_form_alloc_bytes(slot_count as usize) as u32
}

/// `PerryLayoutDesc.gc_header_word`: the idle `GcHeader` image of a layout
/// birth. The live birth colour is ORed in at allocation.
pub const fn layout_gc_header_word(slot_count: u32) -> u64 {
    // SAFETY: GcHeader is eight initialized bytes with no padding.
    unsafe {
        std::mem::transmute::<crate::gc::GcHeader, u64>(crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_OBJECT,
            gc_flags: crate::gc::GC_FLAG_ARENA,
            _reserved: 0,
            size: crate::gc::GC_HEADER_SIZE as u32 + layout_slots_end(slot_count),
        })
    }
}

// Atom records and key sequences point only to immutable image data.
unsafe impl Sync for StaticAtom {}
unsafe impl Sync for PerryLayoutKey {}

/// One compiler-owned layout. Emitted by `perry-codegen/src/layout_desc.rs`;
/// the field order and sizes are pinned by the asserts below.
#[repr(C)]
pub struct PerryLayoutDesc {
    pub family: u32,
    pub variant: u16,
    /// `ShapeObjectKind` (0 = ordinary).
    pub kind: u8,
    pub integrity: u8,
    pub slot_count: u32,
    /// Emitted as [`LAYOUT_ID_CARRIED`] for a layout objects carry (then
    /// written by [`register_static_layouts`], 0 if this runtime cannot
    /// register it) and as 0 for static data no object carries.
    pub layout_id: u32,
    pub rep: u64,
    pub proto: *const PerryProtoFamily,
    pub keys: *const PerryLayoutKey,
    pub key_count: u32,
    /// Index capacity - 1 (a power of two minus one).
    pub index_mask: u32,
    /// Open-addressed: atom hash -> position in `keys`, `u32::MAX` = empty.
    pub index: *const u32,
    /// Case 2c: the class whose instances this static family layout
    /// describes (0: a literal family, which step 1 births).
    pub class_id: u32,
    /// The runtime parent class id the layout assumes (0: none).
    pub parent_class_id: u32,
    /// Immutable layout-owned method index, or null.
    pub methods: *const methods::PerryFamilyMethods,
    pub shape_id: u32,
    /// The static GC scan plan ([`crate::codegen_abi::layout_gc_plan`]): bit 31
    /// set and the slots that may hold a heap pointer in bits 0..=30, or 0 for
    /// the generic walk. Immutable image data emitted by codegen; the
    /// collectors read it for an object on this descriptor's own layout.
    pub gc_plan: u32,
    /// Immutable descriptor-owned canonical key-list cell; null for bindings
    /// whose keys are local to an evaluation, and on the wasm fallback arm.
    pub image_keys: *mut crate::array::ArrayHeader,
    /// A1 precompute, emitted by codegen under the asserts below. It has a reader in
    /// the birth: the idle `GcHeader` image ([`layout_gc_header_word`]).
    pub gc_header_word: u64,
    /// Keeps the size at 128 bytes (112 on ILP32), see `LAYOUT_DESC_FULL_SIZE_LP64`.
    pub _tail_pad: [u8; 32],
}

const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, gc_header_word)
        == if cfg!(target_pointer_width = "64") {
            crate::codegen_abi::LAYOUT_DESC_GC_HEADER_WORD_OFFSET_LP64
        } else {
            crate::codegen_abi::LAYOUT_DESC_GC_HEADER_WORD_OFFSET_ILP32
        }
);
const _: () =
    assert!(std::mem::offset_of!(PerryLayoutKey, off) == crate::codegen_abi::LAYOUT_KEY_OFF_OFFSET);

const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, image_keys)
        == if cfg!(target_pointer_width = "64") {
            80
        } else {
            64
        }
);
const _: () = assert!(
    std::mem::size_of::<PerryLayoutDesc>()
        == if cfg!(target_pointer_width = "32") {
            crate::codegen_abi::LAYOUT_DESC_FULL_SIZE_ILP32
        } else {
            crate::codegen_abi::LAYOUT_DESC_FULL_SIZE_LP64
        }
);
const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, class_id)
        == if cfg!(target_pointer_width = "32") {
            44
        } else {
            56
        }
);
const _: () = assert!(std::mem::offset_of!(PerryLayoutDesc, layout_id) == 12);
const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, keys)
        == if cfg!(target_pointer_width = "32") {
            28
        } else {
            32
        }
);
const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, index)
        == if cfg!(target_pointer_width = "32") {
            40
        } else {
            48
        }
);
const _: () = assert!(
    std::mem::size_of::<PerryLayoutKey>()
        == if cfg!(target_pointer_width = "32") {
            12
        } else {
            16
        }
);
const _: () = assert!(std::mem::size_of::<StaticAtom>() == 24);
const _: () = assert!(
    std::mem::offset_of!(StaticAtom, kind)
        == if cfg!(target_pointer_width = "32") {
            12
        } else {
            16
        }
);
const _: () = assert!(
    std::mem::offset_of!(StaticAtom, index)
        == if cfg!(target_pointer_width = "32") {
            16
        } else {
            20
        }
);

/// `PerryLayoutDesc.layout_id` as emitted for a layout some object carries:
/// [`register_static_layouts`] replaces it with the registered id. Every other
/// descriptor is emitted with 0 and stays 0.
pub const LAYOUT_ID_CARRIED: u32 = u32::MAX;
// The descriptor word has exactly three states: 0 (static data), the carried
// sentinel (not registered yet) and a registered ShapeId. The sentinel must
// never read as a ShapeId, or registration would install it as one.
#[allow(clippy::absurd_extreme_comparisons)] // the sentinel is u32::MAX today; the check guards a future change
const _: () = assert!(LAYOUT_ID_CARRIED >= super::shapes::SHAPE_ID_END);
const _: () = assert!(super::shapes::SHAPE_ID_BASE != 0);

/// The ShapeId a descriptor word holds, if registration already wrote one
/// (this agent or an earlier one); `None` for 0 and for the carried sentinel.
#[inline]
pub(crate) fn registered_layout_id(word: u32) -> Option<u32> {
    super::shapes::is_layout_word(word).then_some(word)
}

/// Read the immutable template word; the independent shape_id cell is atomic.
#[inline]
pub(crate) unsafe fn load_layout_id(desc: *const PerryLayoutDesc) -> u32 {
    (*desc).layout_id
}

/// A section sentinel's family: not a layout.
const SENTINEL_FAMILY: u32 = u32::MAX;

macro_rules! section_sentinel {
    ($name:ident, $ty:ty, $init:expr, $elf:literal, $mach:literal, $win:literal) => {
        section_sentinel!(@storage [mut], $name, $ty, $init, $elf, $mach, $win);
    };
    (@atom $name:ident, $ty:ty, $init:expr, $elf:literal, $mach:literal, $win:literal) => {
        // COFF splits read-only and writable inputs even with the same section
        // name. Atoms are immutable; match the runtime and emitted records.
        #[cfg(target_os = "windows")]
        section_sentinel!(@storage [], $name, $ty, $init, $elf, $mach, $win);
        #[cfg(not(target_os = "windows"))]
        section_sentinel!(@storage [mut], $name, $ty, $init, $elf, $mach, $win);
    };
    (@storage [$($mutable:tt)*], $name:ident, $ty:ty, $init:expr, $elf:literal, $mach:literal, $win:literal) => {
        // Always create the section (programs without layouts, Rust tests):
        // `used(linker)` keeps it, and `bounds` references it.
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = $mach)]
        #[cfg_attr(target_os = "windows", link_section = $win)]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows")),
            link_section = $elf
        )]
        static $($mutable)* $name: $ty = $init;
    };
}

section_sentinel!(
    LAYOUT_SECTION_SENTINEL,
    PerryLayoutDesc,
    PerryLayoutDesc {
        image_keys: std::ptr::null_mut(),
        shape_id: 0,
        gc_plan: 0,
        family: SENTINEL_FAMILY,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 0,
        layout_id: 0,
        rep: 0,
        proto: LAYOUT_PROTO_OBJECT,
        keys: std::ptr::null(),
        key_count: 0,
        index_mask: 0,
        index: std::ptr::null(),
        class_id: 0,
        parent_class_id: 0,
        methods: std::ptr::null(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        _tail_pad: [0; 32],
    },
    "perry_layouts",
    "__DATA,__perry_layouts",
    ".perrylay$m"
);
section_sentinel!(
    @atom ATOM_SECTION_SENTINEL,
    StaticAtom,
    StaticAtom {
        hash: 0,
        string: std::ptr::null(),
        kind: crate::codegen_abi::ATOM_KIND_STRING,
        _pad: [0; 3],
        index: 0,
    },
    "perry_atoms",
    "__DATA,__perry_atoms",
    ".perryatm$m"
);


#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrylay$a"]
// COFF groups sections by their writable characteristic before sorting the
// $ suffix. Match the mutable descriptors in .perrylay$m, or these bounds
// delimit a separate read-only section and registration misses every layout.
static mut LAYOUT_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrylay$z"]
static mut LAYOUT_END: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perryatm$a"]
static ATOM_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perryatm$z"]
static ATOM_END: [u64; 1] = [0];

/// (start, end) of the layout section.
fn layout_bounds() -> (usize, usize) {
    std::hint::black_box(std::ptr::addr_of!(LAYOUT_SECTION_SENTINEL) as usize);
    #[cfg(target_vendor = "apple")]
    {
        unsafe extern "C" {
            #[link_name = "\u{1}section$start$__DATA$__perry_layouts"]
            static START: u8;
            #[link_name = "\u{1}section$end$__DATA$__perry_layouts"]
            static END: u8;
        }
        (
            std::ptr::addr_of!(START) as usize,
            std::ptr::addr_of!(END) as usize,
        )
    }
    #[cfg(target_os = "windows")]
    {
        (
            std::ptr::addr_of!(LAYOUT_START) as usize + 8,
            std::ptr::addr_of!(LAYOUT_END) as usize,
        )
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "windows")))]
    {
        unsafe extern "C" {
            static __start_perry_layouts: u8;
            static __stop_perry_layouts: u8;
        }
        (
            std::ptr::addr_of!(__start_perry_layouts) as usize,
            std::ptr::addr_of!(__stop_perry_layouts) as usize,
        )
    }
}


/// Every record of a section, in section order (the linker may pad between
/// input sections; records are 8-aligned and a zero word never starts one).
fn records<T>(bounds: (usize, usize)) -> impl Iterator<Item = *mut T> {
    let (start, end) = bounds;
    let size = std::mem::size_of::<T>();
    let count = if end > start { (end - start) / size } else { 0 };
    (0..count).map(move |i| (start + i * size) as *mut T)
}



/// The agent-independent well-known symbol a kind-1 atom is bound to.
pub(crate) unsafe fn well_known_atom_symbol(
    atom: *const StaticAtom,
) -> Option<*mut crate::symbol::SymbolHeader> {
    if (*atom).kind != crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL {
        return None;
    }
    let name = crate::codegen_abi::WELL_KNOWN_SYMBOLS.get((*atom).index as usize)?;
    Some(crate::symbol::well_known_symbol(name))
}

/// The registered symbol a kind-3 atom names: `Symbol.for` of its string,
/// the one symbol of that key in every agent's registry.
unsafe fn registered_atom_symbol(
    atom: *const StaticAtom,
) -> Option<*mut crate::symbol::SymbolHeader> {
    if (*atom).kind != crate::codegen_abi::ATOM_KIND_REGISTERED_SYMBOL || (*atom).string.is_null() {
        return None;
    }
    let key = crate::value::JSValue::string_ptr((*atom).string.cast_mut()).bits();
    let symbol = crate::symbol::js_symbol_for(f64::from_bits(key)).to_bits();
    Some((symbol & crate::value::POINTER_MASK) as *mut crate::symbol::SymbolHeader)
}

/// The symbol a symbol atom names (well-known, node's own, registered or an
/// image cell), `None` for a string atom. A string key, the common atom,
/// leaves after one compare instead of failing each symbol kind's test.
pub(crate) unsafe fn atom_symbol(
    atom: *const StaticAtom,
) -> Option<*mut crate::symbol::SymbolHeader> {
    if (*atom).kind == crate::codegen_abi::ATOM_KIND_STRING {
        return None;
    }
    well_known_atom_symbol(atom)
        .or_else(|| super::runtime_layouts::runtime_symbol_of_atom(atom))
        .or_else(|| registered_atom_symbol(atom))
        .or_else(|| image_atom_symbol(atom))
}

/// An image symbol atom names its static `SymbolHeader` directly.
#[cold]
unsafe fn image_atom_symbol(atom: *const StaticAtom) -> Option<*mut crate::symbol::SymbolHeader> {
    ((*atom).kind == crate::codegen_abi::ATOM_KIND_IMAGE_SYMBOL && !(*atom).string.is_null())
        .then(|| (*atom).string.cast_mut().cast())
}


mod atoms;
#[cfg(test)]
use atoms::image_atoms;
pub(crate) use atoms::{atom_bytes, coloured_atoms, seed_static_atoms, shadow_name_descriptor};
pub use atoms::StaticAtom;

#[path = "layout_desc/lazy_image.rs"]
mod lazy_image;
#[path = "layout_desc/registration.rs"]
mod registration;
use registration::binding_symbol_keys;
use registration::declared_kind;
pub(crate) use registration::layout_word_for_birth;

/// Runtime layouts this agent installed (test builds).
#[cfg(test)]
pub(crate) fn registration_installs() -> u32 {
    registration::RUNTIME_LAYOUT_INSTALLS.with(|n| n.get())
}
pub(crate) use registration::layout_key_word;
pub(crate) use registration::register_function_layout;
pub(crate) use registration::register_namespace_layout;
pub(crate) use registration::register_one;
pub(crate) use registration::register_one_bound;
#[cfg(test)]
pub(crate) use registration::register_static_layouts;
pub(crate) use registration::register_static_layouts_in;

/// Allocate an object of a registered layout: the header, `slot_count`
/// slots holding the hole marker, and the layout word. The caller (a static
/// family's birth, step 1) stores every slot its layout's shape names as
/// present before the object can be observed.
///
/// # Safety
/// `desc` is a registered descriptor of this program.
#[no_mangle]
pub unsafe extern "C" fn perry_alloc_layout(desc: *const PerryLayoutDesc) -> *mut ObjectHeader {
    #[cfg(test)]
    crate::test_support::bootstrap_test_thread();
    let d = &*desc;
    let Some(id) = layout_word_for_birth(desc) else {
        eprintln!(
            "Perry internal error: perry_alloc_layout of an unregistered layout (family {:#x})",
            d.family
        );
        std::process::abort();
    };
    alloc_layout_object(d, LayoutWord(id))
}

/// Allocate an instance of class family layout `desc` born on `copy`, a class
/// evaluation's copy of that layout (`shapes::evaluation_family_desc_index_plus_one(copy)`
/// names `desc`'s layout): the family's slots, all holes, under the ShapeId
/// that names the evaluation's prototype and brand. The caller read `copy`
/// from the evaluation's class object and checked it. Can collect.
///
/// # Safety
/// `desc` is a registered class family descriptor of this program.
pub(crate) unsafe fn alloc_family_layout_copy(
    desc: *const PerryLayoutDesc,
    copy: LayoutWord,
) -> *mut ObjectHeader {
    alloc_layout_object(&*desc, copy)
}

/// Allocate an object of the registered layout `id` of `d` (see
/// [`perry_alloc_layout`]). Can collect.
unsafe fn alloc_layout_object(d: &PerryLayoutDesc, id: LayoutWord) -> *mut ObjectHeader {
    // The descriptor carries the header image (with the size) and where the slots start.
    let obj = crate::arena::arena_alloc_gc_object_word(&d.gc_header_word, d.slot_count)
        .cast::<ObjectHeader>();
    finish_layout_object(
        obj,
        d,
        id,
        super::header::meta_form_alloc_bytes(0) as u32,
        d.slot_count,
    )
}

/// Function births alone retain the ordinary inline floor. Keep their
/// sizing and diagnostic work out of the shared class/literal allocator.
/// `desc` is the registered layout admitted by function_family_birth.
pub(crate) unsafe fn alloc_function_layout(
    desc: *const PerryLayoutDesc,
    id: LayoutWord,
) -> *mut ObjectHeader {
    let d = &*desc;
    let obj =
        alloc_layout_object_sized(d, id, (d.slot_count as usize).max(super::INLINE_SLOT_FLOOR));
    FAMILY_DIAG
        .births
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    obj
}

#[inline(always)]
unsafe fn alloc_layout_object_sized(
    d: &PerryLayoutDesc,
    id: LayoutWord,
    physical_slots: usize,
) -> *mut ObjectHeader {
    // A birth writes only holes; constructor stores check the live chain.
    let size = super::header::meta_form_alloc_bytes(physical_slots);
    let obj =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT).cast::<ObjectHeader>();
    finish_layout_object(
        obj,
        d,
        id,
        super::header::meta_form_alloc_bytes(0) as u32,
        physical_slots as u32,
    )
}

/// Header, holes and shape facts of a layout birth whose slot region is
/// `slots_first` (a byte offset from the object) and `slots` long.
#[inline(always)]
unsafe fn finish_layout_object(
    obj: *mut ObjectHeader,
    d: &PerryLayoutDesc,
    id: LayoutWord,
    slots_first: u32,
    slots: u32,
) -> *mut ObjectHeader {
    let word = id;
    // GC_STORE_AUDIT(INIT): unpublished header and hole-only declared slots.
    std::ptr::write(
        obj,
        ObjectHeader {
            class_id: d.class_id,
            parent_class_id: word.0,
            meta: std::ptr::null_mut(),
            #[cfg(target_pointer_width = "32")]
            _slot_alignment_padding: 0,
        },
    );
    let fields = obj.cast::<u8>().add(slots_first as usize).cast::<u64>();
    for i in 0..slots as usize {
        // GC_STORE_AUDIT(POINTER_FREE): All three stores write the immediate TAG_HOLE to physical object slots. They introduce no pointer/child edge; allocation or rooted object custody surrounds the fill and shape publication follows.
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    mark_layout_object_facts(obj, d);
    // The unpublished header already carries this declared layout. Birth
    // creates carrier duty, not a structural mutation: it must not enter
    // the mutation publisher (which can inspect lazily materialized keys).
    if !crate::arena::pointer_in_nursery(obj as usize) {
        super::shapes::note_old_generation_carrier(super::shapes::shape_record_for_word(id));
    }
    super::shapes::store_kind::check_store_facts(obj);
    if d.class_id != 0 {
        FAMILY_DIAG
            .births
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    obj
}

/// The per-object record a layout-born object carries before its shape is
/// stamped: plain ordinary, null prototype, descriptors and integrity, all
/// from the descriptor. Every allocator that stamps a layout shape calls it,
/// so the shape kind and the record agree (R6).
///
/// # Safety
/// `obj` is a freshly allocated, unpublished `GC_TYPE_OBJECT` born on `d`.
unsafe fn mark_layout_object_facts(obj: *mut ObjectHeader, d: &PerryLayoutDesc) {
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
    match declared_kind(d.kind) {
        Some(super::shapes::ShapeObjectKind::OrdinaryUnmarked) => {}
        Some(super::shapes::ShapeObjectKind::CallableOrdinary) => {
            (*gc)._reserved |= crate::gc::GC_OBJECT_CALLABLE;
        }
        _ => {
            super::shapes::store_kind::premark_plain_ordinary(obj);
            if d.kind == super::shapes::ShapeObjectKind::Class.code() as u8 {
                (*gc)._reserved |= crate::gc::GC_OBJECT_CALLABLE;
            }
        }
    }
    if d.proto == LAYOUT_PROTO_NULL {
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
    }
    if d.integrity != 0
        || (d.key_count != 0
            && std::slice::from_raw_parts(d.keys, d.key_count as usize)
                .iter()
                .any(|key| key.attrs & !LAYOUT_KEY_LATE != 0))
    {
        (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
    }
    (*gc)._reserved |= d.integrity as u16;
}

/// An object of `new F` born on instance layout `d`'s keys, every slot the
/// hole, whose prototype is `F.prototype` (identity `proto_id`, the value
/// `proto` holds) rather than the sealed node prototype `d` names: the
/// function-constructed instance with a native ancestor
/// (`instance_birth::derived_layout`). Its shape is `d`'s key list with that
/// identity, from the shape tree; `recorded` (the prototype's recorded birth
/// shape) answers when it is exactly that shape. `None` when `d` is not
/// registered. Can collect; `proto` stays rooted by the caller.
///
/// # Safety
/// `d` is a runtime instance layout; `proto_id` is the identity of the
/// object `proto` holds.
pub(crate) unsafe fn alloc_derived_layout_object(
    d: &PerryLayoutDesc,
    proto_id: u64,
    proto: &crate::gc::RuntimeHandle<'_>,
    recorded: LayoutWord,
) -> Option<*mut ObjectHeader> {
    let id = layout_word_for_birth(d)?;
    let size = super::header::meta_form_alloc_bytes(d.slot_count as usize);
    let obj =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT).cast::<ObjectHeader>();
    // No collection from here until the newborn carries its shape.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let base = super::shapes::shape_descriptor_for_word(LayoutWord(id))?;
    let proto_bits = proto.get_nanbox_u64();
    if super::shapes::identity_prototype_word(proto_id) != proto_bits {
        super::shapes::write_identity_word(proto_id, proto_bits);
    }
    let shape = match super::shapes::shape_keys_and_proto_for_word(recorded) {
        Some((keys, count, recorded_proto))
            if recorded != LayoutWord(0)
                && keys == base.keys
                && count == base.logical_key_count
                && recorded_proto == proto_id =>
        {
            recorded
        }
        _ => super::shapes::publish_shape_result(super::shapes::shape_descriptor_intern_with_rep(
            base.keys_view(),
            base.logical_key_count,
            base.live_inline_slot_count,
            0,
            base.object_kind,
            base.hole_count,
            proto_id,
            base.summary,
            base.rep,
            &[],
            None,
            base.integrity_flags,
        )),
    };
    // GC_STORE_AUDIT(INIT): unpublished header and hole-only declared slots.
    std::ptr::write(
        obj,
        ObjectHeader {
            class_id: 0,
            parent_class_id: shape.0,
            meta: std::ptr::null_mut(),
            #[cfg(target_pointer_width = "32")]
            _slot_alignment_padding: 0,
        },
    );
    let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj));
    for i in 0..d.slot_count as usize {
        // GC_STORE_AUDIT(POINTER_FREE): All three stores write the immediate TAG_HOLE to physical object slots. They introduce no pointer/child edge; allocation or rooted object custody surrounds the fill and shape publication follows.
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    mark_layout_object_facts(obj, d);
    if !crate::arena::pointer_in_nursery(obj as usize) {
        super::shapes::note_old_generation_carrier(super::shapes::shape_record_for_word(shape));
    }
    // The prototype edge lives outside the heap, as for every created birth.
    crate::gc::runtime_shade_external_edge(proto_bits);
    super::shapes::store_kind::check_store_facts(obj);
    Some(obj)
}

/// A builtin prototype holder born on a registered runtime layout (§3.6:
/// builtin prototypes are families with runtime-defined static layouts). Like
/// [`perry_alloc_layout`], but born old and never moving, as every builtin
/// prototype holder is, with `room` inline slots for the keys population adds
/// after the layout's own. The caller stores every layout slot before user
/// code runs.
///
/// # Safety
/// `desc` is a registered runtime descriptor whose prototype is
/// `%Object.prototype%` or null.
pub(crate) unsafe fn alloc_layout_prototype(
    desc: *const PerryLayoutDesc,
    room: u32,
) -> *mut ObjectHeader {
    let d = &*desc;
    let Some(id) = layout_word_for_birth(d) else {
        eprintln!(
            "Perry internal error: a prototype born on an unregistered layout (family {:#x})",
            d.family
        );
        std::process::abort();
    };
    let room = room.max(d.slot_count);
    let obj = if d.proto == LAYOUT_PROTO_NULL {
        super::object_alloc_prototype_old_null_proto(0, room)
    } else {
        super::object_alloc_prototype_old(0, room)
    };
    mark_layout_object_facts(obj, d);
    // A birth: the bit is carried, never admitted.
    super::shapes::stamp_birth_shape_id_with_carrier_note(obj, LayoutWord(id));
    let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj));
    for i in 0..d.slot_count as usize {
        // GC_STORE_AUDIT(INIT): the hole marker is a scalar.
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    // Every birth on this layout is a prototype: marked now, it never leaves
    // the layout for a private lineage.
    let _no_move = crate::gc::GcSuppressScope::new();
    super::proto_validity::mark_born_prototype(obj);
    obj
}

/// The position of `atom` in `desc`'s key sequence, or -1 (§3.7's probe of
/// the layout's immutable key index).
///
/// # Safety
/// `desc` is a descriptor of this program; `atom` a static atom.
#[no_mangle]
pub unsafe extern "C" fn perry_layout_index_probe(
    desc: *const PerryLayoutDesc,
    atom: *const StaticAtom,
) -> i32 {
    let d = &*desc;
    if d.index.is_null() || atom.is_null() {
        return -1;
    }
    let mask = d.index_mask as u64;
    let mut at = (*atom).hash & mask;
    for _ in 0..=mask {
        let pos = *d.index.add(at as usize);
        if pos == u32::MAX {
            return -1;
        }
        if pos < d.key_count && (*d.keys.add(pos as usize)).atom == atom {
            return pos as i32;
        }
        at = (at + 1) & mask;
    }
    -1
}

/// Object model step 1: the birth of `P`, the singleton prototype of the
/// `Object.create(P)` families naming `fam`. `P` takes the family's serial as
/// its prototype identity (the mark every `Object.create(P)` makes, with the
/// serial the family's layouts were registered under), and the identity's
/// word names it. Codegen calls this once, right after the module-level
/// `const` binding `P` is initialized, before any code can read `P`, and
/// stores the value it returns into the binding.
///
/// `image` is `P`'s carried layout (object model §3.6, decision 147), or
/// null when the compiler derived none: every own key of the object the
/// initializer builds, in order, with its attributes, and its
/// [[Prototype]]. When the object is exactly that ([`prototype_image`]), `P`
/// is born on the layout and the value returned is the new object; a family
/// add then proves its chain with one compare of `P`'s layout word. Anything
/// else (a spread source that changed, a key the compiler did not predict)
/// leaves `P` as built, on a word no proof names.
///
/// `symbols` holds the values of the module-local symbol bindings the
/// image's `ATOM_KIND_BINDING_SYMBOL` keys name, `count` of them in key
/// order: the image's layout registers here, with those symbols as its keys
/// (its id was reserved at startup).
///
/// # Safety
/// `fam` is a `PerryProtoFamily` of this program; `image` null or one of its
/// descriptors; `symbols` points at `count` values.
#[no_mangle]
pub unsafe extern "C" fn perry_bind_family_prototype(
    fam: *const PerryProtoFamily,
    proto: f64,
    image: *const PerryLayoutDesc,
    symbols: *const f64,
    count: u32,
) -> f64 {
    let serial = proto_family_serial_registered(fam);
    let bound: Vec<u64> = if count == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(symbols, count as usize)
            .iter()
            .map(|v| v.to_bits())
            .collect()
    };
    let proto = match image.as_ref() {
        Some(d) if serial != 0 => prototype_image(d, proto, serial, &bound).unwrap_or(proto),
        _ => proto,
    };
    let bits = proto.to_bits();
    if serial == 0 || bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return proto;
    }
    let p = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(p) {
        return proto;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    // The mark can allocate its meta record and move `P`.
    let marked = super::proto_validity::mark_object_as_prototype_with_serial(p, serial);
    if marked == Some(serial) {
        super::shapes::write_identity_word(serial, proto.get_nanbox_u64());
    }
    f64::from_bits(proto.get_nanbox_u64())
}

/// `P` on its carried layout `d`: a new object of the layout holding the
/// values of `built`, when `built`'s own properties are exactly `d`'s (each
/// key at its position with its attributes, nothing past them: no overflow,
/// no elements, no boxed primitive or brand) and its [[Prototype]] and
/// extensibility are the layout's. Nothing else can hold `built`: the
/// binding is in its dead zone until the caller stores the result, and the
/// initializer's own code never sees the object it builds (a source's
/// getter runs with the source as `this`). The copy is a birth, so the new
/// object's word carries no shadow bit: every key on it is one the compiler
/// named, with the value the initializer stored. `None` leaves `P` as built.
///
/// `bound` holds the key words of `d`'s module-local symbol keys; each must
/// be a symbol, and `d` registers with them.
///
/// # Safety
/// `d` is a descriptor of this program.
pub(crate) unsafe fn prototype_image(
    d: &PerryLayoutDesc,
    built: f64,
    serial: u64,
    bound: &[u64],
) -> Option<f64> {
    let id = layout_word_for_birth(d)?;
    if bound.len() != binding_symbol_keys(d)
        || !bound
            .iter()
            .all(|&w| crate::symbol::js_is_symbol(f64::from_bits(w)) != 0)
        || !image_matches(d, built, bound)
    {
        return None;
    }
    if !bound.is_empty() && register_one_bound(d, (*d).shape_id, bound) != id {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let built = scope.root_nanbox_f64(built);
    let image = scope.root_raw_mut_ptr(alloc_layout_object(d, LayoutWord(id)));
    // Born a marked prototype (the layout is a marked one's): its meta
    // record carries the mark and the family's serial, so the binding's mark
    // finds it done and makes no transition.
    let meta = image.with_mut_ptr(|p: *mut ObjectHeader| {
        super::object_meta_ensure(
            crate::meta_obj_runtime_born!(p),
            super::MetaReason::BecomesPrototype,
        )
    });
    if meta.is_null() {
        return None;
    }
    // GC_STORE_AUDIT(POINTER_FREE): a scalar classification bit and serial.
    (*meta).flags |= super::OBJECT_META_FLAG_IS_PROTOTYPE;
    (*meta).proto_serial = serial;
    image
        .with_mut_ptr(|p: *mut ObjectHeader| crate::gc::never_move::request_never_move(p as usize));
    let _no_move = crate::gc::GcSuppressScope::new();
    let obj = (built.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ObjectHeader;
    let (keys, live) = super::object_keys_and_live_slot_count(obj);
    let image = image.with_mut_ptr(|p: *mut ObjectHeader| p);
    for (pos, key) in std::slice::from_raw_parts(d.keys, d.key_count as usize)
        .iter()
        .enumerate()
    {
        let pos = pos as u32;
        // An accessor's slot holds its pair; the image's key is one too.
        let value = if super::key_attrs::key_is_accessor_at(keys.arr(), pos) {
            let slot = super::key_attrs::key_storage_slot(keys.arr(), pos);
            super::field_get_set::object_field_at_with_live(obj, slot, live).bits()
        } else {
            super::key_attrs::object_slot_data(obj, pos).bits()
        };
        let slot_addr = crate::meta_slots_of!(crate::meta_obj_runtime_born!(image))
            .add(key.slot as usize) as usize;
        crate::gc::runtime_store_object_jsvalue_slot(
            image as usize,
            slot_addr,
            key.slot as usize,
            value,
        );
    }
    Some(crate::value::js_nanbox_pointer(image as i64))
}

// The image's attribute bytes are the key list's entry bits.
const _: () = {
    use super::key_attrs as k;
    use crate::codegen_abi as abi;
    assert!(k::ENTRY_NON_WRITABLE == abi::LAYOUT_KEY_NON_WRITABLE);
    assert!(k::ENTRY_NON_ENUMERABLE == abi::LAYOUT_KEY_NON_ENUMERABLE);
    assert!(k::ENTRY_NON_CONFIGURABLE == abi::LAYOUT_KEY_NON_CONFIGURABLE);
    assert!(k::ENTRY_ACCESSOR == abi::LAYOUT_KEY_ACCESSOR);
    assert!(k::ENTRY_HAS_GET == abi::LAYOUT_KEY_HAS_GET);
    assert!(k::ENTRY_HAS_SET == abi::LAYOUT_KEY_HAS_SET);
    assert!(k::PRIVATE_FIELD_ENTRY == abi::LAYOUT_KEY_PRIVATE_FIELD);
};

/// Are `built`'s own properties and [[Prototype]] exactly `d`'s?
unsafe fn image_matches(d: &PerryLayoutDesc, built: f64, bound: &[u64]) -> bool {
    let bits = built.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let p = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(p) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(p) else {
        return false;
    };
    if crate::object_lacks_meta_word!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & super::shapes::INTEGRITY_FLAGS != 0
    {
        return false;
    }
    let obj = p as *const ObjectHeader;
    let meta = crate::meta_read!(crate::meta_obj_from_type!(obj as *mut _, header.obj_type));
    if !meta.is_null()
        && ((*meta).boxed_primitive != 0
            || (*meta).reflect_metadata != 0
            || (*meta).private_evaluation_brand != 0)
    {
        return false;
    }
    if !super::overflow::table(obj).is_null()
        || super::shapes::object_shape_stamp(obj) == LayoutWord(0)
    {
        return false;
    }
    let (keys, _) = super::object_keys_and_live_slot_count(obj);
    if keys.is_null() != (d.key_count == 0) || keys.count() != d.key_count {
        return false;
    }
    const FACTS: u8 = super::key_attrs::ENTRY_ATTR_MASK | super::key_attrs::ENTRY_ACCESSOR_MASK;
    let mut next = 0;
    for (pos, key) in (0..d.key_count as usize).map(|i| (i, &*d.keys.add(i))) {
        let words = crate::array::array_elements_ptr(keys.arr());
        let want = layout_key_word(key, bound, &mut next);
        if *words.add(pos) != want
            || super::key_attrs::keys_entry(keys.arr(), pos as u32) & FACTS != key.attrs & FACTS
        {
            return false;
        }
    }
    let proto = super::js_object_get_prototype_of(built).to_bits();
    match d.proto.addr() {
        0 => {
            let object = crate::agent_ptrs::intrinsic_proto(
                super::builtin_proto::BuiltinProto::Object as usize,
            );
            proto & crate::value::POINTER_MASK == object as u64
                && proto & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        }
        1 => proto == crate::value::TAG_NULL,
        _ => {
            let serial = proto_family_serial_registered(d.proto);
            let q = (proto & crate::value::POINTER_MASK) as *const ObjectHeader;
            // A1_UNPROVEN: record-first: the prototype word q before its type test (kind-A/B question for S0f).
            serial != 0
                && proto & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                && crate::value::addr_class::is_above_handle_band(q as usize)
                && !crate::meta_read!(crate::meta_obj_runtime_born!(q as *mut _)).is_null()
                && (*crate::meta_read!(crate::meta_obj_runtime_born!(q as *mut _))).proto_serial
                    == serial
        }
    }
}

/// Object model step 1: `Object.create(P)` at a creation point whose family
/// layout `desc` the compiler fixed (§3.2). When `P` is the family's
/// singleton prototype (it carries the serial of `desc.proto`), the object is
/// born on `desc`'s layout: every slot holds the hole marker and the caller
/// stores each one before the object can be observed. Any other `P` (not
/// bound yet, a different object, a primitive, null) takes the ordinary
/// `Object.create`, which also throws its TypeError. The caller tells the
/// two apart by the layout word.
///
/// # Safety
/// `desc` is a descriptor of this program whose `proto` names a
/// `PerryProtoFamily`.
#[no_mangle]
pub unsafe extern "C" fn perry_create_family(desc: *const PerryLayoutDesc, proto: f64) -> f64 {
    let d = &*desc;
    if let Some(obj) = family_birth(d, proto) {
        return crate::value::js_nanbox_pointer(obj as i64);
    }
    super::js_object_create(proto)
}

unsafe fn family_birth(d: &PerryLayoutDesc, proto: f64) -> Option<*mut ObjectHeader> {
    if d.proto.addr() < LAYOUT_PROTO_TAG_END {
        return None;
    }
    let id = layout_word_for_birth(d)?;
    let serial = proto_family_serial_registered(d.proto);
    let bits = proto.to_bits();
    if serial == 0 || bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let p = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(p) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(p)?;
    if crate::object_lacks_meta_word!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let meta = crate::meta_read!(crate::meta_obj_from_type!(
        p as *const ObjectHeader as *mut _,
        header.obj_type,
    ));
    if meta.is_null() || (*meta).proto_serial != serial {
        return None;
    }
    // The identity names one prototype for its whole life; refill only an
    // emptied word (`stamp_known_prototype_transition`).
    if super::shapes::identity_prototype_word(serial) != bits {
        super::shapes::write_identity_word(serial, bits);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let obj = alloc_layout_object(d, LayoutWord(id));
    // The prototype edge lives outside the heap. A black newborn must still
    // shade it during an incremental mark (`object_alloc_created`).
    crate::gc::runtime_shade_external_edge(proto.get_nanbox_u64());
    Some(obj)
}

/// `PERRY_CHECK_CONTRACTS=1`: the witness of a family store the compiler
/// emitted without `[[Set]]` (an `Object.create(P)` family's birth or key
/// add, object model step 1). `obj` was found on the family layout `desc`:
/// its [[Prototype]] must carry the family's serial, and no key of `desc`
/// may be intercepted on its prototype chain (the census answer the store
/// relies on).
///
/// # Safety
/// `desc` is a family descriptor of this program; `obj` any value.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_create_family_contract_check(
    desc: *const PerryLayoutDesc,
    obj: f64,
) {
    let d = &*desc;
    let bits = obj.to_bits();
    let violated = |why: &str, key: &[u8]| -> ! {
        crate::entry_contracts::contract_violated(
            "create_family_store",
            "prototype carries the family serial && no layout key intercepted on the chain",
            "prototype-write-census",
            bits,
            &format!(
                " family={:#x} key={} why={why}",
                d.family,
                String::from_utf8_lossy(key)
            ),
        )
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let serial = if d.proto.addr() >= LAYOUT_PROTO_TAG_END {
        proto_family_serial_registered(d.proto)
    } else {
        0
    };
    let proto = super::js_object_get_prototype_of(obj).to_bits();
    let p = (proto & crate::value::POINTER_MASK) as *const ObjectHeader;
    // A1_UNPROVEN: record-first: the prototype word of obj (kind-A/B question for S0f).
    if serial == 0
        || proto & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || crate::meta_read!(crate::meta_obj_runtime_born!(p as *mut _)).is_null()
        || (*crate::meta_read!(crate::meta_obj_runtime_born!(p as *mut _))).proto_serial != serial
    {
        violated("prototype is not the family's", b"");
    }
    let keys = if d.key_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(d.keys, d.key_count as usize)
    };
    for key in keys {
        let atom = key.atom;
        let Some(name) = atom_bytes(atom) else {
            continue;
        };
        let k = f64::from_bits(crate::value::JSValue::string_ptr((*atom).string.cast_mut()).bits());
        if super::plain_custom_prototype_may_intercept(
            bits as usize & crate::value::POINTER_MASK as usize,
            k,
        ) {
            violated("the key is intercepted on the prototype chain", name);
        }
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_CREATE_FAMILY_CONTRACT_CHECK: unsafe extern "C-unwind" fn(
    *const PerryLayoutDesc,
    f64,
) = js_create_family_contract_check;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_CREATE_FAMILY: unsafe extern "C" fn(*const PerryLayoutDesc, f64) -> f64 =
    perry_create_family;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_BIND_FAMILY_PROTOTYPE: unsafe extern "C" fn(
    *const PerryProtoFamily,
    f64,
    *const PerryLayoutDesc,
    *const f64,
    u32,
) -> f64 = perry_bind_family_prototype;
// ---------------------------------------------------------------------------
// Case 2c (OBJECT_MODEL_2026-10-13.md §3.7): static class families.
//
// The whole-image ordinal plan owns immutable DPA header words. Each agent's
// current ShapeRecord names its descriptor through family_desc_index_plus_one;
// the checked decoder spans eager nominal and lazy literal descriptors. ShapeId
// identities remain separate from both the dense row and descriptor position.
//
// A static family's layout word never changes after birth. A key its layout
// does not hold goes to the object's overflow (`object/overflow.rs`, §3.4):
// the first such key installs it ([`route_family_add`]), and the overflow's
// routing takes every later one. The family's slots stay where they are, so
// the case-2c site serves the object with or without an overflow.
// ---------------------------------------------------------------------------

/// Descriptor record indices are below this (the static band's upper half).
const LAYOUT_INDEX_LIMIT: u32 = crate::codegen_abi::STATIC_SHAPE_ID_COUNT / 2;

/// The layout id of descriptor record `index`.
#[inline]
fn layout_id_of_index(index: u32) -> Option<u32> {
    (index < LAYOUT_INDEX_LIMIT).then_some(LAYOUT_BASE + index)
}

/// The current agent record owns the descriptor index. The header word is
/// the independent DPA row; generic words receive one checked family decode.
#[inline]
fn family_for_word(word: u32) -> Option<*const PerryLayoutDesc> {
    if !super::shapes::is_layout_word(layout_id_of_word(word)) {
        return None;
    }
    let record = super::shapes::shape_record_for_word(LayoutWord(word))?;
    literal_birth::family_at(record.family_desc_index_plus_one().checked_sub(1)?)
}

/// The class declaration owns the immutable pointer to its instance layout.
/// No descriptor registry or learned class-to-layout mapping.
pub(crate) fn family_of_class(class_id: u32) -> Option<(u32, *const PerryLayoutDesc)> {
    let declaration = super::class_registry::declarations::class_declaration(class_id)?;
    let desc = declaration.instance_layout;
    if desc.is_null() {
        return None;
    }
    let (start, end) = layout_bounds();
    let address = desc as usize;
    if address < start || address >= end {
        return None;
    }
    let index = ((address - start) / std::mem::size_of::<PerryLayoutDesc>()) as u32;
    let parent = unsafe { (*desc).parent_class_id };
    let actual = super::class_meta_registry::get_parent_class_id(class_id).unwrap_or(0);
    (actual == parent).then_some((index, desc))
}

/// Is `d` a layout of class family `class_id` or of a family descending
/// from it (its runtime parents, through their families)?
pub(crate) fn family_descends(d: &PerryLayoutDesc, class_id: u32) -> bool {
    if d.class_id == class_id {
        return true;
    }
    let mut parent = d.parent_class_id;
    for _ in 0..64 {
        if parent == 0 {
            return false;
        }
        if parent == class_id {
            return true;
        }
        parent = match family_of_class(parent) {
            // SAFETY: a registered descriptor of this program.
            Some((_, pd)) => unsafe { (*pd).parent_class_id },
            None => return false,
        };
    }
    false
}

/// A family's creation split (object model §3.2: a family is a creation
/// point), emitted by codegen beside the family's descriptors
/// (`perry-codegen` `layout_desc::emit_module_layouts`): a birth whose
/// constructor argument `param` is a number equal to an entry's constant is
/// born on that entry's layout. Static data, never written.
#[repr(C)]
pub struct PerryFamilyVariants {
    pub param: u32,
    pub count: u32,
    /// `count` entries, sorted by `key_bits`.
    pub entries: [PerryFamilyVariant; 0],
}

/// One layout of a creation split: the constant (an f64's bits) and its
/// descriptor.
#[repr(C)]
pub struct PerryFamilyVariant {
    pub key_bits: u64,
    pub desc: *const PerryLayoutDesc,
}

/// The descriptor a birth of class `class_id` with constructor arguments
/// `args` is born on: its family's creation-split layout for the argument's
/// constant, else the family layout; `None` when the class has no family.
/// Every layout of the family is a correct layout of every instance (each
/// key is a hole until stored), so the argument decides width and slots
/// only.
pub(crate) fn family_birth_desc(class_id: u32, args: &[f64]) -> Option<*const PerryLayoutDesc> {
    let (_, default) = family_of_class(class_id)?;
    let declaration = super::class_registry::declarations::class_declaration(class_id)?;
    let table = declaration.instance_variants;
    if table.is_null() {
        return Some(default);
    }
    // SAFETY: an image constant of this program (codegen emits the table
    // with exactly `count` entries).
    unsafe {
        let t = &*table;
        let Some(&arg) = args.get(t.param as usize) else {
            return Some(default);
        };
        let v = crate::JSValue::from_bits(arg.to_bits());
        if !v.is_number() {
            return Some(default);
        }
        let n = v.as_number();
        // -0 and 0 name one constant; codegen emits integral constants as +0.
        let bits = if n == 0.0 { 0 } else { n.to_bits() };
        let entries = std::slice::from_raw_parts(t.entries.as_ptr(), t.count as usize);
        let Ok(i) = entries.binary_search_by(|e| e.key_bits.cmp(&bits)) else {
            return Some(default);
        };
        let desc = entries[i].desc;
        let (start, end) = layout_bounds();
        let address = desc as usize;
        if address < start || address >= end || (*desc).class_id != class_id {
            return Some(default);
        }
        if family_diag_on() {
            FAMILY_DIAG
                .variant_births
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Some(desc)
    }
}

/// Class births allocate the complete fixed layout directly.
/// Retained for a runtime construction path that already made a keyless shell.
pub(crate) unsafe fn adopt_family_layout(obj: *mut ObjectHeader) -> *mut ObjectHeader {
    if obj.is_null() || is_family_object(obj) {
        return obj;
    }
    let Some((_, desc)) = family_of_class((*obj).class_id) else {
        return obj;
    };
    let d = &*desc;
    let Some(birth) = super::shapes::object_shape_descriptor(obj) else {
        return obj;
    };
    if birth.logical_key_count != 0 || birth.live_inline_slot_count < d.slot_count {
        return obj;
    }
    let prototype = super::shapes::object_prototype_word(obj);
    let brand = super::shapes::object_evaluation_brand(obj);
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(obj);
    let proto = scope.root_nanbox_u64(prototype);
    let brand = scope.root_nanbox_u64(brand);
    // A family object's evaluation brand lives in its meta record
    // (`shapes::object_evaluation_brand`); its [[Prototype]] stays a shape
    // fact, re-linked below once the object is on the layout.
    if brand.get_nanbox_u64() != 0 {
        let (meta, current) = object.across_mut::<ObjectHeader, _>(|| {
            super::object_meta_ensure(
                crate::meta_obj_from_kind!(obj, prototype),
                super::MetaReason::AdoptFamilyLayout,
            )
        });
        (*meta).private_evaluation_brand = brand.get_nanbox_u64();
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of!((*meta).private_evaluation_brand) as usize,
            (*meta).private_evaluation_brand,
        );
        let _ = current;
    }
    let obj = object.get_raw_mut_ptr::<ObjectHeader>();
    let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj));
    for i in 0..d.slot_count as usize {
        // GC_STORE_AUDIT(POINTER_FREE): All three stores write the immediate TAG_HOLE to physical object slots. They introduce no pointer/child edge; allocation or rooted object custody surrounds the fill and shape publication follows.
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    super::shapes::stamp_object_shape_id_with_carrier_note(obj, LayoutWord(load_layout_id(d)));
    // A birth linked to another prototype (a class evaluation's, a
    // newTarget's) keeps that link as its shape's identity: the funnel moves
    // the object to that evaluation's copy of the layout (or off the family,
    // for a link no class instance identity names).
    let prototype = proto.get_nanbox_u64();
    if prototype != 0 {
        super::prototype_chain::object_link_class_evaluation_prototype(obj as usize, prototype);
    }
    object.get_raw_mut_ptr::<ObjectHeader>()
}

/// A key publication on a static family object
/// (`object::set_object_keys_with_live_rep`) that adds one plain data key its
/// layout does not hold: the layout word stays, the object's overflow
/// (§3.4) is installed and orders it from now on, and the added key's entry
/// references fixed slot `slot_count` (past the layout's inline slots, in the
/// spill), where the caller's add stores the value. Every later key of the
/// object takes the overflow's own routing (`has_overflow`). `false` when
/// this is not such a publication; the caller publishes as before.
///
/// # Safety
/// `obj` is live; `keys` is the receiver's next key list.
pub(crate) unsafe fn route_family_add(
    obj: *mut ObjectHeader,
    keys: super::ObjectKeys,
    live_inline_slot_count: u32,
) -> bool {
    let word = (*obj).parent_class_id;
    if word & LAYOUT_SHADOW_BIT != 0 {
        return false;
    }
    let Some(d) = family_desc_of(obj) else {
        return false;
    };
    if live_inline_slot_count != d.slot_count
        || keys.arr().is_null()
        || keys.count() != d.slot_count + 1
    {
        return false;
    }
    let Some(current) = super::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    if current.keys == 0 {
        return false;
    }
    let old =
        crate::array::array_elements_ptr(current.keys as usize as *const crate::array::ArrayHeader)
            as *const u64;
    let new = crate::array::array_elements_ptr(keys.arr() as *const crate::array::ArrayHeader)
        as *const u64;
    for i in 0..d.slot_count as usize {
        if *old.add(i) != *new.add(i) {
            return false;
        }
    }
    let n = d.slot_count;
    if super::key_attrs::keys_entry(keys.arr(), n) != 0 {
        return false;
    }
    let key = crate::JSValue::from_bits(*new.add(n as usize));
    if !key.is_any_string() && crate::symbol::js_is_symbol(f64::from_bits(key.bits())) == 0 {
        return false;
    }
    super::overflow::append_fixed_ref(obj, key.bits(), n);
    true
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_ALLOC_LAYOUT: unsafe extern "C" fn(*const PerryLayoutDesc) -> *mut ObjectHeader =
    perry_alloc_layout;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_LAYOUT_INDEX_PROBE: unsafe extern "C" fn(
    *const PerryLayoutDesc,
    *const StaticAtom,
) -> i32 = perry_layout_index_probe;

/// Does this word name a compiler-owned class instance layout (the family's
/// own id; an evaluation's copy is an object fact, [`is_family_object`])?
#[inline]
pub(crate) fn is_family_layout_word(word: u32) -> bool {
    family_for_word(word).is_some_and(|desc| unsafe {
        registered_layout_id(load_layout_id(desc)) == Some(layout_id_of_word(word))
    })
}

// ---------------------------------------------------------------------------
// Case 2c keys (OBJECT_MODEL_2026-10-13.md §3.2, §3.4).
//
// A class family's descriptor lists its constructor keys, then the keys
// static stores add later (late keys, `layout_families::late` in
// perry-codegen), each with `LAYOUT_KEY_LATE` in its attributes, which is the
// key list's `key_attrs::ENTRY_OPTIONAL`. The birth key list names every
// key, and a birth writes none: a key is absent while its slot holds the
// hole and present, at its key position, once written (`key_view`).
//
// A family object keeps its layout word for life. Writing a key is an add
// ([`family_store`] at the runtime's store entries; a constructor's stores
// on a clean chain are plain slot stores in codegen):
//
// * In order (no key after it in the list is present yet: a constructor
//   stores its keys in key order), the write fills the slot in place: the
//   enumeration order is the key sequence.
// * Out of order, the object's overflow (§3.4) is installed first. It is the
//   presence and order authority from then on: the hole slots are its
//   tombstones, and the write appends the key with a reference to its slot.
// * A write that is not a plain add (a non-extensible object, a prototype
//   chain that may intercept it) is the ordinary `[[Set]]`'s: it sees the
//   unwritten key as absent, and its CreateDataProperty step, when it gets
//   there, adds the key through [`family_create`].
//
// A key the layout does not name joins the overflow the same way.
// ---------------------------------------------------------------------------

/// `PerryLayoutKey.attrs` bit of a late key: its slot holds the hole until
/// the key is written. The key list carries it as `key_attrs::ENTRY_OPTIONAL`.
pub const LAYOUT_KEY_LATE: u8 = crate::codegen_abi::LAYOUT_KEY_OPTIONAL;

/// The family descriptor of `obj`'s layout word (shadow bit ignored).
#[inline]
pub(crate) unsafe fn family_desc_of(obj: *const ObjectHeader) -> Option<&'static PerryLayoutDesc> {
    if obj.is_null() || !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
        return None;
    }
    let word = (*obj).parent_class_id;
    // `obj` is live and carries `word`: the receiver read needs no check.
    if !super::shapes::is_layout_word(layout_id_of_word(word)) {
        return None;
    }
    let index = super::shapes::shape_record_of_receiver(word)?
        .family_desc_index_plus_one()
        .checked_sub(1)?;
    literal_birth::family_at(index).map(|desc| &*desc)
}

/// The static GC scan plan of `obj`: the slots that may hold a heap pointer,
/// read from the descriptor of the family layout `obj` is born on
/// ([`PerryLayoutDesc::gc_plan`]), or `None` when the collectors must walk it
/// generically.
///
/// The plan applies only to an object on its family's OWN layout word: not an
/// evaluation copy (it names a different prototype and holder) and not a
/// shadowed word (the object carries own overflow). `record` is the shape
/// record of that word, which the caller already resolved. A family record is
/// a static record (`install_declared_layout` marks it an external carrier),
/// so the shape table roots its keys array and no prune retires it; that is
/// what lets a plan walk skip the per-object keys edge and carrier notes.
///
/// # Safety
/// `obj` is a live object and `record` is the record of its layout word.
#[inline(always)]
pub(crate) unsafe fn static_gc_scan_mask(
    obj: *const ObjectHeader,
    record: super::shapes::ShapeRecordRef,
) -> Option<u32> {
    let index = record.family_desc_index_plus_one().checked_sub(1)?;
    let desc = literal_birth::family_at(index)?;
    let plan = (*desc).gc_plan;
    (plan & crate::codegen_abi::LAYOUT_GC_PLAN_STATIC != 0
        && (*desc).layout_id == (*obj).parent_class_id
        && record.is_static_record())
    .then_some(plan & !crate::codegen_abi::LAYOUT_GC_PLAN_STATIC)
}

/// Is `obj` a class family object: on its family layout, or on a class
/// evaluation's copy of it ([`family_desc_of`])?
///
/// # Safety
/// `obj` is a live object or null.
#[inline]
pub(crate) unsafe fn is_family_object(obj: *const ObjectHeader) -> bool {
    family_desc_of(obj).is_some()
}

/// The family descriptor a shape record's `family_desc_index_plus_one` fact names
/// (`shapes::evaluation_family_desc_index_plus_one`), or `None` for 0.
///
/// # Safety
/// `family` is a record's `family_desc_index_plus_one` fact.
#[inline]
pub(crate) unsafe fn evaluation_layout_desc(family: u32) -> Option<&'static PerryLayoutDesc> {
    // Publication already established family membership. Consume that record
    // proof with the one checked span decoder, without classifying it again.
    let index = family.checked_sub(1)?;
    literal_birth::descriptor_at(index).map(|(desc, _)| &*desc)
}

/// Is `word` (an `ObjectHeader` layout word) a static class family layout
/// candidate: in the descriptor section's id range, which only
/// [`family_own_key`] answers without a shape record?
#[inline(always)]
pub(crate) fn word_in_layout_index(word: u32) -> bool {
    let image = super::class_image::agent_layout_image();
    let base = crate::codegen_abi::LAYOUT_TAG | image.family_first;
    word.wrapping_sub(base) < image.family_count
}

/// A prototype transition moved an object from layout word `previous` to
/// ShapeId `id`. When `previous` is a class family layout, or an evaluation
/// copy of one, and `id` has that family's facts at a class evaluation's
/// identity, `id`'s record names the family
/// (`shapes::note_evaluation_layout`), so its objects stay family objects.
pub(crate) fn note_family_relink(previous: LayoutWord, id: LayoutWord) {
    let previous = layout_id_of_word(previous.0);
    let Some(record) = super::shapes::shape_record_for_word(LayoutWord(previous)) else {
        return;
    };
    let index = record.family_desc_index_plus_one();
    if let Some(desc) = unsafe { evaluation_layout_desc(index) } {
        super::shapes::note_evaluation_layout(id, LayoutWord(desc.layout_id));
    }
}

#[path = "layout_desc/diagnostics.rs"]
mod diagnostics;
pub(crate) use diagnostics::diag_note_install;
pub(crate) use diagnostics::family_diag_init;
pub(crate) use diagnostics::family_diag_on;
pub(crate) use diagnostics::family_diag_pairs_on;
pub(crate) use diagnostics::install_cause;
pub(crate) use diagnostics::note_install_cause;
use diagnostics::note_other_key;
pub(crate) use diagnostics::note_out_of_order_pair;
pub(crate) use diagnostics::note_run;
use diagnostics::FAMILY_DIAG;

#[path = "layout_desc/store.rs"]
mod store;
pub(crate) use store::desc_own_key;
use store::desc_position_by_bytes;
pub(crate) use store::desc_position_hashed;
pub(crate) use store::family_create;
pub(crate) use store::family_own_key;
pub(crate) use store::family_own_key_hashed;
use store::family_slot_word;
pub(crate) use store::family_store;
pub(crate) use store::family_store_str;
pub(crate) use store::family_store_value;
pub(crate) use store::later_late_key_present;

/// `[[DefineOwnProperty]]` of `key` (a property key) on `receiver`, before the
/// definition runs: when the key is a layout's optional key whose slot holds
/// the hole (absent, `key_view`), the object gets its overflow (§3.4), whose
/// tombstone the hole then is. The definition meets an absent key and adds
/// it, in definition order, with exactly the descriptor's attributes. May
/// allocate: the caller holds its operands in handles.
///
/// # Safety
/// `receiver` is a live value.
pub(crate) unsafe fn prepare_hole_define(receiver: u64, key: u64) {
    if receiver & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return;
    }
    let obj = (receiver & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if super::key_view::own_key_is_hole(obj, crate::JSValue::from_bits(key)) {
        note_install_cause(install_cause::DEFINE);
        super::overflow::ensure(obj);
    }
}

// Program records use linker section bounds on every target. On wasm the
// runtime also keeps its own existing inventory: Rust custom sections cannot
// carry relocations. No program constructor or address registry is needed.
pub(super) fn image_layouts() -> impl Iterator<Item = *mut PerryLayoutDesc> {
    #[cfg(target_family = "wasm")]
    {
        records::<PerryLayoutDesc>(layout_bounds()).chain(
            inventory::iter::<crate::wasm_image::Layout>
                .into_iter()
                .map(|item| item.0),
        )
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<PerryLayoutDesc>(layout_bounds())
    }
}

#[cfg(any(test, target_family = "wasm"))]
#[path = "layout_desc/enumeration.rs"]
mod enumeration;

fn descriptor_index(desc: *const PerryLayoutDesc) -> Option<u32> {
    let (start, end) = layout_bounds();
    let at = desc as usize;
    let size = std::mem::size_of::<PerryLayoutDesc>();
    if at >= start && at < end && (at - start) % size == 0 {
        Some(((at - start) / size) as u32)
    } else {
        let literal = literal_birth::literal_index(desc);
        #[cfg(target_family = "wasm")]
        {
            literal.or_else(|| literal_birth::runtime_index(desc))
        }
        #[cfg(not(target_family = "wasm"))]
        {
            literal
        }
    }
}
const _: () = assert!(
    std::mem::offset_of!(PerryLayoutDesc, shape_id)
        == if cfg!(target_pointer_width = "64") {
            crate::codegen_abi::LAYOUT_DESC_ID_OFFSET_LP64
        } else {
            crate::codegen_abi::LAYOUT_DESC_ID_OFFSET_ILP32
        }
);

#[cfg(all(test, target_os = "windows"))]
mod windows_layout_bounds_tests {
    use super::*;

    #[test]
    fn image_bounds_include_runtime_layouts() {
        let (start, end) = layout_bounds();
        let timeout = std::ptr::addr_of!(crate::object::runtime_layouts::TIMEOUT);
        let sentinel = std::ptr::addr_of!(LAYOUT_SECTION_SENTINEL);
        assert!(start < end, "COFF layout image must not be empty");
        for record in [timeout, sentinel] {
            let address = record as usize;
            assert!(address >= start && address + std::mem::size_of::<PerryLayoutDesc>() <= end);
            assert!(image_layouts().any(|entry| std::ptr::eq(entry, record)));
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_atom_bounds_tests {
    use super::*;

    #[test]
    fn image_bounds_include_runtime_atoms_and_sentinel() {
        let (start, end) = atom_bounds();
        let atom = crate::object::runtime_layouts::test_image_atom();
        let sentinel = std::ptr::addr_of!(ATOM_SECTION_SENTINEL);
        assert!(start < end, "COFF atom image must not be empty");
        for record in [atom, sentinel] {
            let address = record as usize;
            assert!(address >= start && address + std::mem::size_of::<StaticAtom>() <= end);
            assert!(image_atoms().any(|entry| std::ptr::eq(entry, record)));
        }
    }
}

#[cfg(test)]
#[path = "layout_section_tests.rs"]
mod layout_section_tests;
#[cfg(all(feature = "wasm-image-selftest", target_family = "wasm"))]
#[path = "layout_desc/wasm_program_image_checks.rs"]
mod wasm_program_image_checks;
