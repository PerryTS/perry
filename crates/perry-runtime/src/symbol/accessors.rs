use super::{obj_key_from_f64, sym_key_from_f64, TAG_UNDEFINED};

#[derive(Clone, Copy)]
pub(super) struct SymbolAccessorDescriptor {
    pub(super) get: u64,
    pub(super) set: u64,
}

pub(super) fn clear_symbol_accessor_property(obj_key: usize, sym_key: usize) {
    if let Some(entry) = unsafe { crate::object::shaped_symbols::entry(obj_key, sym_key) } {
        if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
            unsafe {
                crate::object::shaped_symbols::define(
                    obj_key,
                    sym_key,
                    TAG_UNDEFINED,
                    entry & !crate::object::key_attrs::ENTRY_ACCESSOR_MASK,
                );
            }
        }
    }
}

#[doc(hidden)]
pub fn symbol_accessor_held_for_test(owner: usize, symbol: usize) -> bool {
    unsafe { crate::object::shaped_symbols::accessor(owner, symbol).is_some() }
}

/// One bit per symbol that has ever carried an accessor, hashed by the
/// symbol's ID (#10481). `obj[sym] = v` consults it before walking the
/// prototype chain for an inherited setter: a clear bit proves no accessor
/// exists under that symbol anywhere, so the walk cannot find one and the
/// write goes straight to the own-data store. A set bit is only a maybe, and
/// costs the walk that a correct answer needed anyway.
///
/// Keyed by `SymbolHeader::id`, not by the symbol's ADDRESS: a moving
/// collection rewrites the owner's shape keys but copies the id verbatim, so an
/// id-keyed filter needs no rescan, no rekey and no GC root. Monotonic —
/// removing an accessor leaves the bit set, which only costs a walk.
static SYMBOL_ACCESSOR_IDS: [std::sync::atomic::AtomicU64; SYMBOL_ACCESSOR_ID_WORDS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; SYMBOL_ACCESSOR_ID_WORDS];
const SYMBOL_ACCESSOR_ID_WORDS: usize = 4;

/// `(word, mask)` for a symbol's move-stable mixed id. Collisions are
/// conservative: a set bit always takes the full inherited-accessor walk.
///
/// # Safety
/// `sym_key` must be a live `SymbolHeader` address (a non-zero
/// `sym_key_from_f64` result, which has already checked the magic).
#[inline]
unsafe fn symbol_accessor_id_bit(sym_key: usize) -> (usize, u64) {
    let id = (*(sym_key as *const crate::symbol::SymbolHeader)).id;
    let bit = (id % (SYMBOL_ACCESSOR_ID_WORDS as u64 * 64)) as usize;
    (bit / 64, 1u64 << (bit % 64))
}

/// `false` ⟹ no accessor has ever been installed under this symbol, on any
/// object. Checked before the inherited-accessor walk on the write path.
///
/// # Safety
/// Same contract as [`symbol_accessor_id_bit`].
#[inline]
pub(super) unsafe fn symbol_may_have_accessor(sym_key: usize) -> bool {
    let (word, mask) = symbol_accessor_id_bit(sym_key);
    SYMBOL_ACCESSOR_IDS[word].load(std::sync::atomic::Ordering::Acquire) & mask != 0
}

/// Record that `sym_key` carries an accessor. Published BEFORE the shape
/// store, so a reader that sees a clear bit cannot miss the entry.
///
/// # Safety
/// Same contract as [`symbol_accessor_id_bit`].
#[inline]
unsafe fn note_symbol_accessor_key(sym_key: usize) {
    let (word, mask) = symbol_accessor_id_bit(sym_key);
    SYMBOL_ACCESSOR_IDS[word].fetch_or(mask, std::sync::atomic::Ordering::AcqRel);
}

#[cfg(test)]
pub(crate) fn test_symbol_accessor_id_bits_set() -> u32 {
    SYMBOL_ACCESSOR_IDS
        .iter()
        .map(|w| w.load(std::sync::atomic::Ordering::Acquire).count_ones())
        .sum()
}

pub(crate) unsafe fn set_symbol_accessor_property(
    obj_f64: f64,
    sym_f64: f64,
    get_bits: u64,
    set_bits: u64,
) {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return;
    }
    crate::symbol::note_symbol_key_installed(sym_key);
    crate::symbol::note_symbol_owner_installed(obj_key);
    note_symbol_accessor_key(sym_key);
    let installed =
        crate::object::shaped_symbols::define_accessor(obj_key, sym_key, get_bits, set_bits);
    debug_assert!(
        installed,
        "symbol accessor owner must have a property holder"
    );
}

pub(super) unsafe fn symbol_accessor_property(
    obj_f64: f64,
    sym_f64: f64,
) -> Option<SymbolAccessorDescriptor> {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    unsafe { crate::object::shaped_symbols::accessor(obj_key, sym_key) }
        .map(|(get, set)| SymbolAccessorDescriptor { get, set })
}

pub(super) fn symbol_accessor_property_by_key(
    obj_key: usize,
    sym_key: usize,
) -> Option<SymbolAccessorDescriptor> {
    unsafe { crate::object::shaped_symbols::accessor(obj_key, sym_key) }
        .map(|(get, set)| SymbolAccessorDescriptor { get, set })
}

/// Setter twin of [`invoke_symbol_accessor_getter`] (#10481): runs `set_bits`
/// with `this === receiver` and returns the assigned value. The value is
/// re-read from its root afterwards — the setter body is user code, so a
/// collection inside it can move whatever `value` points at.
pub(super) unsafe fn invoke_symbol_accessor_setter(
    set_bits: u64,
    receiver: f64,
    value: f64,
) -> f64 {
    let closure = (set_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if set_bits == 0 || closure.is_null() {
        return value;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    crate::closure::js_closure_call1(
        closure,
        crate::closure::JsThis::from_f64(receiver),
        value_h.get_nanbox_f64(),
    );
    value_h.get_nanbox_f64()
}

pub(super) unsafe fn invoke_symbol_accessor_getter(get_bits: u64, receiver: f64) -> f64 {
    if get_bits == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let closure = (get_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::closure::js_closure_call0(closure, crate::closure::JsThis::from_f64(receiver))
}

pub(super) fn has_own_symbol_accessor(obj_key: usize, sym_key: usize) -> bool {
    unsafe { crate::object::shaped_symbols::accessor(obj_key, sym_key).is_some() }
}

/// Symbol keys (raw `SymbolHeader` pointers) of every accessor-only property
/// installed on `obj_key`. Used by `getOwnPropertySymbols`, which must report
/// symbol-keyed accessors even though they live in the owner's shape.
pub(super) fn owner_symbol_accessor_keys(obj_key: usize) -> Vec<usize> {
    unsafe { crate::object::shaped_symbols::entries(obj_key, false) }
        .into_iter()
        .filter_map(|(sym, _)| unsafe {
            crate::object::shaped_symbols::accessor(obj_key, sym).map(|_| sym)
        })
        .collect()
}
