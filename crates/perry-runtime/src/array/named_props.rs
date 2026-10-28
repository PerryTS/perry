//! Array named properties are an ordinary object reached through ArrayHeader.named.
//! Elements never move when a property is added; the collector traces the header edge.
use super::header::{clean_arr_ptr, clean_arr_ptr_mut, string_header_as_str, string_header_bytes};
use super::ArrayHeader;
use crate::gc::RuntimeHandleScope;
use crate::value::TAG_HOLE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum NamedPropertyKeys {
    /// A regex `exec`/`match` result.
    ExecResult = 1,
    /// A regex `exec`/`match` result with the `d` flag.
    ExecResultIndices = 2,
    /// The `indices` array of a `d`-flag result.
    IndicesGroups = 3,
}

impl NamedPropertyKeys {
    #[inline]
    pub(crate) const fn keys(self) -> &'static [&'static str] {
        match self {
            NamedPropertyKeys::ExecResult => &["index", "input", "groups"],
            NamedPropertyKeys::ExecResultIndices => &["index", "input", "groups", "indices"],
            NamedPropertyKeys::IndicesGroups => &["groups"],
        }
    }
}
#[inline]
pub(crate) unsafe fn array_property_bag(
    arr: *const ArrayHeader,
) -> *mut crate::object::ObjectHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        std::ptr::null_mut()
    } else {
        (*arr).named
    }
}

#[inline(always)]
pub(crate) unsafe fn array_property_bag_resolved(
    arr: *const ArrayHeader,
) -> *mut crate::object::ObjectHeader {
    (*arr).named
}

pub(super) unsafe fn publish_named(arr: *mut ArrayHeader, named: *mut crate::object::ObjectHeader) {
    // GC_STORE_AUDIT(BARRIERED): traced raw header child; barrier receives the equivalent boxed pointer.
    (*arr).named = named;
    crate::gc::runtime_write_barrier_slot(
        arr as usize,
        std::ptr::addr_of_mut!((*arr).named) as usize,
        crate::value::js_nanbox_pointer(named as i64).to_bits(),
    );
}

pub(crate) unsafe fn array_property_bag_ensure(
    arr: *mut ArrayHeader,
) -> *mut crate::object::ObjectHeader {
    let arr = clean_arr_ptr_mut(arr);
    if !(*arr).named.is_null() {
        return (*arr).named;
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(arr);
    let (named, owner) = owner.across_mut::<ArrayHeader, _>(|| {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(
            crate::object::runtime_layouts::ARRAY_NAMED
        ))
    });
    publish_named(owner, named);
    named
}

pub(crate) unsafe fn array_prototype_property_bag_ensure(
    arr: *mut ArrayHeader,
    room: u32,
) -> *mut crate::object::ObjectHeader {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(arr);
    let (named, owner) = owner.across_mut::<ArrayHeader, _>(|| {
        crate::object::layout_desc::alloc_layout_prototype(
            std::ptr::addr_of!(crate::object::runtime_layouts::ARRAY_PROTOTYPE_NAMED),
            room,
        )
    });
    publish_named(owner, named);
    named
}

pub(crate) unsafe fn array_named_property_set(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    // A named write to `%Array.prototype%` sets its shadow bit, so every
    // `Pristine(Array)` site takes the generic path.
    crate::object::builtin_proto::note_array_prototype_write(arr as usize, "named property");
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(arr);
    let key = scope.root_string_ptr(key);
    let value = scope.root_nanbox_f64(value);
    let named = owner.with_mut_ptr(|array| array_property_bag_ensure(array));
    let (_, owner) = owner.across_mut::<ArrayHeader, _>(|| {
        key.with_const_ptr(|key| {
            crate::object::object_ops::define_property_force_store_value(
                named,
                key,
                value.get_nanbox_f64(),
            );
        });
    });
    owner
}

unsafe fn present_slot(
    arr: *const ArrayHeader,
    wanted: &[u8],
) -> Option<(*mut crate::object::ObjectHeader, u32)> {
    let named = array_property_bag(arr);
    if named.is_null() {
        return None;
    }
    let keys = crate::object::object_keys(named);
    let slot = crate::object::keys_find_property_slot_by_bytes(keys.arr(), keys.count(), wanted)?;
    (crate::object::field_get_set::js_object_get_key_field(named, slot).bits() != TAG_HOLE)
        .then_some((named, slot))
}

unsafe fn lookup(arr: *const ArrayHeader, wanted: &[u8]) -> Option<f64> {
    let (named, slot) = present_slot(arr, wanted)?;
    let keys = crate::object::object_keys(named);
    let value = crate::object::field_get_set::js_object_get_key_field(named, slot).bits();
    if crate::object::key_attrs::key_is_accessor_at(keys.arr(), slot) {
        let pair = crate::object::accessor_pair::pair_of_value(value).unwrap_or_default();
        return Some(if pair.get == 0 {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            f64::from_bits(
                crate::object::invoke_accessor_getter(
                    pair.get,
                    crate::value::js_nanbox_pointer(clean_arr_ptr(arr) as i64),
                )
                .bits(),
            )
        });
    }
    Some(f64::from_bits(value))
}

pub(crate) unsafe fn array_named_property_get_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> Option<f64> {
    lookup(arr, name.as_bytes())
}
pub(crate) unsafe fn array_named_property_get(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    lookup(arr, string_header_bytes(key)?)
}
/// Quiet presence read for callers that already have the key bytes.
pub(crate) unsafe fn array_named_property_has_by_name(arr: *const ArrayHeader, name: &str) -> bool {
    present_slot(arr, name.as_bytes()).is_some()
}

pub(crate) unsafe fn array_named_property_has(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    string_header_bytes(key).is_some_and(|name| present_slot(arr, name).is_some())
}
pub(crate) unsafe fn array_named_property_names(
    arr: *const ArrayHeader,
    enumerable: bool,
) -> Vec<String> {
    let named = array_property_bag(arr);
    if named.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(named);
    let mut result = Vec::new();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..keys.count() {
        let entry = crate::object::key_attrs::keys_entry(keys.arr(), i);
        if crate::object::key_attrs::object_slot_data_f64(named, i).to_bits() == TAG_HOLE {
            continue;
        }
        if crate::object::key_attrs::entry_is_private(entry)
            || (enumerable && entry & crate::object::key_attrs::ENTRY_NON_ENUMERABLE != 0)
        {
            continue;
        }
        if let Some(bytes) = crate::string::js_string_key_bytes(keys.get(i), &mut sso) {
            if let Ok(name) = std::str::from_utf8(bytes) {
                result.push(name.to_owned());
            }
        }
    }
    result
}
pub(crate) unsafe fn array_named_property_enumeration_names(
    arr: *const ArrayHeader,
    enumerable: bool,
    covered: u32,
) -> Vec<String> {
    array_named_property_names(arr, enumerable)
        .into_iter()
        .filter(|name| crate::object::canonical_array_index(name).is_none_or(|i| i >= covered))
        .collect()
}
pub(crate) unsafe fn array_named_property_delete(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(name) = string_header_as_str(key) else {
        return false;
    };
    array_named_property_delete_by_name(arr, name)
}
pub(crate) unsafe fn array_named_property_delete_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> bool {
    let scope = RuntimeHandleScope::new();
    let named = array_property_bag(arr);
    if named.is_null() || present_slot(arr, name.as_bytes()).is_none() {
        return false;
    }
    let named = scope.root_raw_mut_ptr(named);
    let (key, named) = named.across_mut::<crate::object::ObjectHeader, _>(|| {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    });
    crate::object::js_object_delete_field(named, key) != 0
}
pub(crate) unsafe fn array_has_sparse_index_properties_resolved(arr: *const ArrayHeader) -> bool {
    array_named_property_names(arr, false).iter().any(|name| {
        crate::object::canonical_array_index(name).is_some_and(|i| i >= (*arr).capacity)
    })
}
pub(crate) unsafe fn array_has_named_properties_resolved(arr: *const ArrayHeader) -> bool {
    !(*arr).named.is_null()
        && (0..crate::object::object_keys((*arr).named).count())
            .any(|i| crate::object::js_object_get_field((*arr).named, i).bits() != TAG_HOLE)
}

/// Runtime creation points with known names allocate their ordinary property layout at birth.
#[cfg(feature = "regex-engine")]
pub(crate) unsafe fn array_named_props_install(arr: *mut ArrayHeader, values: &[f64]) {
    let named = (*arr).named;
    for (i, value) in values.iter().enumerate() {
        crate::object::js_object_set_field(
            named,
            i as u32 + 1,
            crate::value::JSValue::from_bits(value.to_bits()),
        );
    }
}

/// Internal key-list attributes still occupy one traced front slot; this is never user named storage.
pub(crate) unsafe fn key_attrs_reserve(arr: *const ArrayHeader) -> usize {
    usize::from(
        super::header::array_object_flags_resolved(arr) & crate::gc::GC_ARRAY_KEY_ATTRS != 0,
    )
}
pub(crate) unsafe fn key_attrs_slot(arr: *const ArrayHeader) -> *mut u64 {
    arr.add(1) as *mut u64
}
pub(crate) unsafe fn store_key_attrs_word(arr: *mut ArrayHeader, bits: u64) {
    let slot = key_attrs_slot(arr);
    // GC_STORE_AUDIT(BARRIERED): internal key-list attribute edge.
    slot.write(bits);
    crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, bits);
}
pub(crate) unsafe fn carry_key_attrs_reserve(
    old: *const ArrayHeader,
    new: *mut ArrayHeader,
    reserve: usize,
) {
    if reserve != 0 {
        store_key_attrs_word(new, *key_attrs_slot(old));
    }
}

pub(crate) unsafe fn ensure_key_attrs_slot(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    if key_attrs_reserve(arr) != 0 {
        return arr;
    }
    let mut arr = arr;
    if super::array_front_offset(arr) == 0 {
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        if dense >= capacity {
            // Full: no slack anywhere. Grow exactly like an append would.
            let grown = super::js_array_grow(arr, (capacity as u32).saturating_add(1));
            if grown.is_null() || ((*grown).capacity as usize) <= dense {
                return std::ptr::null_mut();
            }
            arr = grown;
        }
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        let elems = super::array_elements_ptr(arr);
        // GC_STORE_AUDIT(BARRIERED): the dense prefix moves up by one slot
        // inside its own allocation; `finish_array_dense_move_layout` below
        // translates the old array's dirty-page coverage or replays the
        // barriers for the moved survivors.
        std::ptr::copy(elems, elems.add(1), dense);
        (*arr).capacity -= 1;
        debug_assert_eq!(super::array_front_offset(arr), 1);
        super::finish_array_dense_move_layout(arr, elems, elems.add(1), dense, elems, 0);
    }
    // GC_STORE_AUDIT(INIT): the reserved word is dead front slack until the
    // flag below publishes it as a child slot; a non-pointer sentinel goes in
    // first so the collector never reads a stale element there.
    std::ptr::write(key_attrs_slot(arr), TAG_HOLE);
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
    (*header)._reserved |= crate::gc::GC_ARRAY_KEY_ATTRS;
    arr
}

#[cfg(test)]
pub(crate) unsafe fn test_named_props_state(arr: *const ArrayHeader) -> (bool, usize, usize) {
    let named = array_property_bag(arr);
    if named.is_null() {
        (false, 0, 0)
    } else {
        (
            true,
            named as usize,
            (0..crate::object::object_keys(named).count())
                .filter(|&i| crate::object::js_object_get_field(named, i).bits() != TAG_HOLE)
                .count(),
        )
    }
}

/// Give a compiler-proven array creation family its ordinary named holder.
/// The array is already rooted before the holder's collecting allocation.
#[no_mangle]
pub unsafe extern "C" fn perry_array_attach_layout(
    value: f64,
    desc: *const crate::object::layout_desc::PerryLayoutDesc,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(value);
    let named = crate::object::layout_desc::perry_alloc_layout(desc);
    let arr = crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as *mut ArrayHeader;
    publish_named(arr, named);
    owner.get_nanbox_f64()
}

/// Immutable layout fact: only numeric keys and length can change elements.
pub(crate) unsafe fn keys_hold_element_descriptors(keys: *const ArrayHeader, count: u32) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for slot in 0..count {
        if crate::object::key_attrs::keys_entry(keys, slot) == 0 {
            continue;
        }
        let value =
            crate::JSValue::from_bits(*crate::array::array_elements_ptr(keys).add(slot as usize));
        let Some(bytes) = crate::string::js_string_key_bytes(value, &mut sso) else {
            continue;
        };
        if bytes == b"length" || crate::object::canonical_array_index_bytes(bytes).is_some() {
            return true;
        }
    }
    false
}

/// Codegen's element guard reads the layout fact; named keys do not veto it.
#[no_mangle]
pub unsafe extern "C" fn perry_array_element_kind(arr: *const ArrayHeader) -> i32 {
    let Some(header) = crate::value::addr_class::try_read_gc_header(arr as usize) else {
        return 1;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return 1;
    }
    i32::from(super::array_has_element_descriptors_resolved(arr))
}
