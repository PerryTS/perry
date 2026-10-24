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

/// Shadowed words: an ordinary-band ShapeId (`is_site_matchable_shape_id`)
/// with bit 30 set, `[0xC000_0000, 0xF000_0000)`. Only those ids carry the
/// bit, so the band is exact: the word never holds a class id from it
/// (class ids are small counters, `0x7FFF_FFxx` or `0xFFFF_xxxx`; synthetic
/// class ids live only in `ObjectHeader.class_id`).
pub const SHADOWED_WORD_BASE: u32 = super::shapes::SHAPE_ID_BASE | LAYOUT_SHADOW_BIT;
pub const SHADOWED_WORD_END: u32 = super::shapes::SITE_SHAPE_ID_END | LAYOUT_SHADOW_BIT;
const _: () = assert!(SHADOWED_WORD_BASE == 0xC000_0000 && SHADOWED_WORD_END == 0xF000_0000);
/// The emitted site test (`perry-codegen` `proto_constants`): a layout word
/// below this is an unshadowed ordinary layout or a word no write stamped.
/// Shadowed and exotic words all lie at or above it.
pub const UNSHADOWED_WORD_END: u32 = super::shapes::SITE_SHAPE_ID_END;
const _: () = assert!(UNSHADOWED_WORD_END == crate::codegen_abi::UNSHADOWED_WORD_END);

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
    /// `is_shape_id` accepts, when an overflow is installed (no layout
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
        && id < super::shapes::SITE_SHAPE_ID_END
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
}

const _: () = assert!(
    std::mem::size_of::<PerryLayoutDesc>()
        == if cfg!(target_pointer_width = "32") {
            56
        } else {
            72
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
    super::shapes::is_shape_id(word).then_some(word)
}

/// Read the existing registration word atomically: an ordinary function's
/// first construction may publish its id from another agent.
#[inline]
pub(crate) unsafe fn load_layout_id(desc: *const PerryLayoutDesc) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    AtomicU32::from_ptr(std::ptr::addr_of!((*desc).layout_id).cast_mut()).load(Ordering::Acquire)
}

/// A section sentinel's family: not a layout.
const SENTINEL_FAMILY: u32 = u32::MAX;

macro_rules! section_sentinel {
    ($name:ident, $ty:ty, $init:expr, $elf:literal, $mach:literal, $win:literal) => {
        // Always create the section (programs without layouts, Rust tests):
        // `used(linker)` keeps it, and `bounds` references it.
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = $mach)]
        #[cfg_attr(target_os = "windows", link_section = $win)]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = $elf
        )]
        static mut $name: $ty = $init;
    };
}

section_sentinel!(
    LAYOUT_SECTION_SENTINEL,
    PerryLayoutDesc,
    PerryLayoutDesc {
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
    ATOM_SECTION_SENTINEL,
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
static LAYOUT_START: [u64; 1] = [0];
#[cfg(target_os = "windows")]
#[used(linker)]
#[link_section = ".perrylay$z"]
static LAYOUT_END: [u64; 1] = [0];
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
            // Every agent writes the same id (same seeds, same order).
            (*(std::ptr::addr_of_mut!((*desc).layout_id) as *const std::sync::atomic::AtomicU32))
                .store(got, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// A module namespace layout's id, drawn at startup in section order (so
/// every agent draws the same one) without building its key list or shape:
/// [`register_namespace_layout`] installs them at the namespace's birth. Most
/// compiled namespaces are never built.
unsafe fn reserve_namespace_layout(desc: *const PerryLayoutDesc) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).layout_id).cast_mut());
    if let Some(id) = registered_layout_id(word.load(Ordering::Acquire)) {
        return id;
    }
    debug_assert_eq!((*desc).proto, LAYOUT_PROTO_NULL);
    let proposed = super::shapes::allocate_declared_layout_id(super::shapes::PROTO_ID_NULL);
    match word.compare_exchange(
        LAYOUT_ID_CARRIED,
        proposed,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => proposed,
        Err(other) => registered_layout_id(other)
            .expect("a carried layout word holds the sentinel or a ShapeId"),
    }
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
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).layout_id).cast_mut());
    let existing = registered_layout_id(word.load(Ordering::Acquire));
    let function = (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR;
    if function && function_proto.is_none() {
        return existing.unwrap_or(0);
    }
    let local = binding_symbol_keys(&*desc);
    if let Some(id) = existing {
        if (local != 0 && bound.is_empty()) || super::shapes::shape_descriptor_by_id(id).is_some() {
            return id;
        }
    }
    if local != 0 && !bound.is_empty() && bound.len() != local {
        return 0;
    }
    let d = &*desc;
    let Some(kind) = declared_kind(d.kind) else {
        return 0;
    };
    if !function && requested >= super::shapes::STATIC_SHAPE_ID_END {
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
        return match word.compare_exchange(
            LAYOUT_ID_CARRIED,
            proposed,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => proposed,
            Err(other) => registered_layout_id(other)
                .expect("a carried layout word holds the sentinel or a ShapeId"),
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
    let found = super::shapes::shape_descriptor_find_with_rep(
        list,
        key_count,
        slot_count,
        0,
        kind,
        0,
        proto_id,
        summary,
        rep,
        &[],
        integrity,
    )
    .filter(|&id| {
        super::shapes::is_static_shape_id(id)
            && !class_family
            && !image
            && d.variant != crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
    });
    let proposed = if let Some(id) = existing {
        id
    } else if function {
        super::shapes::allocate_declared_layout_id(proto_id)
    } else if let Some(id) = found {
        id
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
        match word.compare_exchange(
            LAYOUT_ID_CARRIED,
            proposed,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => proposed,
            Err(other) => registered_layout_id(other)
                .expect("a carried layout word holds the sentinel or a ShapeId"),
        }
    };
    if super::shapes::shape_descriptor_by_id(id).is_none() {
        super::shapes::install_declared_layout(
            id, list, key_count, slot_count, kind, proto_id, summary, rep, integrity, generation,
        );
    }
    super::shapes::note_external_shape_carrier(super::shapes::shape_descriptor_by_id(id));
    id
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
/// evaluation's copy of that layout (`shapes::evaluation_layout_family(copy)`
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
pub(crate) unsafe fn alloc_function_layout(desc: *const PerryLayoutDesc) -> *mut ObjectHeader {
    let d = &*desc;
    let id =
        registered_layout_id(load_layout_id(desc)).expect("registered function constructor layout");
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
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    mark_layout_object_facts(obj, d);
    // The unpublished header already carries this declared layout. Birth
    // creates carrier duty, not a structural mutation: it must not enter
    // the mutation publisher (which can inspect lazily materialized keys).
    if !crate::arena::pointer_in_nursery(obj as usize) {
        super::shapes::note_old_generation_carrier(super::shapes::shape_record_by_id(id));
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
    let base = super::shapes::shape_descriptor_by_id(id)?;
    let proto_bits = proto.get_nanbox_u64();
    if super::shapes::identity_prototype_word(proto_id) != proto_bits {
        super::shapes::write_identity_word(proto_id, proto_bits);
    }
    let shape = match super::shapes::shape_keys_and_proto_by_id(recorded) {
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
        *fields.add(i) = crate::value::TAG_HOLE;
    }
    mark_layout_object_facts(obj, d);
    if !crate::arena::pointer_in_nursery(obj as usize) {
        super::shapes::note_old_generation_carrier(super::shapes::shape_record_by_id(shape));
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
    if !bound.is_empty() && register_one_bound(d, id, bound) != id {
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

include!("layout_desc_family.rs");
