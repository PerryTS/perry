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
//! which no ShapeId carries. Nothing sets the shadow bit before step 4.

use super::ObjectHeader;
use crate::string::StringHeader;

/// The shadow bit of the layout word (§3.6). Never part of a ShapeId.
pub const LAYOUT_SHADOW_BIT: u32 = 1 << 30;
/// The LayoutId of a layout word.
#[inline(always)]
pub const fn layout_id_of_word(word: u32) -> u32 {
    word & !LAYOUT_SHADOW_BIT
}
// Every ShapeId lies in [0x8000_0000, 0xC000_0000): bit 30 is always clear.
const _: () = assert!(super::shapes::SHAPE_ID_BASE == 0x8000_0000);
const _: () = assert!(super::shapes::SHAPE_ID_END <= 0xC000_0000);

/// Section-ordered layout ids start here: the upper half of the static band.
/// The content-hash band (`assign_static_shape_ids`) stays in the lower half.
pub const LAYOUT_BASE: u32 =
    super::shapes::SHAPE_ID_BASE + crate::codegen_abi::STATIC_SHAPE_ID_COUNT / 2;
const _: () = assert!(LAYOUT_BASE == crate::codegen_abi::SHAPE_ID_BASE + (1 << 19));

/// `PerryLayoutDesc.proto` builtin tags (values below 4096; anything else
/// is a `*const PerryLayoutDesc`, which step 1 introduces).
pub const LAYOUT_PROTO_OBJECT: usize = 0;
pub const LAYOUT_PROTO_NULL: usize = 1;
pub const LAYOUT_PROTO_URL: usize = 2;
pub const LAYOUT_PROTO_READABLE_STATE: usize = 3;
pub const LAYOUT_PROTO_WRITABLE_STATE: usize = 4;
pub const LAYOUT_PROTO_TIMEOUT: usize = 5;
pub const LAYOUT_PROTO_IMMEDIATE: usize = 6;

/// A program-named key: its precomputed FNV-1a hash (the runtime's key hash)
/// and the immutable image string that is its one address.
#[repr(C)]
pub struct StaticAtom {
    pub hash: u64,
    pub string: *const StringHeader,
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
    pub proto: usize,
    pub keys: *const PerryLayoutKey,
    pub key_count: u32,
    /// Index capacity - 1 (a power of two minus one).
    pub index_mask: u32,
    /// Open-addressed: atom hash -> position in `keys`, `u32::MAX` = empty.
    pub index: *const u32,
}

const _: () = assert!(std::mem::size_of::<PerryLayoutDesc>() == 56);
const _: () = assert!(std::mem::offset_of!(PerryLayoutDesc, layout_id) == 12);
const _: () = assert!(std::mem::offset_of!(PerryLayoutDesc, keys) == 32);
const _: () = assert!(std::mem::offset_of!(PerryLayoutDesc, index) == 48);
const _: () = assert!(std::mem::size_of::<PerryLayoutKey>() == 16);
const _: () = assert!(std::mem::size_of::<StaticAtom>() == 16);

/// `PerryLayoutDesc.layout_id` as emitted for a layout some object carries:
/// [`register_static_layouts`] replaces it with the registered id. Every other
/// descriptor is emitted with 0 and stays 0.
pub const LAYOUT_ID_CARRIED: u32 = u32::MAX;
// The descriptor word has exactly three states: 0 (static data), the carried
// sentinel (not registered yet) and a registered ShapeId. The sentinel must
// never read as a ShapeId, or registration would install it as one.
const _: () = assert!(LAYOUT_ID_CARRIED >= super::shapes::SHAPE_ID_END);
const _: () = assert!(super::shapes::SHAPE_ID_BASE != 0);

/// The ShapeId a descriptor word holds, if registration already wrote one
/// (this agent or an earlier one); `None` for 0 and for the carried sentinel.
#[inline]
fn registered_layout_id(word: u32) -> Option<u32> {
    super::shapes::is_shape_id(word).then_some(word)
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
        proto: 0,
        keys: std::ptr::null(),
        key_count: 0,
        index_mask: 0,
        index: std::ptr::null(),
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
        string: std::ptr::null()
    },
    "perry_atoms",
    "__DATA,__perry_atoms",
    ".perryatm$m"
);

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
    if s.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(
        crate::string::string_data(s),
        (*s).byte_len as usize,
    ))
}

/// Pre-seed the agent's atom table, then register every layout. Runs once per
/// agent from `js_shape_run_static_seed`, after the hash-band literal seeds.
pub(crate) fn seed_static_atoms() {
    unsafe {
        for atom in records::<StaticAtom>(atom_bounds()) {
            if let Some(bytes) = atom_bytes(atom) {
                crate::string::seed_static_atom(bytes, (*atom).hash, (*atom).string);
            }
        }
    }
}

pub(crate) fn register_static_layouts() {
    unsafe {
        let mut index = 0u32;
        for desc in records::<PerryLayoutDesc>(layout_bounds()) {
            if (*desc).family == SENTINEL_FAMILY || (*desc).keys.is_null() && (*desc).key_count != 0
            {
                continue;
            }
            // Ids are section positions, so every agent assigns the same one.
            let id = LAYOUT_BASE + index;
            index += 1;
            // Carried: the emitted sentinel (first agent) or the id an earlier
            // agent wrote. Uncarried descriptors stay 0 and are never minted.
            if (*desc).layout_id == 0 {
                continue;
            }
            let got = register_one(desc, id);
            // Every agent writes the same id (same seeds, same order).
            (*(std::ptr::addr_of_mut!((*desc).layout_id) as *const std::sync::atomic::AtomicU32))
                .store(got, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Register one descriptor under `requested`; the id it resolved to, or 0
/// when this runtime cannot represent it yet (a descriptor prototype, a
/// non-ordinary kind, slots not in key order, a key that is not an atom).
unsafe fn register_one(desc: *const PerryLayoutDesc, requested: u32) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).layout_id).cast_mut());
    let existing = registered_layout_id(word.load(Ordering::Acquire));
    if let Some(id) = existing {
        if super::shapes::shape_descriptor_by_id(id).is_some() {
            return id;
        }
    }
    let d = &*desc;
    if d.kind != 0 || requested >= super::shapes::STATIC_SHAPE_ID_END {
        return 0;
    }
    let keys = if d.key_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(d.keys, d.key_count as usize)
    };
    let mut entries: Vec<(&str, f64)> = Vec::with_capacity(keys.len());
    let mut attrs: Vec<u8> = Vec::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        if key.slot as usize != i || key.slot >= d.slot_count {
            return 0;
        }
        let Some(bytes) = atom_bytes(key.atom) else {
            return 0;
        };
        let Ok(name) = std::str::from_utf8(bytes) else {
            return 0;
        };
        entries.push((name, f64::from_bits(crate::value::TAG_UNDEFINED)));
        attrs.push(key.attrs);
    }
    let proto_id = match d.proto {
        LAYOUT_PROTO_OBJECT => super::shapes::PROTO_ID_DEFAULT,
        LAYOUT_PROTO_NULL => super::shapes::PROTO_ID_NULL,
        LAYOUT_PROTO_URL..=LAYOUT_PROTO_IMMEDIATE => super::shapes::fresh_unique_proto_id(),
        _ => return 0,
    };
    let key_count = d.key_count;
    let slot_count = d.slot_count;
    let rep = d.rep;
    let integrity = d.integrity as u16;
    // Compatibility key arrays are owned by the one authoritative shape
    // record, and are shared by the canonical key trie when names/attrs agree.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let list = if key_count == 0 {
        std::ptr::null_mut()
    } else {
        let list = super::key_attrs::alloc_canonical_key_list(
            key_count,
            true,
            attrs.iter().any(|&a| a != 0),
        );
        let values = crate::array::array_elements_ptr(list);
        let attributes = super::key_attrs::keys_attrs(list);
        for (i, key) in keys.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): unpublished startup list, under suppression.
            *values.add(i) =
                crate::value::JSValue::string_ptr((*key.atom).string.cast_mut()).bits();
            if !attributes.is_null() {
                super::key_attrs::attrs_write(attributes, i as u32, key.attrs, entries[i].0);
            }
        }
        (*list).length = key_count;
        super::canonical_keys::canonicalize(
            &super::canonical_keys::SharedLayout::shape_cache_entry(),
            list,
            key_count,
        )
        .view()
        .arr()
    };
    let summary = super::key_attrs::keys_summary(list, key_count);
    let found = super::shapes::shape_descriptor_find_with_rep(
        list,
        key_count,
        slot_count,
        0,
        super::shapes::ShapeObjectKind::Ordinary,
        0,
        proto_id,
        summary,
        rep,
        &[],
        integrity,
    )
    .filter(|&id| super::shapes::is_static_shape_id(id));
    let proposed = if let Some(id) = existing {
        id
    } else if let Some(id) = found {
        id
    } else if proto_id == super::shapes::PROTO_ID_DEFAULT {
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
            id, list, key_count, slot_count, proto_id, summary, rep, integrity,
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
    let d = &*desc;
    let Some(id) = registered_layout_id(d.layout_id) else {
        eprintln!(
            "Perry internal error: perry_alloc_layout of an unregistered layout (family {:#x})",
            d.family
        );
        std::process::abort();
    };
    let size = std::mem::size_of::<ObjectHeader>() + d.slot_count as usize * 8;
    let obj =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT).cast::<ObjectHeader>();
    // GC_STORE_AUDIT(INIT): unpublished header and hole-only declared slots.
    std::ptr::write(
        obj,
        ObjectHeader {
            class_id: 0,
            parent_class_id: id,
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
    super::shapes::store_kind::premark_plain_ordinary(obj);
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
    if d.proto == LAYOUT_PROTO_NULL {
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
    }
    if d.integrity != 0
        || (d.key_count != 0
            && std::slice::from_raw_parts(d.keys, d.key_count as usize)
                .iter()
                .any(|key| key.attrs != 0))
    {
        (*gc)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
    }
    (*gc)._reserved |= d.integrity as u16;
    super::shapes::stamp_object_shape_id_with_carrier_note(obj, id);
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
