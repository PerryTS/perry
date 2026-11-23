//! ECMAScript Arguments objects, born on the two arguments layouts
//! (`runtime_layouts::ARGUMENTS` kind 13, `ARGUMENTS_STRICT` kind 14).
//!
//! Slot 0 is `length`, 1 `callee` (the function, or the %ThrowTypeError%
//! accessor pair for kind 14), 2 @@iterator (%Array.prototype.values%). The
//! elements follow inline from slot 3 and are not keys: their count is the
//! GcHeader size word (`subclass_elements::ElementsView`). Mapped vs unmapped
//! is no state: an element word that points at a `GC_TYPE_BOX` is a mapped
//! parameter cell, read and written through. Named keys added later never take
//! an inline slot (`layout_desc::route_family_add`, `object_inline_capacity`).

use super::descriptors::{build_accessor_descriptor, build_data_descriptor};
use super::*;
use crate::codegen_abi::LayoutWord;

// The owner of the two arguments layouts: a per-call birth site, no realm
// cell. A program with a function that synthesizes `arguments` reaches it
// (perry-hir realm_reach::scan_lowered), and with it both descriptors and the
// value slot 2 holds.
crate::realm_decl! { "%ArgumentsBirth%", kind = birth_site, births = [Array.prototype.values]; }

use crate::array::subclass_elements::ARGUMENTS_ELEMENTS_OFFSET;

/// What an arguments object's kind says about it: 13 (sloppy, `callee` the
/// function) or 14 (strict, `callee` the %ThrowTypeError% pair). `obj` is
/// valid until the next collection point; callers re-derive after anything
/// that can allocate.
#[derive(Clone, Copy)]
struct ArgumentsState {
    obj: *mut ObjectHeader,
    strict: bool,
}

impl ArgumentsState {
    fn restricted_callee(self) -> bool {
        self.strict
    }

    /// The number of inline elements (from the GcHeader size word).
    unsafe fn element_count(self) -> u32 {
        let gc = crate::gc::header_from_trusted_user_ptr(self.obj.cast());
        arguments_element_count((*gc).size)
    }

    /// Element `index`'s word, when `index` is inside the inline elements.
    unsafe fn element_slot(self, index: u32) -> Option<*mut u64> {
        (index < self.element_count()).then(|| {
            ((self.obj as usize + ARGUMENTS_ELEMENTS_OFFSET) as *mut u64).add(index as usize)
        })
    }

    /// The cell parameter `index` still aliases, if any: the box word in
    /// whichever slot holds property `index` (its element word, or the slot
    /// of own key `index` once a define moved it there). Never allocates.
    unsafe fn mapped_box(self, index: u32) -> Option<*mut crate::r#box::Box> {
        match self.element_slot(index) {
            Some(slot) if *slot != crate::value::TAG_HOLE => box_of_word(*slot),
            _ => box_of_word(key_word(self.obj, own_index_key(self.obj, index)?)),
        }
    }

    /// Break index `index`'s alias: the slot that holds the cell keeps the
    /// cell's current value. Never allocates.
    unsafe fn unmap(self, index: u32) {
        if let Some(slot) = self.element_slot(index) {
            if *slot != crate::value::TAG_HOLE {
                if let Some(cell) = box_of_word(*slot) {
                    store_element_word(self.obj, slot, crate::r#box::js_box_get(cell).to_bits());
                }
                return;
            }
        }
        let Some(position) = own_index_key(self.obj, index) else {
            return;
        };
        if let Some(cell) = box_of_word(key_word(self.obj, position)) {
            set_key_word(self.obj, position, crate::r#box::js_box_get(cell).to_bits());
        }
    }
}

/// The key-list position of `obj`'s own named key `index` (a canonical index
/// string), if it has one. Never allocates on the GC heap.
unsafe fn own_index_key(obj: *const ObjectHeader, index: u32) -> Option<u32> {
    let keys_view = crate::object::object_keys(obj);
    let count = keys_view.count();
    if count == 0 {
        return None;
    }
    let keys = keys_view.arr();
    let name = index.to_string();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    (0..count).find(|&i| {
        let key_val = crate::array::js_array_get(keys, i);
        crate::string::js_string_key_bytes(key_val, &mut sso).is_some_and(|b| b == name.as_bytes())
    })
}

/// The storage word of the key at key-list `position` of `obj`. Never allocates.
unsafe fn key_word(obj: *const ObjectHeader, position: u32) -> u64 {
    let slot = crate::object::object_keys(obj).storage_slot(position) as usize;
    if slot < crate::object::object_inline_capacity(obj) as usize {
        js_object_get_field(obj, slot as u32).bits()
    } else {
        overflow_get(obj as usize, slot).unwrap_or(crate::value::TAG_UNDEFINED)
    }
}

/// Store the storage word of the key at key-list `position` of `obj`. Never
/// allocates.
unsafe fn set_key_word(obj: *mut ObjectHeader, position: u32, bits: u64) {
    let slot = crate::object::object_keys(obj).storage_slot(position) as usize;
    if slot < crate::object::object_inline_capacity(obj) as usize {
        js_object_set_field(obj, slot as u32, JSValue::from_bits(bits));
    } else {
        overflow_set(obj as usize, slot, bits);
    }
}

/// `defineProperty`'s own-slot store of `value` into a slot of `obj` that
/// holds `current`: when `obj` is an arguments object and `current` is a
/// parameter cell, the value goes into the cell and the slot keeps it
/// (Set(map, P, V)); `true` then. Never allocates.
pub(crate) unsafe fn arguments_store_through_cell(
    obj: *const ObjectHeader,
    current: u64,
    value: f64,
) -> bool {
    if arguments_state(obj).is_none() {
        return false;
    }
    match box_of_word(current) {
        Some(cell) => {
            crate::r#box::js_box_set(cell, value);
            true
        }
        None => false,
    }
}

/// `Object.freeze` of an arguments object: every element gets
/// `writable: false`, which unmaps it (it keeps the cell's value). Never
/// allocates.
pub(crate) unsafe fn arguments_object_unmap_all(obj: *mut ObjectHeader) {
    let Some(state) = arguments_state(obj) else {
        return;
    };
    for index in 0..state.element_count() {
        state.unmap(index);
    }
}

/// Inline element count of an arguments object of GcHeader size `gc_size`.
#[inline]
fn arguments_element_count(gc_size: u32) -> u32 {
    ((gc_size as usize - crate::gc::GC_HEADER_SIZE).saturating_sub(ARGUMENTS_ELEMENTS_OFFSET) / 8)
        as u32
}

/// The parameter cell an element word names, if it is one (a box pointer is
/// never a JS value).
#[inline]
unsafe fn box_of_word(word: u64) -> Option<*mut crate::r#box::Box> {
    if (word >> 48) != 0x7FFD {
        return None;
    }
    let cell = (word & crate::value::POINTER_MASK) as usize;
    crate::value::addr_class::try_read_gc_header(cell)
        .filter(|h| h.obj_type == crate::gc::GC_TYPE_BOX)
        .map(|_| cell as *mut crate::r#box::Box)
}

/// Store an element word of a live arguments object. Never allocates.
#[inline]
unsafe fn store_element_word(obj: *mut ObjectHeader, slot: *mut u64, bits: u64) {
    // GC_STORE_AUDIT(BARRIERED): element word of a live object; the slot barrier follows.
    slot.write(bits);
    crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, bits);
}

/// Element `index`'s value when it is a present inline element (a mapped
/// element reads its cell). Never allocates.
unsafe fn present_element(state: ArgumentsState, index: u32) -> Option<f64> {
    let word = *state.element_slot(index)?;
    if word == crate::value::TAG_HOLE {
        return None;
    }
    Some(match box_of_word(word) {
        Some(cell) => crate::r#box::js_box_get(cell),
        None => f64::from_bits(word),
    })
}

/// `[[Set]]` of a present inline element (through its cell when mapped).
/// `false` when `index` is not one, or the object is frozen: the caller's
/// general path answers. Never allocates.
unsafe fn write_present_element(obj: *mut ObjectHeader, index: u32, value: f64) -> bool {
    let Some(state) = arguments_state(obj) else {
        return false;
    };
    let Some(slot) = state.element_slot(index) else {
        return false;
    };
    let word = *slot;
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if word == crate::value::TAG_HOLE || (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return false;
    }
    match box_of_word(word) {
        Some(cell) => crate::r#box::js_box_set(cell, value),
        None => store_element_word(obj, slot, value.to_bits()),
    }
    true
}

/// `obj`'s arguments state, from its kind. Never allocates.
/// `obj` may be any pointer a probe was handed: a non-object, a handle, or an
/// ordinary object without the arguments identity all answer `None`.
///
/// # Safety
/// `obj` is a live heap address or arbitrary non-pointer bits (the
/// `try_read_gc_header` contract).
#[inline]
unsafe fn arguments_state(obj: *const ObjectHeader) -> Option<ArgumentsState> {
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    if header._reserved & crate::gc::OBJ_FLAG_ARGUMENTS == 0 {
        return None;
    }
    arguments_state_of_flagged(obj)
}

/// [`arguments_state`] for an object whose caller already read a live,
/// unforwarded object header carrying `OBJ_FLAG_ARGUMENTS`.
#[inline]
unsafe fn arguments_state_of_flagged(obj: *const ObjectHeader) -> Option<ArgumentsState> {
    let record = crate::object::shapes::shape_record_for_word(LayoutWord((*obj).shape_word()))?;
    let strict = if record.kind_is(crate::object::shapes::ShapeObjectKind::Arguments) {
        false
    } else if record.kind_is(crate::object::shapes::ShapeObjectKind::ArgumentsStrict) {
        true
    } else {
        return None;
    };
    Some(ArgumentsState {
        obj: obj as *mut ObjectHeader,
        strict,
    })
}

/// The descriptor an arguments object of this kind is born on.
fn arguments_desc(strict: bool) -> *const crate::object::layout_desc::PerryLayoutDesc {
    if strict {
        crate::object::runtime_layouts::ARGUMENTS_STRICT::desc()
    } else {
        crate::object::runtime_layouts::ARGUMENTS::desc()
    }
}

/// %Array.prototype.values%, the value of every arguments object's slot 2.
fn array_values_intrinsic() -> f64 {
    let mut addr = crate::array::realm_array_values_addr();
    if addr == 0 {
        let _ = crate::object::js_get_global_this();
        addr = crate::array::realm_array_values_addr();
    }
    if addr == 0 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(addr as i64)
    }
}

#[cfg(test)]
pub(crate) fn test_clear_arguments_object_roots() {
    crate::array::note_realm_restricted_callee_pair(0);
}

/// The realm's restricted-`callee` accessor pair (a realm intrinsic, built
/// with the first strict arguments object). A pair is immutable once built
/// (`accessor_pair::pair_new` is its only writer) and `callee` is
/// non-configurable, so one pair serves every strict arguments object.
fn restricted_callee_pair() -> *mut ArrayHeader {
    let cached = crate::array::realm_restricted_callee_pair_addr();
    if cached != 0 {
        return cached as *mut ArrayHeader;
    }
    let thrower = thrower_closure_value();
    let pair = unsafe {
        super::accessor_pair::pair_new(super::accessor_pair::pair_from(&AccessorDescriptor {
            get: thrower.to_bits(),
            set: thrower.to_bits(),
        }))
    };
    // GC_STORE_AUDIT(ROOT): the realm-intrinsic scanner rewrites this cell.
    crate::array::note_realm_restricted_callee_pair(pair as usize);
    pair
}

/// Test-only: the box parameter `index` of `obj` aliases.
#[cfg(test)]
pub(crate) fn test_arguments_mapped_box(obj: *const ObjectHeader, index: u32) -> Option<usize> {
    unsafe { arguments_state(obj).and_then(|state| state.mapped_box(index)) }
        .map(|cell| cell as usize)
}

/// Test-only: the mapping array a sloppy mapped object's tail points at.
#[cfg(test)]
pub(crate) fn test_arguments_mapping_array(obj: *const ObjectHeader) -> Option<usize> {
    let state = unsafe { arguments_state(obj) }?;
    (0..unsafe { state.element_count() })
        .any(|i| unsafe { state.mapped_box(i) }.is_some())
        .then_some(obj as usize)
}

fn key_name(key: *const crate::StringHeader) -> Option<String> {
    if key.is_null() || (key as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

fn intern_key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

extern "C" fn arguments_throw_type_error(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    super::throw_object_type_error(
        b"'caller', 'callee', and 'arguments' properties may not be accessed",
    );
}

pub(super) const THROWER_FROZEN_FLAGS: u16 =
    crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;

pub(super) fn thrower_closure_value() -> f64 {
    let closure = crate::closure::js_closure_alloc_singleton(
        crate::fn_info!(#[realm("Function.prototype.arguments.<get>", integrity = frozen, effects = { returns(never) }, spec = {name = ""}, emit = metadata(realm_thrower_public))] export crate::object::arguments::arguments_throw_type_error, 0; with_name("get arguments"), with_declared(0)),
    );
    // #10509: configure the per-thread singleton once, not per strict call.
    // The frozen bits are the last step below and live in the closure's own
    // header; every side table the earlier steps write (closure props,
    // descriptor attrs, builtin-closure metadata) is rekeyed when the closure
    // moves, so a set bit means the whole configuration is still in place.
    if !closure.is_null()
        && unsafe { crate::value::addr_class::try_read_tracked_gc_header(closure as usize) }
            .is_some_and(|gc| unsafe {
                (*gc.as_ptr())._reserved & THROWER_FROZEN_FLAGS == THROWER_FROZEN_FLAGS
            })
    {
        return crate::value::js_nanbox_pointer(closure as i64);
    }
    super::native_module::set_bound_native_closure_name(closure, realm_thrower_public!(name));
    super::native_module::set_builtin_closure_length(closure as usize, 0);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let frozen = PropertyAttrs::new(false, false, false);
    set_property_attrs(closure as usize, "name".to_string(), frozen);
    set_property_attrs(closure as usize, "length".to_string(), frozen);
    if !closure.is_null() {
        unsafe {
            if let Some(gc) = crate::value::addr_class::try_read_tracked_gc_header(closure as usize)
            {
                (*gc.as_ptr())._reserved |= THROWER_FROZEN_FLAGS;
            }
        }
    }
    crate::value::js_nanbox_pointer(closure as i64)
}

/// The attribute entry of a mapped/sloppy arguments object's `length` and
/// `callee`: `{ writable: true, enumerable: false, configurable: true }`.

#[no_mangle]
pub extern "C" fn js_arguments_object_alloc(
    raw_args: f64,
    callee: f64,
    restricted_callee: i32,
) -> *mut ObjectHeader {
    arguments_object_alloc(raw_args, callee, restricted_callee != 0, 0)
}

/// A sloppy MAPPED arguments object (#11506): [`js_arguments_object_alloc`]
/// with an unrestricted `callee`, plus room for `mapped_count` parameter
/// aliases, which the prologue then fills with
/// [`js_arguments_object_map_index`]. CreateMappedArgumentsObject maps only
/// the indices the call actually passed (`index < len`), so the room is
/// `min(mapped_count, len)`.
///
/// The room is allocated HERE and never in `map_index`: the prologue holds
/// the object in a bare register across its `map_index` calls, so those
/// calls must not be able to collect.
#[no_mangle]
pub extern "C" fn js_arguments_object_alloc_mapped(
    raw_args: f64,
    callee: f64,
    mapped_count: u32,
) -> *mut ObjectHeader {
    arguments_object_alloc(raw_args, callee, false, mapped_count)
}

fn arguments_object_alloc(
    raw_args: f64,
    callee: f64,
    restricted_callee: bool,
    mapped_count: u32,
) -> *mut ObjectHeader {
    let addr = crate::value::js_nanbox_get_pointer(raw_args) as *const ObjectHeader;
    if is_arguments_object(addr) {
        // The immutable body info selected exactly this birth ABI. Its object
        // is already rooted in the synthetic parameter slot at body entry.
        return addr as *mut ObjectHeader;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_args = scope.root_nanbox_f64(raw_args);
    let callee = scope.root_nanbox_f64(callee);
    let arr_ptr = crate::array::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
        raw_args.get_nanbox_f64(),
    ) as *const ArrayHeader);
    let len = if arr_ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr_ptr)
    };

    // A plain dense input (no hole, no element descriptor: the call's own
    // bundle) cannot run code on a read, so its elements are copied once,
    // straight into the newborn's slots, after the last allocation.
    if !arr_ptr.is_null() && unsafe { plain_dense_elements(arr_ptr, len) } {
        return arguments_object_birth(
            len,
            callee.get_nanbox_f64(),
            restricted_callee,
            mapped_count,
            ArgumentsSource::Array(&raw_args),
        );
    }
    // Snapshot before birth: even a descriptor-bearing input Array can run
    // getters. Every earlier value remains rooted across the next read.
    let values: Vec<_> = (0..len)
        .map(|i| {
            let arr = crate::array::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
                raw_args.get_nanbox_f64(),
            ) as *const ArrayHeader);
            scope.root_nanbox_f64(f64::from_bits(crate::array::js_array_get(arr, i).bits()))
        })
        .collect();
    arguments_object_alloc_rooted(
        &values,
        callee.get_nanbox_f64(),
        restricted_callee,
        mapped_count,
    )
}

/// Birth straight from the caller's rooted values; no rest Array.
pub(crate) fn arguments_object_alloc_rooted(
    values: &[crate::gc::RuntimeHandle<'_>],
    callee: f64,
    restricted_callee: bool,
    mapped_count: u32,
) -> *mut ObjectHeader {
    let len = u32::try_from(values.len()).expect("arguments length fits u32");
    arguments_object_birth(
        len,
        callee,
        restricted_callee,
        mapped_count,
        ArgumentsSource::Handles(values),
    )
}

/// Where a newborn arguments object's element values are read from, once,
/// after its last allocation.
#[derive(Clone, Copy)]
enum ArgumentsSource<'a, 'h> {
    /// The caller's rooted values.
    Handles(&'a [crate::gc::RuntimeHandle<'h>]),
    /// A rooted plain dense Array (`plain_dense_elements`): its first `len`
    /// element words are the values.
    Array(&'a crate::gc::RuntimeHandle<'h>),
}

/// `arr` (a resolved Array) holds `len` plain element words: no element
/// descriptor and no hole, so reading them raw is exactly `[[Get]]` and no
/// read can run code or allocate.
unsafe fn plain_dense_elements(arr: *const ArrayHeader, len: u32) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(arr as usize) else {
        return false;
    };
    if crate::gc_type_is_not_array!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*arr).length != len
        || (*arr).length > (*arr).capacity
        || crate::array::array_has_element_descriptors_resolved(arr)
    {
        return false;
    }
    let elements = crate::array::array_elements_ptr(arr) as *const u64;
    (0..len as usize).all(|i| *elements.add(i) != crate::value::TAG_HOLE)
}

fn arguments_object_birth(
    len: u32,
    callee: f64,
    restricted_callee: bool,
    _mapped_count: u32,
    source: ArgumentsSource<'_, '_>,
) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(callee);
    // The restricted `callee` pair (built once per realm) and slot 2's value
    // are ready before the birth: nothing allocates between the allocation
    // and the initialization of every slot.
    let pair = restricted_callee.then(|| scope.root_raw_mut_ptr(restricted_callee_pair()));
    let values = scope.root_nanbox_f64(array_values_intrinsic());
    let slots = 3 + len as usize;
    // The one allocation: the three named slots and the `len` elements, on the
    // kind's layout (header, holes, descriptor flag from the descriptor).
    let obj = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::alloc_arguments_layout(arguments_desc(restricted_callee), slots)
    });
    obj.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
        let header = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*header)._reserved |= crate::gc::OBJ_FLAG_ARGUMENTS;
        let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj));
        // GC_STORE_AUDIT(INIT): newborn slots; rooted values; stores cannot collect.
        fields.write((len as f64).to_bits());
        fields.add(1).write(pair.as_ref().map_or_else(
            || callee.get_nanbox_f64().to_bits(),
            |pair| {
                crate::value::js_nanbox_pointer(pair.get_raw_mut_ptr::<ArrayHeader>() as i64)
                    .to_bits()
            },
        ));
        fields.add(2).write(values.get_nanbox_f64().to_bits());
        let elements = fields.add(3);
        match source {
            ArgumentsSource::Handles(values) => {
                for (i, value) in values.iter().enumerate() {
                    // GC_STORE_AUDIT(INIT): rooted values; stores cannot collect.
                    elements.add(i).write(value.get_nanbox_f64().to_bits());
                }
            }
            ArgumentsSource::Array(array) => {
                let arr = crate::array::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
                    array.get_nanbox_f64(),
                ) as *const ArrayHeader);
                // GC_STORE_AUDIT(INIT): the rooted Array's plain element words
                // (checked before birth; nothing ran since); stores cannot collect.
                std::ptr::copy_nonoverlapping(
                    crate::array::array_elements_ptr(arr) as *const u64,
                    elements,
                    len as usize,
                );
            }
        }
        crate::object::rebuild_object_field_layout(obj, slots);
    });
    obj.get_raw_mut_ptr::<ObjectHeader>()
}

/// #10509: `arguments[key]` in a function whose Arguments object codegen
/// elided (`perry-codegen` `codegen/arguments.rs`, `ArgumentsElision::Reads`).
///
/// `raw_args` is the marked Array the caller bundled — the exact input
/// [`js_arguments_object_alloc`] would have copied. An own element (a
/// canonical array index below the bundle's length) reads through the same
/// `clean_arr_ptr` / `js_array_length` / `js_array_get` sequence that alloc
/// uses, so it is the value the object's field would have held. Every other
/// key — `"callee"`, a symbol, a string, a fractional, negative or
/// out-of-range number, an INT32-tagged value, or a hole element — answers
/// `TAG_HOLE`, which no JS read produces, and the caller takes
/// [`js_arguments_bundle_get_slow`]. Cannot allocate, throw, or run user code.
#[no_mangle]
pub extern "C" fn js_arguments_bundle_index_get(raw_args: f64, key: f64) -> f64 {
    let not_own_element = f64::from_bits(crate::value::TAG_HOLE);
    // Only an exact non-negative integer below 2^32 - 1 is an array index. A
    // NaN-boxed key is a NaN, so it fails the range test; -0 names index 0.
    if !(key >= 0.0 && key < u32::MAX as f64) {
        return not_own_element;
    }
    let index = key as u32;
    if index as f64 != key {
        return not_own_element;
    }
    let raw = crate::value::js_nanbox_get_pointer(raw_args) as *const ArrayHeader;
    // Admit the snapshot once. Re-entering clean_arr_ptr and then
    // array_receiver_gc_tag repeated ownership classification for every
    // read, even though both establish the same live Array header fact.
    // Forwarded or descriptor-bearing inputs keep the complete getter below.
    unsafe {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(raw as usize) {
            if crate::gc_type_is_array!(header.obj_type)
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && !crate::array::array_has_element_descriptors_resolved(raw)
                && (*raw).length <= (*raw).capacity
                && (*raw).capacity as usize <= crate::array::array_physical_capacity(raw)
            {
                if index >= (*raw).length {
                    return not_own_element;
                }
                return *(crate::array::array_elements_ptr(raw) as *const f64).add(index as usize);
            }
        }
    }
    let arr = crate::array::clean_arr_ptr(raw);
    if arr.is_null() {
        return not_own_element;
    }
    if index >= crate::array::js_array_length(arr) {
        return not_own_element;
    }
    f64::from_bits(crate::array::js_array_get(arr, index).bits())
}

/// #10509: the cold half of [`js_arguments_bundle_index_get`]. Builds the
/// Arguments object the prologue would have built (`callee_wrapper`, when
/// non-null, is the info of the function whose singleton closure is `callee`) and
/// performs an ordinary `obj[key]` on it, so a non-element key sees the real
/// object's `callee`, `length`, `Symbol.iterator` and `Object.prototype`
/// surface. The object is not kept: codegen only takes this path for a
/// function that never observes the object's identity.
#[no_mangle]
pub extern "C" fn js_arguments_bundle_get_slow(
    raw_args: f64,
    key: f64,
    callee: f64,
    callee_wrapper: *const crate::closure::JsFunctionInfo,
    restricted_callee: i32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_args = scope.root_nanbox_f64(raw_args);
    let key = scope.root_nanbox_f64(key);
    let callee = if callee_wrapper.is_null() {
        callee
    } else {
        let closure = crate::closure::js_closure_alloc_singleton(callee_wrapper);
        crate::value::js_nanbox_pointer(closure as i64)
    };
    let obj = js_arguments_object_alloc(raw_args.get_nanbox_f64(), callee, restricted_callee);
    crate::value::js_dyn_index_get(
        crate::value::js_nanbox_pointer(obj as i64),
        key.get_nanbox_f64(),
    )
}

#[no_mangle]
pub extern "C" fn js_arguments_object_map_index(
    obj: *mut ObjectHeader,
    index: u32,
    box_ptr: *mut crate::r#box::Box,
) {
    if obj.is_null() || box_ptr.is_null() {
        return;
    }
    // Never allocates: the prologue holds `obj` in a bare register across
    // these calls. Only an index the call passed (inside the inline elements)
    // is mapped (CreateMappedArgumentsObject); the element word becomes the
    // parameter's cell, a traced pointer edge stored with the slot barrier.
    unsafe {
        let Some(state) = arguments_state(obj) else {
            return;
        };
        if state.strict {
            return;
        }
        let Some(slot) = state.element_slot(index) else {
            return;
        };
        if *slot == crate::value::TAG_HOLE {
            return;
        }
        let bits = crate::value::POINTER_TAG | (box_ptr as u64 & crate::value::POINTER_MASK);
        store_element_word(obj, slot, bits);
    }
}

pub(crate) fn is_arguments_object(obj: *const ObjectHeader) -> bool {
    #[cfg(test)]
    TEST_ARGUMENTS_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    // #7854: nothing has ever been created ⟹ nothing can be found. Checked
    // first because it is the only arm that never touches the receiver. See
    // `ARGUMENTS_OBJECTS_EVER_USED`.
    if obj.is_null() {
        return false;
    }
    let found = unsafe { arguments_state(obj) }.is_some();
    // The private identity must also revoke ordinary shape-keyed read proofs.
    debug_assert!(
        !found
            || unsafe {
                crate::object::proto_validity::object_is_exotic_read_receiver(obj as usize)
            },
        "arguments identity did not revoke ordinary read proofs"
    );
    found
}

#[cfg(test)]
thread_local! {
/// Every entry into [`is_arguments_object`]. Twin of
/// `set::TEST_SET_REGISTRY_PROBES` — lets a test assert that the latch
/// actually short-circuits rather than merely that nothing threw.
    static TEST_ARGUMENTS_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_arguments_registry_probe_count() -> u64 {
    TEST_ARGUMENTS_REGISTRY_PROBES.with(|c| c.get())
}

/// The birth word of an arguments object's meta record as (length, birth
/// LayoutWord), or `None` when the object has no record.
#[cfg(test)]
pub(crate) unsafe fn test_arguments_birth_word(obj: *const ObjectHeader) -> Option<(u32, u32)> {
    let state = arguments_state(obj)?;
    Some((state.element_count(), (*obj).shape_word()))
}

/// Exact birth shape proves all indexed properties remain own data slots.
/// Value stores do not weaken it; deletes, descriptors and prototype edits do.
unsafe fn born_arguments_len(obj: *const ObjectHeader) -> Option<u32> {
    let state = arguments_state(obj)?;
    crate::object::layout_desc::is_unchanged_birth_layout(obj, arguments_desc(state.strict))
        .then(|| state.element_count())
}

/// `obj[index]` read by the element path from the object kind alone: the
/// caller holds a live, unforwarded object header with `OBJ_FLAG_ARGUMENTS`.
/// Answers an element the exact birth layout still proves own data (mapped
/// indices read their cell); every other index is the caller's general path.
/// Never allocates, throws or runs user code.
#[inline]
pub(crate) unsafe fn arguments_object_index_of_flagged(
    obj: *const ObjectHeader,
    index: u32,
) -> Option<f64> {
    present_element(arguments_state_of_flagged(obj)?, index)
}

/// `arguments.length` from its fixed slot while the object keeps its exact
/// birth layout (own data `length` in named slot 0); `None` otherwise.
#[cold]
#[inline(never)]
pub(crate) unsafe fn arguments_born_length(obj: *const ObjectHeader) -> Option<f64> {
    let len = born_arguments_len(obj)?;
    Some(born_arguments_length_value(obj, len))
}

unsafe fn born_arguments_length_value(obj: *const ObjectHeader, _len: u32) -> f64 {
    *(crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj as *mut ObjectHeader)).cast::<f64>())
}

/// Borrow the caller's unchanged list after the exact layout proof. A mapped
/// list must read its cells instead. The borrower enters the callee before
/// anything can collect, and call dispatch roots/spills every supplied value
/// before its first allocation.
pub(crate) unsafe fn arguments_object_born_slice<'a>(
    obj: *const ObjectHeader,
) -> Option<&'a [f64]> {
    let born_len = born_arguments_len(obj)?;
    let length = born_arguments_length_value(obj, born_len);
    if !length.is_finite() || length < 0.0 || length > born_len as f64 {
        return None;
    }
    let n = length.floor() as usize;
    let elements = (obj as usize + ARGUMENTS_ELEMENTS_OFFSET) as *const u64;
    for i in 0..n {
        let word = *elements.add(i);
        if word == crate::value::TAG_HOLE || box_of_word(word).is_some() {
            return None;
        }
    }
    Some(std::slice::from_raw_parts(elements.cast::<f64>(), n))
}

pub(crate) unsafe fn arguments_object_get_index(
    obj: *const ObjectHeader,
    index: u32,
) -> Option<f64> {
    if !is_arguments_object(obj) {
        return None;
    }
    if let Some(value) = arguments_state(obj).and_then(|state| present_element(state, index)) {
        return Some(value);
    }
    // Interning the key allocates, and `obj` is dereferenced after it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    let key = intern_key(&index.to_string());
    Some(obj_h.with_const_ptr(|obj| js_object_get_field_by_name_f64(obj, key)))
}

pub(crate) unsafe fn arguments_object_set_index(
    obj: *mut ObjectHeader,
    index: u32,
    value: f64,
) -> bool {
    if !is_arguments_object(obj) {
        return false;
    }
    if write_present_element(obj, index, value) {
        return true;
    }
    // Interning the key allocates, and `obj` is dereferenced after it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let key = intern_key(&index.to_string());
    obj_h.with_mut_ptr(|obj| arguments_object_set_field(obj, key, value))
}

pub(crate) fn arguments_object_to_string_tag(value: f64) -> Option<f64> {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
    if !is_arguments_object(ptr) {
        return None;
    }
    let bytes = b"[object Arguments]";
    let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    Some(crate::value::js_nanbox_string(s as i64))
}

pub(crate) unsafe fn arguments_object_get_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let name = key_name(key)?;
    let state = arguments_state(obj)?;
    let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
        if super::own_key_present(obj as *mut ObjectHeader, key) {
            state.mapped_box(idx)
        } else {
            None
        }
    });

    if name == "callee" && state.restricted_callee() {
        arguments_throw_type_error(std::ptr::null(), crate::closure::JsThis::UNDEFINED);
    }
    if let Some(box_ptr) = mapped_box {
        let value = crate::r#box::js_box_get(box_ptr);
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if super::own_key_present(obj as *mut ObjectHeader, key) {
        if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
            if acc.get != 0 {
                let closure =
                    (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                if !closure.is_null() {
                    // An accessor on the arguments object runs with it as `this`.
                    let this = crate::closure::JsThis::from_f64(crate::value::js_nanbox_pointer(
                        obj as i64,
                    ));
                    let value = crate::closure::js_closure_call0(closure, this);
                    return Some(JSValue::from_bits(value.to_bits()));
                }
            }
            return Some(JSValue::undefined());
        }
        return Some(read_ordinary_own_value(obj, key));
    }
    None
}

/// The value of own key `key_bits` that an own-slot walk (JSON.stringify)
/// must read: a mapped index is the parameter's cell, not the slot (the slot
/// keeps the birth value; the cell is the live binding). `None` for every
/// other object and key, so the caller keeps its slot value.
pub(crate) unsafe fn arguments_object_mapped_value(
    obj: *const ObjectHeader,
    key_bits: u64,
) -> Option<f64> {
    let state = arguments_state(obj)?;
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(JSValue::from_bits(key_bits), &mut sso)?;
    let index = super::canonical_array_index(std::str::from_utf8(bytes).ok()?)?;
    let cell = state.mapped_box(index)?;
    Some(crate::r#box::js_box_get(cell))
}

pub(crate) unsafe fn arguments_object_set_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let Some(name) = key_name(key) else {
        return false;
    };
    let Some(state) = arguments_state(obj) else {
        return false;
    };
    let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
        if super::own_key_present(obj, key) {
            state.mapped_box(idx)
        } else {
            None
        }
    });

    if let Some(index) = super::canonical_array_index(&name) {
        if write_present_element(obj, index, value) {
            return true;
        }
    }
    if name == "callee" && state.restricted_callee() {
        arguments_throw_type_error(std::ptr::null(), crate::closure::JsThis::UNDEFINED);
    }
    if !super::own_key_present(obj, key) {
        return false;
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        if acc.set != 0 {
            let closure =
                (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !closure.is_null() {
                let this =
                    crate::closure::JsThis::from_f64(crate::value::js_nanbox_pointer(obj as i64));
                crate::closure::js_closure_call1(closure, this, value);
            }
        }
        return true;
    }
    if let Some(attrs) = get_property_attrs(obj as usize, &name) {
        if !attrs.writable() {
            crate::error::throw_immutable_write(0, &name);
        }
    }
    // A mapped key's slot holds the cell: the write goes into the cell.
    match mapped_box {
        Some(cell) => crate::r#box::js_box_set(cell, value),
        None => write_ordinary_own_value(obj, key, value),
    }
    true
}

pub(crate) unsafe fn arguments_object_before_delete(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<i32> {
    let name = key_name(key)?;
    let state = arguments_state(obj)?;
    if name == "callee" && state.restricted_callee() {
        return Some(0);
    }
    if let Some(index) = super::canonical_array_index(&name) {
        // An element is deleted where it lives (`overflow::delete`'s miss),
        // which drops its cell word, and a refused delete keeps its mapping.
        if state
            .element_slot(index)
            .is_some_and(|slot| *slot != crate::value::TAG_HOLE)
        {
            return None;
        }
        if get_property_attrs(obj as usize, &name).is_none_or(|attrs| attrs.configurable()) {
            state.unmap(index);
        }
    }
    None
}

pub(crate) unsafe fn arguments_object_after_define(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    descriptor: &super::object_ops::DescView<'_>,
) {
    let Some(name) = key_name(key) else {
        return;
    };
    let Some(index) = super::canonical_array_index(&name) else {
        return;
    };
    let Some(state) = arguments_state(obj) else {
        return;
    };
    let Some(box_ptr) = state.mapped_box(index) else {
        return;
    };
    if descriptor.has_named(b"value") {
        crate::r#box::js_box_set(
            box_ptr,
            f64::from_bits(descriptor.read_named(b"value").bits()),
        );
    }
    if descriptor.has_named(b"get")
        || descriptor.has_named(b"set")
        || descriptor.flag(b"writable") == Some(false)
    {
        state.unmap(index);
    }
}

pub(crate) unsafe fn arguments_object_descriptor(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let name = key_name(key)?;
    let restricted_callee = arguments_state(obj)?.restricted_callee();
    if let Some(view) = crate::array::subclass_elements::own_elements(obj as usize) {
        if let Some(elements_key) = crate::array::subclass_elements::key_of_header(key)
            .filter(|k| view.admits(*k) && !view.keyed(*k))
        {
            return Some(
                crate::array::subclass_elements::own_property_descriptor(view, elements_key)
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED)),
            );
        }
    }
    if !super::own_key_present(obj, key) {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if name == "callee" && restricted_callee {
        let thrower = thrower_closure_value();
        return Some(build_accessor_descriptor(thrower, thrower, false, false));
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        let get = if acc.get != 0 {
            f64::from_bits(acc.get)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let set = if acc.set != 0 {
            f64::from_bits(acc.set)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let attrs =
            get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
        return Some(build_accessor_descriptor(
            get,
            set,
            attrs.enumerable(),
            attrs.configurable(),
        ));
    }
    let value = if let Some(index) = super::canonical_array_index(&name) {
        arguments_state(obj)
            .and_then(|state| state.mapped_box(index))
            .map(|box_ptr| crate::r#box::js_box_get(box_ptr))
            .unwrap_or_else(|| f64::from_bits(read_ordinary_own_value(obj, key).bits()))
    } else {
        f64::from_bits(read_ordinary_own_value(obj, key).bits())
    };
    let attrs =
        get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
    Some(build_data_descriptor(
        value,
        attrs.writable(),
        attrs.enumerable(),
        attrs.configurable(),
    ))
}

pub(crate) unsafe fn arguments_object_to_vec(obj: *const ObjectHeader) -> Option<Vec<f64>> {
    if !is_arguments_object(obj) {
        return None;
    }
    if let Some(born_len) = born_arguments_len(obj) {
        let value = born_arguments_length_value(obj, born_len);
        // Slot VALUE still matters: writing a string length need not change
        // the shape. Non-numbers keep ToLength's full coercion semantics.
        if JSValue::from_bits(value.to_bits()).is_number() {
            let len = if value.is_finite() && value > 0.0 {
                value.floor().min(u32::MAX as f64) as u32
            } else {
                0
            };
            if len <= born_len && !value.is_infinite() {
                let state = arguments_state(obj)?;
                let values: Option<Vec<f64>> =
                    (0..len).map(|i| present_element(state, i)).collect();
                if let Some(values) = values {
                    return Some(values);
                }
            }
        }
    }
    Some(super::global_this::generic_array_like_to_vec(
        crate::value::js_nanbox_pointer(obj as i64),
    ))
}

/// Typed lowering's fast arm. The selected method VALUE must be the
/// intrinsic apply at this call, or the lookup's by-name answer for a
/// function whose pristine `Function.prototype` holds it; an override returns
/// the internal miss value and codegen invokes that already-selected value
/// through its ordinary site.
#[no_mangle]
pub extern "C-unwind" fn js_arguments_forward_apply(
    method: f64,
    target: f64,
    receiver: f64,
    args: f64,
) -> f64 {
    let bits = method.to_bits();
    if bits == super::method_call::METHOD_CALL_BY_NAME_DIRECT {
        let pair = [receiver, args];
        if let Some(value) = unsafe { super::builtin_method::by_name_function_apply(target, &pair) }
        {
            return value;
        }
    } else if bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        let address = (bits & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(address) {
            if let Some(info) =
                crate::closure::closure_info(address as *const crate::closure::ClosureHeader)
            {
                if info.code == super::global_this::function_prototype_apply_thunk as *const u8 {
                    let pair = [receiver, args];
                    if let Some(value) = unsafe {
                        super::native_call_method::run_function_intrinsic(
                            target,
                            "apply",
                            pair.as_ptr(),
                            2,
                        )
                    } {
                        return value;
                    }
                }
            }
        }
    }
    f64::from_bits(crate::value::TAG_HOLE)
}

/// Typed `f.call(x, ...arguments)`: reuse the selected method value for its
/// cold dispatch. Only the intrinsic call body and the
/// exact unchanged Arguments layout and intrinsic iterator step admit the
/// borrowed-list arm. The existing iterator owner shape proves the step.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_arguments_call_spread(
    method: f64,
    target: f64,
    method_id: i64,
    receiver: f64,
    args: f64,
) -> f64 {
    let bits = method.to_bits();
    let obj = crate::value::js_nanbox_get_pointer(args) as *const ObjectHeader;
    let intrinsic = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
        && crate::closure::closure_info(
            (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader,
        )
        .is_some_and(|info| {
            info.code == super::global_this::js_function_prototype_call_body as *const u8
        });
    let next_owner = super::iterator_prototypes::array_iterator_prototype_addr();
    if intrinsic
        && is_arguments_object(obj)
        && arguments_object_born_slice(obj).is_some()
        && arguments_iterator_is_intrinsic(obj)
        && next_owner != 0
        && super::iterator_prototypes::array_iterator_next_is_intrinsic(
            next_owner as *const ObjectHeader,
        )
    {
        // `call` and `apply` enter the same target with the same receiver.
        let pair = [receiver, args];
        if let Some(result) =
            super::native_call_method::run_function_intrinsic(target, "apply", pair.as_ptr(), 2)
        {
            return result;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(method);
    let target = scope.root_nanbox_f64(target);
    let receiver = scope.root_nanbox_f64(receiver);
    let args = scope.root_nanbox_f64(args);
    let mut values = vec![receiver];
    let symbol = crate::symbol::well_known_symbol("iterator");
    let own_iterator = crate::symbol::js_object_has_own_symbol_property(
        args.get_nanbox_f64(),
        crate::value::js_nanbox_pointer(symbol as i64),
    );
    // Ask the actual iterator method on mutated arguments (including a user
    // iterator getter) before falling back to their intrinsic array iterator.
    let iter =
        crate::symbol::get_iterator_from_method(args.get_nanbox_f64()).unwrap_or_else(|| {
            if own_iterator {
                super::throw_object_type_error(b"Arguments iterator is not callable");
            }
            crate::symbol::js_get_iterator(args.get_nanbox_f64())
        });
    let iter = scope.root_nanbox_f64(iter);
    let next = scope.root_nanbox_f64(crate::array::js_iterator_next_method(iter.get_nanbox_f64()));
    loop {
        let mut value = f64::from_bits(crate::value::TAG_UNDEFINED);
        if crate::array::js_iterator_step(iter.get_nanbox_f64(), next.get_nanbox_f64(), &mut value)
            != 0
        {
            break;
        }
        values.push(scope.root_nanbox_f64(value));
    }
    let values = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&values);
    super::method_call::js_method_call_split(
        method.get_nanbox_f64(),
        0,
        target.get_nanbox_f64(),
        method_id,
        values.as_ptr(),
        values.len(),
    )
}

/// G8: typed `f.call(x, ...arguments)` in a function whose Arguments object
/// codegen elided (perry-codegen `codegen/arguments.rs`): `bundle` is the
/// caller's private argument Array, holding exactly the values the object
/// would hold. Spreading the object walks its own @@iterator
/// (%Array.prototype.values% from birth), so with `%ArrayIteratorPrototype%.next`
/// the builtin the spread list is the bundle's elements, and the intrinsic
/// `call` enters the target as `apply(x, bundle)` does. Any other method or
/// a patched `next` gets the object the binding stands for, rebuilt here, on
/// [`js_arguments_call_spread`]'s complete path.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_arguments_bundle_call_spread(
    method: f64,
    target: f64,
    method_id: i64,
    receiver: f64,
    bundle: f64,
) -> f64 {
    let bits = method.to_bits();
    let intrinsic = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
        && crate::closure::closure_info(
            (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader,
        )
        .is_some_and(|info| {
            info.code == super::global_this::js_function_prototype_call_body as *const u8
        });
    if intrinsic && crate::array::intrinsic_array_like_iteration(true) {
        let pair = [receiver, bundle];
        if let Some(result) =
            super::native_call_method::run_function_intrinsic(target, "apply", pair.as_ptr(), 2)
        {
            return result;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(method);
    let target = scope.root_nanbox_f64(target);
    let receiver = scope.root_nanbox_f64(receiver);
    let obj = js_arguments_object_alloc(bundle, f64::from_bits(crate::value::TAG_UNDEFINED), 0);
    js_arguments_call_spread(
        method.get_nanbox_f64(),
        target.get_nanbox_f64(),
        method_id,
        receiver.get_nanbox_f64(),
        crate::value::js_nanbox_pointer(obj as i64),
    )
}

/// Exact shape admits the length SLOT; a mutated or mistyped receiver keeps
/// the ordinary Get. Return the JS value, rather than a ToLength conversion.
#[no_mangle]
pub extern "C-unwind" fn js_arguments_length_value(args: f64) -> f64 {
    let obj = crate::value::js_nanbox_get_pointer(args) as *const ObjectHeader;
    if is_arguments_object(obj) {
        if let Some(len) = unsafe { born_arguments_len(obj) } {
            let value = unsafe { born_arguments_length_value(obj, len) };
            if value.to_bits() != crate::value::TAG_HOLE {
                return value;
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = scope.root_nanbox_f64(args);
    let key = intern_key("length");
    crate::value::js_dyn_index_get(
        args.get_nanbox_f64(),
        crate::value::js_nanbox_string(key as i64),
    )
}

/// Live `length` read used by the Arguments Array iterator.
pub(crate) unsafe fn arguments_object_length(obj: *const ObjectHeader) -> u32 {
    if !is_arguments_object(obj) {
        return 0;
    }
    let value = js_arguments_length_value(crate::value::js_nanbox_pointer(obj as i64));
    let tagged = JSValue::from_bits(value.to_bits());
    if tagged.is_bigint() {
        super::throw_object_type_error(b"Cannot convert a BigInt value to a number");
    }
    let value = if tagged.is_number() {
        value
    } else {
        crate::builtins::js_number_coerce(value)
    };
    if !value.is_nan() && value > 0.0 {
        value.floor().min(u32::MAX as f64) as u32
    } else {
        0
    }
}

/// Live indexed read used by the Arguments Array iterator.
pub(crate) unsafe fn arguments_object_index_value(obj: *const ObjectHeader, index: u32) -> f64 {
    if !is_arguments_object(obj) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    arguments_object_get_index(obj, index)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) unsafe fn arguments_object_to_array(
    obj: *const ObjectHeader,
) -> Option<*mut ArrayHeader> {
    if !is_arguments_object(obj) {
        return None;
    }
    // Every read below interns its key and may run an accessor, so the source
    // and the result are rooted, and each value lands in the result before
    // the next allocation. The result is born with room for every element, so
    // no push grows it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    let len = obj_h.with_const_ptr(|obj| arguments_object_length(obj));
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(len));
    for i in 0..len {
        let value = obj_h.with_const_ptr(|obj| arguments_object_index_value(obj, i));
        let next = out.with_mut_ptr(|arr| crate::array::js_array_push_f64(arr, value));
        out.set_raw_mut_ptr(next);
    }
    Some(out.with_mut_ptr(|arr: *mut ArrayHeader| arr))
}

/// The call-argument drain of `f(...x)`: the one spread path
/// ([`crate::array::spread_values`], GetIterator semantics: a non-iterable
/// operand throws). A proven dense array is returned as is (its readers copy
/// the elements).
#[no_mangle]
pub extern "C" fn js_array_like_to_array(value: f64) -> *mut ArrayHeader {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_pointer()
        && crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE
    {
        if let Some(arr) = crate::array::plain_call_spread_source(value) {
            return arr as *mut ArrayHeader;
        }
        if let Some(arr) = crate::array::dense_spread_source(value) {
            return arr as *mut ArrayHeader;
        }
    }
    crate::array::spread_values(value)
}

unsafe fn read_ordinary_own_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let key_count = keys_view.count() as usize;
    let alloc_limit = crate::object::object_inline_capacity(obj) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            let i = keys_view.storage_slot(i as u32) as usize;
            if i < alloc_limit {
                return js_object_get_field(obj, i as u32);
            }
            return overflow_get(obj as usize, i)
                .map(JSValue::from_bits)
                .unwrap_or_else(JSValue::undefined);
        }
    }
    JSValue::undefined()
}

unsafe fn write_ordinary_own_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let key_count = keys_view.count() as usize;
    let alloc_limit = crate::object::object_inline_capacity(obj) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            let i = keys_view.storage_slot(i as u32) as usize;
            if i < alloc_limit {
                js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
            } else {
                overflow_set(obj as usize, i, value.to_bits());
            }
            return;
        }
    }
}

/// Slot 2 (@@iterator) still holds %Array.prototype.values%.
unsafe fn arguments_iterator_is_intrinsic(obj: *const ObjectHeader) -> bool {
    let word =
        *crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj as *mut ObjectHeader)).add(2);
    let raw = crate::value::js_nanbox_get_pointer(f64::from_bits(word))
        as *const crate::closure::ClosureHeader;
    JSValue::from_bits(word).is_pointer()
        && crate::closure::closure_info(raw).is_some_and(|info| {
            info.code == crate::object::array_prototype_values_thunk as *const u8
        })
}

/// `defineProperty` of a present inline element `index`. A plain data define
/// (no accessor half, no attribute set to false) stays an element: its value
/// is written through (the cell of a mapped element: the mapping stays) and
/// the define is done (`true`). Any other define first applies the value to a
/// mapped cell (Set(map, P, V)), then moves the element out of the inline
/// elements into an ordinary own data property `index` (W E C, unmapped) that
/// the caller's define then edits (`false`). Not an element: `false`.
pub(crate) unsafe fn arguments_define_element(
    receiver: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    descriptor: &super::object_ops::DescView<'_>,
) -> bool {
    let bits = receiver.get_nanbox_u64();
    if !JSValue::from_bits(bits).is_pointer() {
        return false;
    }
    let obj = crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut ObjectHeader;
    let Some(state) = arguments_state(obj) else {
        return false;
    };
    // The key is decoded only for an arguments receiver (its birth flag).
    let Some(crate::array::subclass_elements::ElementsKey::Index(index)) =
        crate::array::subclass_elements::key_of_value(key.get_nanbox_f64())
    else {
        return false;
    };
    let Some(slot) = state.element_slot(index) else {
        return false;
    };
    let word = *slot;
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if word == crate::value::TAG_HOLE
        || (*gc)._reserved & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED) != 0
    {
        return false;
    }
    let value = descriptor
        .has_named(b"value")
        .then(|| f64::from_bits(descriptor.read_named(b"value").bits()));
    let data_keeps_map = !descriptor.has_named(b"get")
        && !descriptor.has_named(b"set")
        && descriptor.flag(b"writable") != Some(false);
    let plain = !descriptor.has_named(b"get")
        && !descriptor.has_named(b"set")
        && descriptor.flag(b"writable") != Some(false)
        && descriptor.flag(b"enumerable") != Some(false)
        && descriptor.flag(b"configurable") != Some(false);
    let cell = box_of_word(word);
    if plain {
        if let Some(value) = value {
            match cell {
                Some(cell) => crate::r#box::js_box_set(cell, value),
                None => store_element_word(obj, slot, value.to_bits()),
            }
        }
        return true;
    }
    let current = match cell {
        Some(cell) => {
            if let Some(value) = value {
                crate::r#box::js_box_set(cell, value);
            }
            crate::r#box::js_box_get(cell)
        }
        None => f64::from_bits(word),
    };
    // The property leaves the elements whole: the element word becomes the
    // hole, and key `index` takes the box word when a data define keeps
    // `writable` (the alias holds, wherever the property lives), else the
    // value.
    // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is a non-pointer sentinel.
    slot.write(crate::value::TAG_HOLE);
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_value = scope.root_nanbox_f64(if cell.is_some() && data_keeps_map {
        f64::from_bits(word)
    } else {
        current
    });
    let key = intern_key(&index.to_string());
    let obj = crate::value::js_nanbox_get_pointer(f64::from_bits(receiver.get_nanbox_u64()))
        as *mut ObjectHeader;
    js_object_set_field_by_name(obj, key, key_value.get_nanbox_f64());
    false
}
