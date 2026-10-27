//! Descriptor-owned canonical lists. The descriptor's key array is the only
//! description: const evaluation supplies its strings, attributes and slots.
//! Native relocations carry string tags; Apple tags and process-lifetime
//! symbols are resolved once, before publication in any agent's trie.
use super::layout_desc::PerryLayoutKey;
use crate::array::ArrayHeader;
use crate::gc::{GcHeader, GC_HEADER_SIZE};
use std::cell::UnsafeCell;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union ImageWord {
    bits: u64,
    #[cfg(target_pointer_width = "64")]
    pointer: *const u8,
    #[cfg(target_pointer_width = "32")]
    parts: PointerParts,
}
#[cfg(target_pointer_width = "32")]
#[repr(C)]
#[derive(Clone, Copy)]
struct PointerParts {
    pointer: *const u8,
    tag: u32,
}

impl ImageWord {
    const fn pointer(pointer: *const u8, tag: u64) -> Self {
        #[cfg(all(target_pointer_width = "64", not(target_vendor = "apple")))]
        {
            Self {
                pointer: pointer.wrapping_add(tag as usize),
            }
        }
        #[cfg(all(target_pointer_width = "64", target_vendor = "apple"))]
        {
            let _ = tag;
            Self { pointer }
        }
        #[cfg(target_pointer_width = "32")]
        {
            Self {
                parts: PointerParts {
                    pointer,
                    tag: (tag >> 32) as u32,
                },
            }
        }
    }
}

#[repr(C, align(8))]
pub(crate) struct AttributeCell<const N: usize> {
    gc: GcHeader,
    array: ArrayHeader,
    slots: [u64; N],
}
#[repr(C, align(8))]
pub(crate) struct ImageList<const N: usize, const A: usize> {
    gc: GcHeader,
    pub(crate) array: ArrayHeader,
    front: [ImageWord; A],
    slots: [ImageWord; N],
    attrs: [AttributeCell<N>; A],
}
/// Storage is writable only for the target's one-time relocation fixups.
#[repr(transparent)]
pub(crate) struct ImageStorage<T>(pub(crate) UnsafeCell<T>);
unsafe impl<T> Sync for ImageStorage<T> {}

pub(crate) const fn has_attributes(keys: &[PerryLayoutKey]) -> bool {
    let mut i = 0;
    while i < keys.len() {
        if keys[i].attrs != 0 || keys[i].slot as usize != i {
            return true;
        }
        i += 1;
    }
    false
}
const fn mapped(keys: &[PerryLayoutKey]) -> bool {
    let mut i = 0;
    while i < keys.len() {
        if keys[i].slot as usize != i {
            return true;
        }
        i += 1;
    }
    false
}
const fn array(length: usize) -> ArrayHeader {
    ArrayHeader {
        length: length as u32,
        capacity: length as u32,
        named: std::ptr::null_mut(),
        #[cfg(target_pointer_width = "32")]
        _ek_pad: 0,
        ek_prefix: 0,
        ek_kind: 0,
    }
}
const fn header(size: usize, reserved: u16) -> GcHeader {
    GcHeader {
        obj_type: crate::gc::GC_TYPE_ARRAY,
        gc_flags: crate::value::addr_class::realm::IMAGE_CELL_FLAGS
            | crate::gc::GC_FLAG_SHAPE_SHARED,
        _reserved: reserved,
        size: size as u32,
    }
}
impl<const N: usize, const A: usize> ImageList<N, A> {
    pub(crate) const fn new(keys: &[PerryLayoutKey], address: *const u8) -> Self {
        assert!(keys.len() == N && A == has_attributes(keys) as usize);
        let size = GC_HEADER_SIZE + std::mem::size_of::<ArrayHeader>() + 8 * (N + A);
        let mut result = Self {
            gc: header(
                size,
                perry_abi::GC_LAYOUT_SIDE_MASK
                    | perry_abi::GC_LAYOUT_ALL_POINTERS
                    | if A != 0 {
                        crate::gc::GC_ARRAY_KEY_ATTRS
                    } else {
                        0
                    },
            ),
            array: array(N),
            front: [ImageWord { bits: 0 }; A],
            slots: [ImageWord {
                bits: crate::value::TAG_UNDEFINED,
            }; N],
            attrs: [const {
                AttributeCell {
                    gc: header(
                        GC_HEADER_SIZE + std::mem::size_of::<ArrayHeader>() + 8 * N,
                        perry_abi::GC_LAYOUT_POINTER_FREE,
                    ),
                    array: array(N),
                    slots: [0; N],
                }
            }; A],
        };
        if A != 0 {
            result.front[0] = ImageWord::pointer(
                address.wrapping_add(size + GC_HEADER_SIZE),
                crate::value::POINTER_TAG,
            );
        }
        let mapped = mapped(keys);
        let mut previous = 0;
        let mut i = 0;
        while i < N {
            let atom = unsafe { &*keys[i].atom };
            let string = atom.kind == crate::codegen_abi::ATOM_KIND_STRING;
            if string {
                result.slots[i] = ImageWord::pointer(atom.string.cast(), crate::value::STRING_TAG);
            }
            if A != 0 {
                previous = perry_abi::canonical_key_attribute_word(
                    previous,
                    keys[i].attrs,
                    if string { 1u16 << (atom.hash >> 60) } else { 0 },
                    if mapped { Some(keys[i].slot) } else { None },
                );
                result.attrs[0].slots[i] = previous;
            }
            i += 1;
        }
        result
    }
}

macro_rules! image_key_list {
    ($keys:expr) => {{
        #[cfg(not(target_family = "wasm"))]
        {
            use crate::object::image_keys::{ImageList, ImageStorage};
            const N: usize = $keys.len();
            const A: usize = crate::object::image_keys::has_attributes(&$keys) as usize;
            #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
            #[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
            #[cfg_attr(
                not(any(target_vendor = "apple", target_os = "windows")),
                link_section = "perry_realm"
            )]
            static CELL: ImageStorage<ImageList<N, A>> = ImageStorage(std::cell::UnsafeCell::new(
                ImageList::new(&$keys, std::ptr::addr_of!(CELL).cast()),
            ));
            if N == 0 {
                std::ptr::null_mut()
            } else {
                unsafe { std::ptr::addr_of_mut!((*CELL.0.get()).array) }
            }
        }
        #[cfg(target_family = "wasm")]
        {
            std::ptr::null_mut()
        }
    }};
}
pub(crate) use image_key_list;

/// Complete only relocations the target cannot express. A compare/exchange
/// performs at most one write per slot; once published, every agent sees the
/// same immutable words. Module-local symbols deliberately have no image list.
pub(crate) unsafe fn prepare(keys: &[PerryLayoutKey], cell: *mut ArrayHeader) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    if cell.is_null() {
        return false;
    }
    if !crate::value::addr_class::realm::is_image_object(cell as usize) {
        return false;
    }
    if (*cell).length as usize != keys.len() {
        return false;
    }
    #[cfg(all(target_vendor = "apple", target_pointer_width = "64"))]
    if has_attributes(keys) {
        let front = AtomicU64::from_ptr(cell.add(1).cast());
        let raw = front.load(Ordering::Acquire);
        if raw >> 48 == 0 {
            let _ = front.compare_exchange(
                raw,
                raw | crate::value::POINTER_TAG,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    let values = crate::array::array_elements_ptr(cell);
    for (i, key) in keys.iter().enumerate() {
        let atom = &*key.atom;
        if atom.kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
            return false;
        }
        let string = atom.kind == crate::codegen_abi::ATOM_KIND_STRING;
        #[cfg(not(all(target_vendor = "apple", target_pointer_width = "64")))]
        if string {
            continue;
        }
        let expected = if string {
            atom.string as u64
        } else {
            crate::value::TAG_UNDEFINED
        };
        let actual = super::layout_desc::layout_key_word(key, &[], &mut 0);
        let word = AtomicU64::from_ptr(values.add(i));
        // GC_STORE_AUDIT(INIT): unpublished immutable facts; the string or
        // process-lifetime symbol is immortal and introduces no heap edge.
        let _ = word.compare_exchange(expected, actual, Ordering::AcqRel, Ordering::Acquire);
    }
    true
}
