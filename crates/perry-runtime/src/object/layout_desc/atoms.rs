//! The `perry_atoms` section: the compiler-emitted atom records, the shadow-name
//! descriptor, the section bounds and the reader over them, and the two seeding
//! entry points (atom table seeding and the coloured-key census). This is the
//! atom half of the compiler-owned image; `layout_desc` keeps the layout half.
use super::*;

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

/// The entry module supplies the strong, immutable program descriptor.
/// Runtime-only binaries use this empty weak definition. Its address is
/// resolved by the linker, never registered, memoized or changed at startup.
#[used(linker)]
#[linkage = "weak"]
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

/// (start, end) of the atom section.
pub(super) fn atom_bounds() -> (usize, usize) {
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
    #[cfg(not(any(target_vendor = "apple", target_os = "windows")))]
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
}

/// The bytes of an atom's image string.
pub(crate) unsafe fn atom_bytes<'a>(atom: *const StaticAtom) -> Option<&'a [u8]> {
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
    // Ordered by the program's own numbers (colour bit, slot, hash). A
    // coloured key has one (bit, slot), so equal triples are one atom image
    // repeated; the bytes never need comparing.
    out.sort_unstable_by_key(|a| (a.2, a.3, a.1));
    out.dedup_by_key(|a| (a.2, a.3, a.1));
    out
}

/// Pre-seed the agent's atom table, then register every layout. Runs once per
/// agent from `js_shape_run_static_seed`, after the hash-band literal seeds.
pub(crate) fn seed_static_atoms() {
    #[cfg(target_family = "wasm")]
    assert!(
        image_atoms().next().is_some(),
        "Wasm image constructors did not run"
    );
    crate::string::reserve_static_atoms(
        super::super::runtime_layouts::PERRY_ASCII_CHARS.0.len() + image_atoms().count(),
    );
    super::super::runtime_layouts::seed_ascii_char_atoms();
    unsafe {
        for atom in image_atoms() {
            if let Some(bytes) = atom_bytes(atom) {
                crate::string::seed_static_atom(bytes, (*atom).hash, (*atom).string);
            }
        }
    }
    crate::object::layout_probe::seed_colours();
    #[cfg(all(feature = "wasm-image-selftest", target_family = "wasm"))]
    wasm_program_image_checks::check_atoms();
}

pub(crate) fn image_atoms() -> impl Iterator<Item = *mut StaticAtom> {
    #[cfg(target_family = "wasm")]
    {
        records::<StaticAtom>(atom_bounds()).chain(
            inventory::iter::<crate::wasm_image::Atom>
                .into_iter()
                .map(|item| item.0.cast_mut()),
        )
    }
    #[cfg(not(target_family = "wasm"))]
    {
        records::<StaticAtom>(atom_bounds())
    }
}
