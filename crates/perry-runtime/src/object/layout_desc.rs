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

use super::ObjectHeader;
mod define_field;
pub(crate) mod methods;
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
        let meta = self.meta;
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
pub const fn carry_shadow(previous: u32, id: u32) -> u32 {
    if word_is_shadowed(previous)
        && id >= super::shapes::SHAPE_ID_BASE
        && id < crate::codegen_abi::LAYOUT_ORDINARY_END
    {
        id | LAYOUT_SHADOW_BIT
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
#[repr(C)]
pub struct PerryProtoFamily {
    pub serial: u64,
}

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

/// A program-named key (§6a). `kind` 0 is a string key: its precomputed
/// FNV-1a hash (the runtime's key hash) and the immutable image string that
/// is its one address. `kind` 1 is a well-known symbol: `string` null,
/// `index` its number in `codegen_abi::WELL_KNOWN_SYMBOLS`, `hash` that
/// number's fixed constant. `kind` 2 is a program symbol: `string` its
/// description, `hash` the creation sequence number the runtime assigns when
/// module init creates it. Registration binds a well-known symbol atom to the
/// agent's symbol before user code runs.
#[repr(C)]
pub struct StaticAtom {
    pub hash: u64,
    pub string: *const StringHeader,
    pub kind: u8,
    pub _pad: [u8; 3],
    pub index: u32,
}

/// One key of a layout's key sequence (enumeration order).
#[repr(C)]
pub struct PerryLayoutKey {
    pub atom: *const StaticAtom,
    pub slot: u32,
    pub attrs: u8,
    pub _pad: [u8; 3],
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
    pub _layout_pad: u32,
}

const _: () = assert!(
    std::mem::size_of::<PerryLayoutDesc>()
        == if cfg!(target_pointer_width = "32") {
            64
        } else {
            80
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
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = $elf
        )]
        static $($mutable)* $name: $ty = $init;
    };
}

section_sentinel!(
    LAYOUT_SECTION_SENTINEL,
    PerryLayoutDesc,
    PerryLayoutDesc {
        shape_id: 0,
        _layout_pad: 0,
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

/// The entry module supplies the strong, immutable program descriptor.
/// Runtime-only binaries use this empty weak definition. Its address is
/// resolved by the linker, never registered, memoized or changed at startup.
#[used(linker)]
#[cfg_attr(not(target_family = "wasm"), linkage = "weak")]
#[export_name = "perry_shadow_name_descriptor"]
static SHADOW_NAME_DESCRIPTOR: StaticAtom = StaticAtom {
    hash: 0,
    string: std::ptr::null(),
    kind: 0,
    _pad: [0; 3],
    index: 0,
};

#[inline(always)]
pub(crate) fn shadow_name_descriptor() -> &'static StaticAtom {
    &SHADOW_NAME_DESCRIPTOR
}

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
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")))]
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
    #[cfg(target_family = "wasm")]
    {
        (0, 0)
    }
}

/// (start, end) of the atom section.
fn atom_bounds() -> (usize, usize) {
    std::hint::black_box(std::ptr::addr_of!(ATOM_SECTION_SENTINEL) as usize);
    #[cfg(target_vendor = "apple")]
    {
        unsafe extern "C" {
            #[link_name = "\u{1}section$start$__DATA$__perry_atoms"]
            static START: u8;
            #[link_name = "\u{1}section$end$__DATA$__perry_atoms"]
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
            std::ptr::addr_of!(ATOM_START) as usize + 8,
            std::ptr::addr_of!(ATOM_END) as usize,
        )
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")))]
    {
        unsafe extern "C" {
            static __start_perry_atoms: u8;
            static __stop_perry_atoms: u8;
        }
        (
            std::ptr::addr_of!(__start_perry_atoms) as usize,
            std::ptr::addr_of!(__stop_perry_atoms) as usize,
        )
    }
    #[cfg(target_family = "wasm")]
    {
        (0, 0)
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

/// The bytes of an atom's image string.
unsafe fn atom_bytes<'a>(atom: *const StaticAtom) -> Option<&'a [u8]> {
    let s = (*atom).string;
    if (*atom).kind != crate::codegen_abi::ATOM_KIND_STRING || s.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(
        crate::string::string_data(s),
        (*s).byte_len as usize,
    ))
}

/// Object model step 5 (case 2c): every coloured key of the program, as
/// `(bytes, hash, mask bit, slot)`. Codegen marks a coloured string atom with
/// `_pad[0] = 1` and `index = bit | slot << 8` (`perry-codegen`
/// `layout_desc::atom`); the entry module emits one for every coloured key.
pub(crate) fn coloured_atoms() -> Vec<(&'static [u8], u64, u32, u32)> {
    let mut out = Vec::new();
    unsafe {
        for atom in image_atoms() {
            if (*atom)._pad[0] != 1 {
                continue;
            }
            if let Some(bytes) = atom_bytes(atom) {
                let index = (*atom).index;
                out.push((bytes, (*atom).hash, index & 0xFF, index >> 8));
            }
        }
    }
    out.sort();
    out.dedup();
    out
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

/// The symbol a symbol atom names (well-known, node's own, or registered),
/// `None` for a string atom.
pub(crate) unsafe fn atom_symbol(
    atom: *const StaticAtom,
) -> Option<*mut crate::symbol::SymbolHeader> {
    well_known_atom_symbol(atom)
        .or_else(|| super::runtime_layouts::runtime_symbol_of_atom(atom))
        .or_else(|| registered_atom_symbol(atom))
}

/// Pre-seed the agent's atom table, then register every layout. Runs once per
/// agent from `js_shape_run_static_seed`, after the hash-band literal seeds.
pub(crate) fn seed_static_atoms() {
    #[cfg(target_family = "wasm")]
    assert!(
        image_atoms().next().is_some(),
        "Wasm image constructors did not run"
    );
    super::runtime_layouts::seed_ascii_char_atoms();
    unsafe {
        for atom in image_atoms() {
            if let Some(bytes) = atom_bytes(atom) {
                crate::string::seed_static_atom(bytes, (*atom).hash, (*atom).string);
            }
        }
    }
    crate::object::layout_probe::seed_colours();
}

pub(crate) fn register_static_layouts() {
    #[cfg(target_family = "wasm")]
    assert!(
        image_layouts().next().is_some(),
        "Wasm layout constructors did not run"
    );
    unsafe {
        for (index, desc) in image_layouts().enumerate() {
            if (*desc).family == SENTINEL_FAMILY || (*desc).keys.is_null() && (*desc).key_count != 0
            {
                continue;
            }
            if (*desc).layout_id == 0 {
                continue;
            }
            let Some(id) = layout_id_of_index(index as u32) else {
                break;
            };
            let got = if (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_NAMESPACE {
                reserve_namespace_layout(desc)
            } else {
                register_one(desc, id)
            };
            let _ = got;
        }
    }
}

/// A module namespace layout's id, drawn at startup in section order (so
/// every agent draws the same one) without building its key list or shape:
/// [`register_namespace_layout`] installs them at the namespace's birth. Most
/// compiled namespaces are never built.
unsafe fn reserve_namespace_layout(desc: *const PerryLayoutDesc) -> u32 {
    super::static_shapes::resolve_layout_fields(
        std::ptr::addr_of!((*desc).shape_id).cast_mut(),
        (*desc).layout_id,
        super::shapes::PROTO_ID_NULL,
    )
    .identity
    .0
}

/// The installed layout of module namespace descriptor `desc`, installing
/// its key list and shape under the id startup reserved on the first birth
/// in this agent; `None` when this runtime cannot represent it.
pub(crate) unsafe fn register_namespace_layout(desc: *const PerryLayoutDesc) -> Option<u32> {
    let id = register_one(desc, 0);
    (id != 0).then_some(id)
}

/// The shape kind a descriptor's `kind` code declares, among the ordinary
/// kinds the registrar represents: `Ordinary`, and for a runtime prototype
/// whose stores must reach their funnel, `OrdinaryUnmarked` or
/// `CallableOrdinary`.
fn declared_kind(code: u8) -> Option<super::shapes::ShapeObjectKind> {
    use super::shapes::ShapeObjectKind::{CallableOrdinary, Ordinary, OrdinaryUnmarked};
    [Ordinary, OrdinaryUnmarked, CallableOrdinary]
        .into_iter()
        .find(|k| k.code() == code as u64)
}

/// Register one descriptor under `requested`; the id it resolved to, or 0
/// when this runtime cannot represent it yet (a descriptor prototype, a
/// non-ordinary kind, slots that are not a permutation of the key positions,
/// a key that is not an atom). The key sequence is the enumeration order;
/// each key's slot is its storage.
pub(crate) unsafe fn register_one(desc: *const PerryLayoutDesc, requested: u32) -> u32 {
    register_one_bound(desc, requested, &[])
}

/// The module-local symbol keys (`ATOM_KIND_BINDING_SYMBOL`) of a
/// descriptor's key sequence.
unsafe fn binding_symbol_keys(d: &PerryLayoutDesc) -> usize {
    if d.key_count == 0 {
        return 0;
    }
    std::slice::from_raw_parts(d.keys, d.key_count as usize)
        .iter()
        .filter(|k| (*k.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL)
        .count()
}

/// The key word of key `key` of a descriptor: a string's, a symbol atom's
/// symbol, or for the `j`-th module-local symbol key, `bound[j]` (`j` counts
/// those keys before it, `next` the running count).
unsafe fn layout_key_word(key: &PerryLayoutKey, bound: &[u64], next: &mut usize) -> u64 {
    if (*key.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
        let word = bound[*next];
        *next += 1;
        return word;
    }
    match atom_symbol(key.atom) {
        // The key word a symbol data property stores.
        Some(symbol) => crate::value::POINTER_TAG | symbol as u64,
        None => crate::value::JSValue::string_ptr((*key.atom).string.cast_mut()).bits(),
    }
}

/// [`register_one`] for a descriptor with module-local symbol keys: `bound`
/// holds their key words, in key order. Without them (startup) the id is
/// only reserved: the word names it, but no shape is installed until the
/// prototype's bind hands the symbols (`perry_bind_family_prototype`), so no
/// object carries the id before then.
pub(crate) unsafe fn register_one_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
) -> u32 {
    register_bound(desc, requested, bound, None)
}

/// An ordinary function's layout, with the actual prototype serial carried
/// by its ordinary shape record. The existing identity word roots its link.
pub(crate) unsafe fn register_function_layout(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    proto_id: u64,
) -> u32 {
    register_bound(desc, requested, bound, Some(proto_id))
}

unsafe fn register_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    function_proto: Option<u64>,
) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).shape_id).cast_mut());
    let function = (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR;
    if function && function_proto.is_none() {
        return 0;
    }
    let existing = if function {
        None
    } else {
        Some(word.load(Ordering::Acquire)).filter(|&id| id != 0)
    };
    let local = binding_symbol_keys(&*desc);
    if let Some(id) = existing {
        if (local != 0 && bound.is_empty()) || super::shapes::shape_descriptor_by_id(id).is_some() {
            return (*desc).layout_id;
        }
    }
    if local != 0 && !bound.is_empty() && bound.len() != local {
        return 0;
    }
    let d = &*desc;
    let Some(kind) = declared_kind(d.kind) else {
        return 0;
    };
    if existing.is_none() && !function && requested >= super::shapes::STATIC_SHAPE_ID_END {
        return 0;
    }
    let all_keys: &[PerryLayoutKey] = if d.key_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(d.keys, d.key_count as usize)
    };
    // An optional key (a class family's late key, case 2c, or a runtime
    // layout's optional member) is in the birth key list with
    // `key_attrs::ENTRY_OPTIONAL`: absent while its slot holds the hole
    // (`key_view`).
    let keys = all_keys;
    // Names, hashes and attribute bytes already live in the binary.
    // Validate the fixed ABI without rebuilding transient name/attribute
    // vectors or revalidating each image string as UTF-8 at agent startup.
    let mapped = keys
        .iter()
        .enumerate()
        .any(|(i, key)| key.slot as usize != i);
    if mapped {
        // A permutation keeps the next position free as the next key's slot.
        let mut taken = vec![false; keys.len()];
        for key in keys {
            let slot = key.slot as usize;
            if slot >= keys.len() || taken[slot] {
                return 0;
            }
            taken[slot] = true;
        }
    }
    for key in keys {
        if key.slot >= d.slot_count {
            return 0;
        }
        match (*key.atom).kind {
            crate::codegen_abi::ATOM_KIND_STRING if !(*key.atom).string.is_null() => {}
            crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL
                if ((*key.atom).index as usize) < crate::codegen_abi::WELL_KNOWN_SYMBOLS.len() => {}
            // A symbol of node's own library the runtime binds
            // (`runtime_layouts::runtime_symbol_of_atom`).
            crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL
                if super::runtime_layouts::runtime_symbol_of_atom(key.atom).is_some() => {}
            // A registered symbol (`Symbol.for(desc)`) the program names.
            crate::codegen_abi::ATOM_KIND_REGISTERED_SYMBOL if !(*key.atom).string.is_null() => {}
            // A module-local symbol: a prototype image's, bound at its bind.
            crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL
                if d.variant == PROTOTYPE_IMAGE_VARIANT
                    || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR => {}
            _ => return 0,
        }
    }
    // Case 2c: a class family's instances have the class's prototype, and
    // its id is its record's (`layout_index_of_word`).
    let class_family = d.class_id != 0;
    let proto_id = match d.proto.addr() {
        3 => super::shapes::PROTO_ID_DEFAULT,
        _ if class_family => super::shapes::class_proto_id(d.class_id),
        0 => super::shapes::PROTO_ID_DEFAULT,
        1 => super::shapes::PROTO_ID_NULL,
        2 if d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR => {
            match function_proto {
                Some(id) if id != 0 => id,
                _ => return 0,
            }
        }
        fam if fam >= LAYOUT_PROTO_TAG_END => proto_family_serial(d.proto),
        _ => return 0,
    };
    if local != 0 && bound.is_empty() {
        // Reserve the id the registration would take; the bind installs it.
        let proposed = if proto_id == super::shapes::PROTO_ID_DEFAULT
            || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        {
            requested
        } else {
            super::shapes::allocate_declared_layout_id(proto_id)
        };
        return match word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => (*desc).layout_id,
            Err(_) => (*desc).layout_id,
        };
    }
    let key_count = keys.len() as u32;
    let slot_count = d.slot_count;
    let rep = d.rep;
    let integrity = d.integrity as u16;
    // Compatibility key arrays are owned by the one authoritative shape
    // record, and are shared by the canonical key trie when names/attrs agree.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let list = if key_count == 0 {
        std::ptr::null_mut()
    } else if let Some(canonical) = (!function)
        .then(|| {
            super::canonical_keys::from_static_layout(
                &super::canonical_keys::SharedLayout::shape_cache_entry(),
                keys,
                mapped,
            )
        })
        .flatten()
    {
        canonical.view().arr()
    } else {
        let list = super::key_attrs::alloc_canonical_key_list(
            key_count,
            true,
            mapped || keys.iter().any(|key| key.attrs != 0),
        );
        let values = crate::array::array_elements_ptr(list);
        let attributes = super::key_attrs::keys_attrs(list);
        let mut next = 0;
        for (i, key) in keys.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): unpublished list, under suppression.
            *values.add(i) = if function && (*key.atom).kind == crate::codegen_abi::ATOM_KIND_STRING
            {
                let atom = crate::string::property_atom((*key.atom).string);
                crate::value::JSValue::string_ptr(atom.cast_mut()).bits()
            } else {
                layout_key_word(key, bound, &mut next)
            };
            if !attributes.is_null() {
                super::key_attrs::attrs_write(attributes, i as u32, key.attrs, &*key.atom);
                if mapped {
                    super::key_attrs::attrs_write_slot(attributes, i as u32, key.slot);
                }
            }
        }
        (*list).length = key_count;
        super::canonical_keys::canonicalize_owned(
            &super::canonical_keys::SharedLayout::shape_cache_entry(),
            list,
            key_count,
        )
        .view()
        .arr()
    };
    let summary = super::key_attrs::keys_summary(list, key_count);
    // A carried prototype layout is a marked prototype's: its own semantic
    // generation, shared with no other layout.
    let image = d.variant == PROTOTYPE_IMAGE_VARIANT;
    let generation = if image {
        super::shapes::fresh_semantic_generation()
    } else {
        0
    };
    if function {
        let word =
            super::shapes::publish_shape_result(super::shapes::shape_descriptor_intern_with_rep(
                list,
                key_count,
                slot_count,
                generation,
                kind,
                0,
                proto_id,
                summary,
                rep,
                &[],
                None,
                integrity,
            ));
        // A per-evaluation record is NOT an external carrier: the objects born
        // on it (and the prototype they name) keep it live, and it retires
        // with the last of them. The caller holds GC suppression from here to
        // the birth's stamp (function_family_birth::allocate_layout).
        return word;
    }
    let proposed = if let Some(id) = existing {
        id
    } else if function {
        super::shapes::allocate_declared_layout_id(proto_id)
    } else if proto_id == super::shapes::PROTO_ID_DEFAULT
        || class_family
        || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
    {
        requested
    } else {
        super::shapes::allocate_declared_layout_id(proto_id)
    };
    // End the descriptor borrow before the startup-only id publication.
    let id = if let Some(id) = existing {
        id
    } else {
        match word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => proposed,
            Err(other) => {
                assert!(
                    super::shapes::is_shape_id(other),
                    "descriptor identity cell holds a ShapeId"
                );
                other
            }
        }
    };
    if super::shapes::shape_descriptor_by_id(id).is_none() {
        super::shapes::install_declared_layout(
            id,
            (*desc).layout_id,
            if (*desc).class_id != 0 {
                descriptor_index(desc).map_or(0, |i| i + 1)
            } else {
                0
            },
            list,
            key_count,
            slot_count,
            kind,
            proto_id,
            summary,
            rep,
            integrity,
            generation,
        );
    }
    super::shapes::note_external_shape_carrier(super::shapes::shape_descriptor_for_word(
        (*desc).layout_id,
    ));
    (*desc).layout_id
}

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
    let Some(id) = registered_layout_id(d.layout_id) else {
        eprintln!(
            "Perry internal error: perry_alloc_layout of an unregistered layout (family {:#x})",
            d.family
        );
        std::process::abort();
    };
    alloc_layout_object(d, id)
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
    copy: u32,
) -> *mut ObjectHeader {
    alloc_layout_object(&*desc, copy)
}

/// Allocate an object of the registered layout `id` of `d` (see
/// [`perry_alloc_layout`]). Can collect.
unsafe fn alloc_layout_object(d: &PerryLayoutDesc, id: u32) -> *mut ObjectHeader {
    alloc_layout_object_sized(d, id, d.slot_count as usize)
}

/// Function births alone retain the ordinary inline floor. Keep their
/// sizing and diagnostic work out of the shared class/literal allocator.
/// `desc` is the registered layout admitted by function_family_birth.
pub(crate) unsafe fn alloc_function_layout(
    desc: *const PerryLayoutDesc,
    id: u32,
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
    id: u32,
    physical_slots: usize,
) -> *mut ObjectHeader {
    // A birth writes only holes; constructor stores check the live chain.
    let word = id;
    let size = std::mem::size_of::<ObjectHeader>() + physical_slots * 8;
    let obj =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT).cast::<ObjectHeader>();
    // GC_STORE_AUDIT(INIT): unpublished header and hole-only declared slots.
    std::ptr::write(
        obj,
        ObjectHeader {
            class_id: d.class_id,
            parent_class_id: word,
            meta: std::ptr::null_mut(),
            #[cfg(target_pointer_width = "32")]
            _slot_alignment_padding: 0,
        },
    );
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    for i in 0..physical_slots {
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
    recorded: u32,
) -> Option<*mut ObjectHeader> {
    let id = registered_layout_id(d.layout_id)?;
    let size = std::mem::size_of::<ObjectHeader>() + d.slot_count as usize * 8;
    let obj =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT).cast::<ObjectHeader>();
    // No collection from here until the newborn carries its shape.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let base = super::shapes::shape_descriptor_for_word(id)?;
    let proto_bits = proto.get_nanbox_u64();
    if super::shapes::identity_prototype_word(proto_id) != proto_bits {
        super::shapes::write_identity_word(proto_id, proto_bits);
    }
    let shape = match super::shapes::shape_keys_and_proto_for_word(recorded) {
        Some((keys, count, recorded_proto))
            if recorded != 0
                && keys == base.keys
                && count == base.logical_key_count
                && recorded_proto == proto_id =>
        {
            recorded
        }
        _ => super::shapes::publish_shape_result(super::shapes::shape_descriptor_intern_with_rep(
            base.keys as usize as *const crate::array::ArrayHeader,
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
            parent_class_id: shape,
            meta: std::ptr::null_mut(),
            #[cfg(target_pointer_width = "32")]
            _slot_alignment_padding: 0,
        },
    );
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
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
    let Some(id) = registered_layout_id(d.layout_id) else {
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
    super::shapes::stamp_birth_shape_id_with_carrier_note(obj, id);
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
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
    let id = registered_layout_id(d.layout_id)?;
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
    let image = scope.root_raw_mut_ptr(alloc_layout_object(d, id));
    // Born a marked prototype (the layout is a marked one's): its meta
    // record carries the mark and the family's serial, so the binding's mark
    // finds it done and makes no transition.
    let meta = image.with_mut_ptr(|p: *mut ObjectHeader| super::object_meta_ensure(p));
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
        let slot_addr = image as usize
            + std::mem::size_of::<ObjectHeader>()
            + key.slot as usize * std::mem::size_of::<u64>();
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
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & super::shapes::INTEGRITY_FLAGS != 0
    {
        return false;
    }
    let obj = p as *const ObjectHeader;
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).boxed_primitive != 0
            || (*meta).reflect_metadata != 0
            || (*meta).private_evaluation_brand != 0)
    {
        return false;
    }
    if !super::overflow::table(obj).is_null() || super::shapes::object_shape_stamp(obj) == 0 {
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
            serial != 0
                && proto & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                && crate::value::addr_class::is_above_handle_band(q as usize)
                && !(*q).meta.is_null()
                && (*(*q).meta).proto_serial == serial
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
    let id = registered_layout_id(d.layout_id)?;
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
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let meta = (*(p as *const ObjectHeader)).meta;
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
    let obj = alloc_layout_object(d, id);
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
    if serial == 0
        || proto & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
        || (*p).meta.is_null()
        || (*(*p).meta).proto_serial != serial
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
// Every descriptor's layout id is `LAYOUT_BASE + i`, where `i` is its record
// index in `perry_layouts` (the static band's upper half, which no counter
// mints from). So a read site maps a layout word to its descriptor with
// arithmetic: strip the shadow bit, subtract `LAYOUT_BASE`, compare with the
// record count.
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

/// The descriptor record index a layout word names (shadow bit ignored), or
/// `None` for every word outside the section's ids.
#[inline]
pub(crate) fn layout_index_of_word(word: u32) -> Option<u32> {
    if !super::shapes::is_layout_word(layout_id_of_word(word)) {
        return None;
    }
    let record = super::shapes::shape_record_for_word(word)?;
    record.family_desc_index_plus_one().checked_sub(1)
}

/// Descriptor `index` of the section.
#[inline]
fn desc_at(index: u32) -> *const PerryLayoutDesc {
    (layout_bounds().0 + index as usize * std::mem::size_of::<PerryLayoutDesc>())
        as *const PerryLayoutDesc
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
        let (meta, current) =
            object.across_mut::<ObjectHeader, _>(|| super::object_meta_ensure(obj));
        (*meta).private_evaluation_brand = brand.get_nanbox_u64();
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of!((*meta).private_evaluation_brand) as usize,
            (*meta).private_evaluation_brand,
        );
        let _ = current;
    }
    let obj = object.get_raw_mut_ptr::<ObjectHeader>();
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    for i in 0..d.slot_count as usize {
        // GC_STORE_AUDIT(POINTER_FREE): All three stores write the immediate TAG_HOLE to physical object slots. They introduce no pointer/child edge; allocation or rooted object custody surrounds the fill and shape publication follows.
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    super::shapes::stamp_object_shape_id_with_carrier_note(obj, d.layout_id);
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
    layout_index_of_word(word).is_some_and(|i| unsafe {
        let d = &*desc_at(i);
        d.class_id != 0 && registered_layout_id(d.layout_id) == Some(layout_id_of_word(word))
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
    let d = &*desc_at(index);
    (d.class_id != 0).then_some(d)
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
    match family {
        0 => None,
        // The record names a registered class family layout: only
        // [`note_family_relink`] writes it, from one.
        family => Some(&*desc_at(family - 1)),
    }
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
pub(crate) fn note_family_relink(previous: u32, id: u32) {
    let previous = layout_id_of_word(previous);
    let Some(record) = super::shapes::shape_record_for_word(previous) else {
        return;
    };
    let index = record.family_desc_index_plus_one();
    if index != 0 {
        let family_word = unsafe { (*desc_at(index - 1)).layout_id };
        super::shapes::note_evaluation_layout(id, family_word);
    }
}

struct FamilyDiag {
    births: std::sync::atomic::AtomicU64,
    dirty: std::sync::atomic::AtomicU64,
    seen: std::sync::atomic::AtomicU64,
    fills: std::sync::atomic::AtomicU64,
    out_of_order: std::sync::atomic::AtomicU64,
    other: std::sync::atomic::AtomicU64,
    installs: std::sync::atomic::AtomicU64,
    declined: std::sync::atomic::AtomicU64,
    variant_births: std::sync::atomic::AtomicU64,
}

static FAMILY_DIAG: FamilyDiag = FamilyDiag {
    births: std::sync::atomic::AtomicU64::new(0),
    dirty: std::sync::atomic::AtomicU64::new(0),
    seen: std::sync::atomic::AtomicU64::new(0),
    fills: std::sync::atomic::AtomicU64::new(0),
    out_of_order: std::sync::atomic::AtomicU64::new(0),
    other: std::sync::atomic::AtomicU64::new(0),
    installs: std::sync::atomic::AtomicU64::new(0),
    declined: std::sync::atomic::AtomicU64::new(0),
    variant_births: std::sync::atomic::AtomicU64::new(0),
};

/// `PERRY_FAMILY_DIAG=1`: print the family store counters at exit.
pub(crate) fn family_diag_init() {
    per_test_global! {
        static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    }
    extern "C" fn report() {
        family_diag_report();
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some()
        && !ON.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        RUN_DIAG_ON.store(true, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: registering a plain extern "C" function.
        unsafe { libc::atexit(report) };
    }
}

/// `PERRY_FAMILY_DIAG=2`: count out-of-order fills by (family, key written,
/// first later key present) and print the most frequent at exit (measurement
/// only: which key orders the census gets wrong).
pub(crate) fn family_diag_pairs_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var("PERRY_FAMILY_DIAG").as_deref() == Ok("2"))
}

static OUT_OF_ORDER_PAIRS: std::sync::Mutex<
    Option<std::collections::HashMap<(u32, String, String), u64>>,
> = std::sync::Mutex::new(None);

#[cold]
pub(crate) unsafe fn note_out_of_order_pair(obj: *const ObjectHeader, pos: u32) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let name = |k: &PerryLayoutKey| {
        atom_bytes(k.atom)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    };
    let Some(later) = keys[pos as usize + 1..]
        .iter()
        .find(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE)
    else {
        return;
    };
    let key = (d.class_id, name(&keys[pos as usize]), name(later));
    if let Ok(mut map) = OUT_OF_ORDER_PAIRS.lock() {
        *map.get_or_insert_with(Default::default)
            .entry(key)
            .or_default() += 1;
    }
}

/// `PERRY_FAMILY_DIAG`: count the constructor runs' checks by outcome
/// (`layout_probe::perry_family_run_admits`).
pub(crate) static RUN_DIAG_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(crate) static RUN_DIAG: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];
/// The outcomes [`RUN_DIAG`] counts.
pub(crate) const RUN_OUTCOMES: [&str; 8] = [
    "admitted",
    "not_family",
    "class",
    "key",
    "flags",
    "order",
    "chain",
    "record",
];

/// Count a run check's outcome `i` ([`RUN_OUTCOMES`]) under `PERRY_FAMILY_DIAG`.
#[inline(always)]
pub(crate) fn note_run(i: usize) {
    if RUN_DIAG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        RUN_DIAG[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn family_diag_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let runs: Vec<String> = RUN_OUTCOMES
        .iter()
        .zip(RUN_DIAG.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG runs {}", runs.join(" "));
    if family_diag_pairs_on() {
        if let Ok(map) = OUT_OF_ORDER_PAIRS.lock() {
            let mut pairs: Vec<_> = map.iter().flatten().collect();
            pairs.sort_by(|a, b| b.1.cmp(a.1));
            for ((class_id, key, later), n) in pairs.into_iter().take(40) {
                eprintln!(
                    "PERRY_FAMILY_DIAG out_of_order class={class_id} key={key} after={later} n={n}"
                );
            }
        }
    }
    if std::env::var_os("PERRY_FAMILY_DIAG").is_some() {
        family_diag_other_report();
        eprintln!(
            "PERRY_FAMILY_DIAG births={} variant_births={} dirty={} seen={} fills={} out_of_order={} other={} overflow_installs={} declined={}",
            FAMILY_DIAG.births.load(Relaxed),
            FAMILY_DIAG.variant_births.load(Relaxed),
            FAMILY_DIAG.dirty.load(Relaxed),
            FAMILY_DIAG.seen.load(Relaxed),
            FAMILY_DIAG.fills.load(Relaxed),
            FAMILY_DIAG.out_of_order.load(Relaxed),
            FAMILY_DIAG.other.load(Relaxed),
            FAMILY_DIAG.installs.load(Relaxed),
            FAMILY_DIAG.declined.load(Relaxed)
        );
    }
}

// --- overflow install causes (PERRY_FAMILY_DIAG, measurement only) ---------

/// Why the next `overflow::ensure` on a family object installs its table.
pub(crate) mod install_cause {
    pub const LATE_OUT_OF_ORDER: u8 = 1;
    pub const LATE_PROTO_OR_SPILL: u8 = 2;
    pub const OTHER_KEY: u8 = 3;
    pub const DEFINE: u8 = 4;
    pub const DELETE: u8 = 5;
    pub const DELETE_HOLE: u8 = 6;
    pub const UNIQUE_RUN: u8 = 7;
    pub const NAMES: [&str; 8] = [
        "unknown",
        "late_ooo",
        "late_proto_spill",
        "other_key",
        "define",
        "delete",
        "delete_hole",
        "unique_run",
    ];
}

thread_local! {
    static INSTALL_CAUSE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

static INSTALLS_BY_CAUSE: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];

pub(crate) fn family_diag_on() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var_os("PERRY_FAMILY_DIAG").is_some())
}

#[inline(always)]
pub(crate) fn note_install_cause(cause: u8) {
    INSTALL_CAUSE.with(|c| c.set(cause));
}

/// `overflow::ensure` is installing a table on `obj`: count it by cause when
/// `obj` is a family object.
pub(crate) unsafe fn diag_note_install(obj: *const ObjectHeader) {
    let cause = INSTALL_CAUSE.with(|c| c.replace(0));
    if family_diag_on() && family_desc_of(obj).is_some() {
        INSTALLS_BY_CAUSE[cause as usize & 7].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// (class id, key) -> (first installs, all stores) of keys a family layout
/// does not name.
static OTHER_KEYS: std::sync::Mutex<Option<std::collections::HashMap<(u32, String), (u64, u64)>>> =
    std::sync::Mutex::new(None);

#[cold]
unsafe fn note_other_key(obj: *const ObjectHeader, key_bits: u64, first: bool) {
    let Some(d) = family_desc_of(obj) else {
        return;
    };
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    if let Ok(mut map) = OTHER_KEYS.lock() {
        let e = map
            .get_or_insert_with(Default::default)
            .entry((d.class_id, name))
            .or_default();
        e.0 += first as u64;
        e.1 += 1;
    }
}

fn family_diag_other_report() {
    use std::sync::atomic::Ordering::Relaxed;
    let causes: Vec<String> = install_cause::NAMES
        .iter()
        .zip(INSTALLS_BY_CAUSE.iter())
        .map(|(n, c)| format!("{n}={}", c.load(Relaxed)))
        .collect();
    eprintln!("PERRY_FAMILY_DIAG install_causes {}", causes.join(" "));
    if std::env::var("PERRY_FAMILY_DIAG").as_deref() != Ok("3") {
        return;
    }
    if let Ok(map) = OTHER_KEYS.lock() {
        let mut rows: Vec<_> = map.iter().flatten().collect();
        let mut by_class: std::collections::HashMap<u32, (u64, u64, u32)> = Default::default();
        for ((c, _), (f, t)) in &rows {
            let e = by_class.entry(*c).or_default();
            e.0 += f;
            e.1 += t;
            e.2 += 1;
        }
        let mut classes: Vec<_> = by_class.into_iter().collect();
        classes.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        for (c, (f, t, n)) in classes.into_iter().take(20) {
            let name = super::class_name_for_id(c).unwrap_or_default();
            eprintln!(
                "PERRY_FAMILY_DIAG other_class class={c} name={name} first={f} stores={t} keys={n}"
            );
        }
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.1 .1.cmp(&a.1 .1)));
        for ((c, k), (f, t)) in rows.into_iter().take(80) {
            eprintln!("PERRY_FAMILY_DIAG other class={c} key={k} first={f} stores={t}");
        }
    }
}

/// The word in storage slot `slot` of `obj` (a declared layout's births
/// allocate its slots inline; a slot past the inline storage is the spill's),
/// or `0` (not the hole) when the object has no storage for it.
#[inline(always)]
unsafe fn family_slot_word(obj: *const ObjectHeader, slot: u32) -> u64 {
    if slot < super::object_inline_capacity(obj) {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
            .add(slot as usize);
    }
    super::spill::spill_get_present(obj as usize, slot as usize).unwrap_or(0)
}

/// Is a key after position `pos` of `obj`'s list present (its slot
/// written)? Then a fill of `pos` in place would enumerate out of order.
pub(crate) unsafe fn later_late_key_present(
    obj: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    count: u32,
    pos: u32,
) -> bool {
    // A family object: its descriptor names every key's slot, all inline (a
    // birth allocates them), so the scan is one load per key.
    if let Some(d) = family_desc_of(obj) {
        if d.key_count == count
            && !d.keys.is_null()
            && d.slot_count <= super::object_inline_capacity(obj)
        {
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            return std::slice::from_raw_parts(d.keys, d.key_count as usize)[pos as usize + 1..]
                .iter()
                .any(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE);
        }
    }
    // Any present key after `pos` (a required one always is, an optional or
    // an added one once written): node appends a new key after it.
    (pos + 1..count).any(|p| {
        family_slot_word(obj, super::key_attrs::key_storage_slot(keys, p)) != crate::value::TAG_HOLE
    })
}

/// A plain `[[Set]]` of string key `key_bits` (its bytes `bytes`) with
/// `value` on `obj`, which is its own receiver, when the store adds a key to
/// a layout's own terms ([`family_add`]): an optional key whose slot holds
/// the hole (a family's late key, a runtime layout's optional member), or a
/// key a family layout does not name. `false` leaves the store to the
/// caller's ordinary path: not a family object, a present key, a key the
/// overflow holds, a constructor key, and an add that is not plain (a
/// non-extensible object, a prototype chain that may intercept it). That path
/// sees an unwritten late key as absent (`key_view`, `proxy::own_set_descriptor`)
/// and, when it creates the property, comes back through
/// [`family_create`].
///
/// # Safety
/// `obj` is a live object or null; `key_bits` a live string value whose text
/// is `bytes`; the caller holds no other raw heap pointer across the call
/// (the overflow may allocate).
pub(crate) unsafe fn family_store(
    obj: *mut ObjectHeader,
    key_bits: u64,
    bytes: &[u8],
    value: u64,
) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let family = family_desc_of(obj).is_some();
    if !family && !super::key_view::object_may_have_holes(obj) {
        return false;
    }
    // A private field's by-name write is the private path's: it fills or
    // overwrites the element, never a property of that spelling.
    if super::field_get_set::private_namespace_bytes(bytes) {
        return false;
    }
    FAMILY_DIAG.seen.fetch_add(1, Relaxed);
    if family && family_overwrite(obj, bytes, value) {
        return true;
    }
    let add = match family_add_kind(obj, bytes) {
        // A key outside a layout that is not a family's: the ordinary add.
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    let receiver =
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
        || super::shape_store::chain_blocks(receiver, bytes)
    {
        FAMILY_DIAG.declined.fetch_add(1, Relaxed);
        return false;
    }
    family_add(obj, key_bits, add, value);
    true
}

/// The `CreateDataProperty` step of an ordinary `[[Set]]` whose receiver
/// `receiver` does not hold key `key` (a string value): the caller's walk has
/// decided the add (the chain does not intercept it, the receiver is
/// extensible). For a family object, and for a layout's optional key whose
/// slot holds the hole on any object with such a layout, this makes the add
/// ([`family_add`]); `false` otherwise, and the caller defines the property.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_create(receiver: f64, key: f64, value: f64) -> bool {
    let bits = receiver.to_bits();
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG || !key_value.is_any_string()
    {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let family = family_desc_of(obj).is_some();
    if !family && !super::key_view::object_may_have_holes(obj) {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    let add = match family_add_kind(obj, bytes) {
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    family_add(obj, key.to_bits(), add, value.to_bits());
    true
}

/// The position of the key spelled `bytes` in descriptor `d`'s key sequence:
/// one probe of the descriptor's immutable hash index (§3.3), no key-list
/// scan.
#[inline]
unsafe fn desc_position_by_bytes(d: &PerryLayoutDesc, bytes: &[u8]) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    desc_position_hashed(d, bytes, super::key_bytes_hash(bytes.as_ptr(), bytes.len()))
}

/// [`desc_position_by_bytes`] for `bytes` whose hash is `hash`.
#[inline]
pub(crate) unsafe fn desc_position_hashed(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: u64,
) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let mask = d.index_mask as u64;
    let mut at = hash & mask;
    for _ in 0..=mask {
        let pos = *d.index.add(at as usize);
        if pos == u32::MAX {
            return None;
        }
        if pos < d.key_count && {
            let atom = (*d.keys.add(pos as usize)).atom;
            (*atom).hash == hash && atom_bytes(atom) == Some(bytes)
        } {
            return Some(pos);
        }
        at = (at + 1) & mask;
    }
    None
}

/// `obj`'s own key spelled `bytes`, answered by its family layout alone
/// when `obj` is a family object with no overflow (every own key is then a
/// layout key, present while its slot holds a value): `Some(Some((pos,
/// slot)))` when the layout names the key at key position `pos` and storage
/// slot `slot` (the slot may hold the hole: the key is absent), `Some(None)`
/// when it names no such key (no own key of that name), `None` when `obj` is
/// not such an object. One probe of the descriptor's immutable hash index:
/// no key-list scan. Allocation-free.
///
/// # Safety
/// `obj` is a live object.
#[inline]
pub(crate) unsafe fn family_own_key(
    obj: *const ObjectHeader,
    bytes: &[u8],
) -> Option<Option<(u32, u32)>> {
    family_own_key_hashed(obj, bytes, None)
}

/// [`family_own_key`] with the key's hash (`key_bytes_hash(bytes)`) when the
/// caller has it already (a fixed key's compile-time hash).
///
/// # Safety
/// As [`family_own_key`]; `hash`, when given, is `bytes`' hash.
#[inline]
pub(crate) unsafe fn family_own_key_hashed(
    obj: *const ObjectHeader,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    // The header word names a family descriptor through its shape record
    // (static family words and class-evaluation copies alike); most objects
    // are not family objects and answer from the word's tag bits first.
    let word = (*obj).parent_class_id;
    // Only a static family word (the image's family rows) or a class
    // evaluation's copy (a mixed-kind word) can name a descriptor; both tests
    // read the word alone, before any shape record.
    if word & LAYOUT_SHADOW_BIT != 0
        || !(word_in_layout_index(word)
            || super::shapes::shape_word_kind(word) == super::shapes::SHAPE_ID_KIND_MIXED)
    {
        return None;
    }
    let d = &*desc_at(layout_index_of_word(word)?);
    if d.class_id == 0 {
        return None;
    }
    desc_own_key(d, bytes, hash)
}

/// [`family_own_key_hashed`] of an object whose family descriptor the caller
/// already holds (`d`, from its layout word or its shape record).
///
/// # Safety
/// `d` is a registered class family descriptor; `hash`, when given, is
/// `bytes`' hash.
#[inline(always)]
pub(crate) unsafe fn desc_own_key(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    // A private element is no own property (the index omits it too). A
    // layout lists each spelling once, so only a match is checked.
    let own = |pos: usize, k: &PerryLayoutKey| {
        (k.attrs & super::key_attrs::ENTRY_PRIVATE == 0).then_some((pos as u32, k.slot))
    };
    if let Some(pos) = leading_position(bytes).filter(|&pos| (pos as usize) < keys.len()) {
        let k = &keys[pos as usize];
        if atom_bytes(k.atom) == Some(bytes) {
            return Some(own(pos as usize, k));
        }
    }
    // A short list: its keys' lengths, then their bytes, with no hashing.
    if keys.len() <= SHORT_FAMILY_KEYS {
        for (pos, k) in keys.iter().enumerate() {
            if atom_bytes(k.atom) == Some(bytes) {
                return Some(own(pos, k));
            }
        }
        return Some(None);
    }
    let hash = hash.unwrap_or_else(|| super::key_bytes_hash(bytes.as_ptr(), bytes.len()));
    Some(desc_position_hashed(d, bytes, hash).map(|pos| (pos, keys[pos as usize].slot)))
}

/// A family layout with at most this many keys is searched by comparing
/// them, cheaper than hashing the name for the index.
const SHORT_FAMILY_KEYS: usize = 8;

/// The key position a family layout over node's EventEmitter gives the
/// emitter's state keys: they lead it, in `EventEmitter.init`'s order
/// (`perry-codegen` `layout_families`, the native parent's init keys). Only
/// a first guess for [`family_own_key`], which compares the key there.
#[inline(always)]
fn leading_position(bytes: &[u8]) -> Option<u32> {
    match bytes {
        b"_events" => Some(0),
        b"_eventsCount" => Some(1),
        b"_maxListeners" => Some(2),
        _ => None,
    }
}

/// A store of a present key of family object `obj`: a plain writable data
/// entry of its overflow, or (no overflow) a layout key whose entry has no
/// attribute but optional (every family key is, so no store cache serves it,
/// `key_attrs::entry_is_plain_overwrite`) and whose slot holds a value. The
/// store overwrites in place; `false` for anything else.
unsafe fn family_overwrite(obj: *mut ObjectHeader, bytes: &[u8], value: u64) -> bool {
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return false;
    }
    let table = super::overflow::table(obj);
    if !table.is_null() {
        // The overflow holds every present key (a layout key as a reference
        // to its slot): a live plain data entry is overwritten in place.
        let Some(entry) = super::overflow::find_bytes(table, bytes) else {
            return false;
        };
        let attrs = super::overflow::entry_attrs(table as *const crate::array::ArrayHeader, entry);
        return super::key_attrs::entry_is_plain_writable_data(attrs)
            && super::overflow::store_index(obj, entry, value);
    }
    // A family layout without an overflow: the descriptor's index names the
    // key's slot.
    if let Some(d) = family_desc_of(obj) {
        if d.slot_count <= super::object_inline_capacity(obj) {
            let Some(pos) = desc_position_by_bytes(d, bytes) else {
                return false;
            };
            let k = &*d.keys.add(pos as usize);
            if k.attrs != LAYOUT_KEY_LATE || family_slot_word(obj, k.slot) == crate::value::TAG_HOLE
            {
                return false;
            }
            super::store_object_field_slot(obj, k.slot as usize, value);
            return true;
        }
    }
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return false;
    }
    let Some(pos) =
        super::keys_find_property_slot_by_bytes(keys, record.logical_key_count(), bytes)
    else {
        return false;
    };
    if super::key_attrs::keys_entry(keys, pos) != super::key_attrs::ENTRY_OPTIONAL {
        return false;
    }
    let slot = super::key_attrs::key_storage_slot(keys, pos);
    if slot >= super::object_inline_capacity(obj)
        || family_slot_word(obj, slot) == crate::value::TAG_HOLE
    {
        return false;
    }
    super::store_object_field_slot(obj, slot as usize, value);
    true
}

/// How a store of the key spelled `bytes` adds it to family object `obj`, or
/// `None` when it is not an add this module makes (a present key, a key the
/// overflow holds, a constructor key).
#[derive(Clone, Copy)]
enum FamilyAdd {
    /// A late key at list position `pos`, its slot `slot` holding the hole.
    Late { pos: u32, slot: u32 },
    /// A key the layout does not name.
    Other,
}

unsafe fn family_add_kind(obj: *mut ObjectHeader, bytes: &[u8]) -> Option<FamilyAdd> {
    // A family layout without an overflow: the descriptor's index answers.
    if !super::overflow::has_overflow(obj) {
        if let Some(d) = family_desc_of(obj) {
            if d.slot_count <= super::object_inline_capacity(obj) {
                return match desc_position_by_bytes(d, bytes) {
                    Some(pos) => {
                        let k = &*d.keys.add(pos as usize);
                        (k.attrs & LAYOUT_KEY_LATE != 0
                            && family_slot_word(obj, k.slot) == crate::value::TAG_HOLE)
                            .then_some(FamilyAdd::Late { pos, slot: k.slot })
                    }
                    None => Some(FamilyAdd::Other),
                };
            }
        }
    }
    let record = super::shapes::object_shape_record(obj)?;
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    let count = record.logical_key_count();
    let position = if keys.is_null() {
        None
    } else {
        super::keys_find_property_slot_by_bytes(keys, count, bytes)
    };
    let held_by_overflow = || {
        super::overflow::has_overflow(obj)
            && super::overflow::find_bytes(super::overflow::table(obj), bytes).is_some()
    };
    match position {
        Some(pos) => {
            if super::key_attrs::keys_entry(keys, pos) & super::key_attrs::ENTRY_OPTIONAL == 0 {
                return None;
            }
            let slot = super::key_attrs::key_storage_slot(keys, pos);
            if family_slot_word(obj, slot) != crate::value::TAG_HOLE || held_by_overflow() {
                return None;
            }
            Some(FamilyAdd::Late { pos, slot })
        }
        None if held_by_overflow() => None,
        None => Some(FamilyAdd::Other),
    }
}

/// Add the key `key_bits` to family object `obj` with `value` (see the
/// section comment): a late key in order fills its slot in place; out of
/// order, or with the overflow installed, it is appended to the overflow
/// with a reference to its slot; a key the layout does not name is an
/// overflow value.
unsafe fn family_add(obj: *mut ObjectHeader, key_bits: u64, add: FamilyAdd, value: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let shadowed = super::overflow::has_overflow(obj);
    let mut out_of_order = false;
    if let FamilyAdd::Late { pos, slot } = add {
        if !shadowed {
            let record = super::shapes::object_shape_record(obj).unwrap();
            let keys = record.keys() as usize as *const crate::array::ArrayHeader;
            out_of_order = later_late_key_present(obj, keys, record.logical_key_count(), pos);
            if out_of_order && family_diag_pairs_on() {
                note_out_of_order_pair(obj, pos);
            }
            // In order: the key's position is its enumeration order. An
            // object something inherits from takes the overflow, whose
            // install sets the shadow bit every inherited-read site tests
            // (step 3).
            if !out_of_order
                && slot < super::object_inline_capacity(obj)
                && !(super::proto_validity::any_prototype_marked()
                    && super::proto_validity::object_is_marked_prototype(obj as usize))
            {
                FAMILY_DIAG.fills.fetch_add(1, Relaxed);
                super::store_object_field_slot(obj, slot as usize, value);
                return;
            }
        }
    }
    if family_diag_on() {
        note_install_cause(match add {
            FamilyAdd::Late { .. } if out_of_order => install_cause::LATE_OUT_OF_ORDER,
            FamilyAdd::Late { .. } => install_cause::LATE_PROTO_OR_SPILL,
            FamilyAdd::Other => install_cause::OTHER_KEY,
        });
        if matches!(add, FamilyAdd::Other) {
            note_other_key(obj, key_bits, !shadowed);
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_u64(value);
    let atom = crate::string::property_atom_from_value(key_bits);
    let atom = scope.root_nanbox_u64(atom);
    if !shadowed {
        FAMILY_DIAG.installs.fetch_add(1, Relaxed);
    }
    let atom_ptr =
        || (atom.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    match add {
        FamilyAdd::Late { .. } => {
            if out_of_order {
                FAMILY_DIAG.out_of_order.fetch_add(1, Relaxed);
            }
            // The overflow first (its tombstone for this hole), then the
            // dynamic writer appends the key with a reference to its slot.
            super::overflow::ensure(owner.get_raw_mut_ptr::<ObjectHeader>());
            super::overflow::store_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
        FamilyAdd::Other => {
            FAMILY_DIAG.other.fetch_add(1, Relaxed);
            super::overflow::store_unfixed_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
    }
}

/// [`family_store`] for NaN-boxed operands: `target` is its own receiver and
/// `key` any string value (a heap or a short string).
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_value(target: f64, key: f64, value: f64) -> bool {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if !family_store_admits(obj) {
        return false;
    }
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if !key_value.is_any_string() {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    // `family_store` reads the text before its first allocation only; a copy
    // keeps the borrow independent of the key string.
    let mut buf = [0u8; 96];
    let owned;
    let bytes: &[u8] = if bytes.len() <= buf.len() {
        buf[..bytes.len()].copy_from_slice(bytes);
        &buf[..bytes.len()]
    } else {
        owned = bytes.to_vec();
        &owned
    };
    family_store(obj, key.to_bits(), bytes, value.to_bits())
}

/// `family_desc_of(obj).is_some() || object_may_have_holes(obj)`, answered
/// from ONE read of the receiver's record: the record names its family
/// descriptor and carries the key list summary. Every dynamic-key store asks
/// this first, so the second record read of the two-call form was paid by
/// every ordinary store.
///
/// # Safety
/// `obj` is a live object or null.
#[inline(always)]
unsafe fn family_store_admits(obj: *const ObjectHeader) -> bool {
    if obj.is_null() || !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
        return false;
    }
    let word = (*obj).parent_class_id;
    if !super::shapes::is_layout_word(layout_id_of_word(word)) {
        return false;
    }
    // `obj` is live and carries `word`: the receiver read needs no check.
    let Some(record) = super::shapes::shape_record_of_receiver(word) else {
        return false;
    };
    if let Some(index) = record.family_desc_index_plus_one().checked_sub(1) {
        if (*desc_at(index)).class_id != 0 {
            return true;
        }
    }
    super::key_view::summary_may_have_holes(record.summary()) && !super::overflow::has_overflow(obj)
}

/// [`family_store`] for a key passed as a string header.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_str(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    if key.is_null()
        || family_desc_of(obj).is_none() && !super::key_view::object_may_have_holes(obj)
    {
        return false;
    }
    let key_bits = crate::JSValue::string_ptr(key as *mut _).bits();
    family_store_value(
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK),
        f64::from_bits(key_bits),
        value,
    )
}

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

// Wasm uses the same records and registration logic; only image enumeration
// differs because relocated data cannot live in Rust's Wasm custom sections.
fn image_atoms() -> impl Iterator<Item = *mut StaticAtom> {
    #[cfg(target_family = "wasm")]
    {
        inventory::iter::<crate::wasm_image::Atom>
            .into_iter()
            .map(|item| item.0.cast_mut())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<StaticAtom>(atom_bounds())
    }
}
fn image_layouts() -> impl Iterator<Item = *mut PerryLayoutDesc> {
    #[cfg(target_family = "wasm")]
    {
        inventory::iter::<crate::wasm_image::Layout>
            .into_iter()
            .map(|item| item.0)
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<PerryLayoutDesc>(layout_bounds())
    }
}

fn descriptor_index(desc: *const PerryLayoutDesc) -> Option<u32> {
    let (start, end) = layout_bounds();
    let at = desc as usize;
    (at >= start && at < end)
        .then(|| ((at - start) / std::mem::size_of::<PerryLayoutDesc>()) as u32)
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
