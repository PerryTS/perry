//! A function object's own properties, stored IN the function object (D1).
//!
//! `ClosureHeader::props` points at the function's BAG: a runtime-internal,
//! null-prototype `ObjectHeader` whose keys and slots are the function's own
//! string and symbol properties, with attributes and accessor pairs, in creation
//! order. It is created on
//! the first own-property write and is a traced, rewritten raw-pointer child
//! edge of the closure (`gc::layout`'s `ClosureCaptures` arm), so it moves
//! and dies with the function — no address-keyed table, no re-key hook, no
//! dead-owner prune, no young log.
//!
//! The bag's own `ObjectMeta.expando` (an ordinary child edge of the bag)
//! holds the function's rare internal STATE record, another null-prototype
//! object: the #3655 deleted-synthesized-key markers (`"d:" + key`) and the
//! recorded `[[Prototype]]` (`"p"`). Its keys are namespaced, and it is never
//! reachable from JS.
//!
//! Writers run under `GcSuppressScope`: the ~100 callers of
//! `closure_set_dynamic_prop` hold raw closure addresses across the call, as
//! they always could when the store was a Rust `HashMap`, so the allocations
//! here (bag, key string, state record) must not move anything.
use super::ClosureHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The intrinsic own data keys a function's bag is born with, in creation
/// order: every function's `length` and `name`, then a generator function's
/// `prototype`.
const INTRINSIC_OWN_KEYS: [&str; 3] = ["length", "name", "prototype"];

/// A generator function's `prototype`: writable, non-enumerable,
/// non-configurable (ECMA-262 §15.5.5 / §15.6.5 InstantiateGeneratorFunction*).
pub(crate) const GENERATOR_PROTOTYPE_ATTRS: crate::object::PropertyAttrs =
    crate::object::PropertyAttrs::new(true, false, false);

/// Does the closure at `ptr` have a (sync or async) generator body? Those
/// declare an own `prototype` that their bag is born with.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn closure_has_generator_body(closure: *const ClosureHeader) -> bool {
    let info = (*closure).info;
    !info.is_null()
        && !super::shape::is_class_info(info)
        && (*info).code != super::BOUND_FUNCTION_FUNC_PTR
        && (*info).code != super::BOUND_METHOD_FUNC_PTR
        && (*info).flags & (super::FN_GENERATOR | super::FN_ASYNC_GENERATOR) != 0
}

/// Inline slots a function's bag is born with beyond its keys when its
/// first own key is a program's define or store (`object::function_own_put`):
/// such a function usually takes several (ArkType's callable nodes,
/// `Object.assign` of attachments, then class fields), and each one past the
/// birth bound would otherwise go to the spill buffer. A bag born holding a
/// known key set (builtin metadata, `closure_define_first_props`) is born
/// exact: room there only re-publishes its shape at a later add.
/// Two slots: measured on Supabase startup (0: 6.015 G, 2: 5.998 G, 4 and 6:
/// 6.27 G, the larger bags cost one more minor collection).
pub(crate) const BAG_ROOM: u32 = 2;

const STATE_PROTO: &str = "p";
const DELETED_PREFIX: &str = "d:";
/// The tombstone of a deleted info-derived own key (`length`, `name`): the
/// entry the function's info answers is gone for good, so a bag entry of
/// that key defined after the delete is a later key (it lists after the
/// birth keys), not the birth entry.
const TOMBSTONE_PREFIX: &str = "t:";
const INTERNAL_PREFIX: &str = "i:";

/// The bag of the closure at `ptr` (null when it never had an own property).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn bag_of(ptr: usize) -> *mut ObjectHeader {
    (*(ptr as *const ClosureHeader)).props
}

/// The function's own symbol value: the bag's own route reaches the same
/// `shaped_symbols::get` body as an ordinary holder.
#[inline(never)]
pub(crate) unsafe fn fn_sym_get(ptr: usize, symbol: usize) -> Option<u64> {
    crate::object::shaped_symbols::get(bag_of(ptr) as usize, symbol)
}

/// A symbol data definition on the function's store, after the caller ran
/// the owner's prologue once (`shaped_symbols::define`).
#[inline(never)]
pub(crate) unsafe fn fn_sym_define(ptr: usize, symbol: usize, bits: u64, entry: u8) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && crate::object::shaped_symbols::define_on(bag, symbol, bits, entry)
}

/// A symbol key's attribute byte edit on the function's store.
#[inline(never)]
pub(crate) unsafe fn fn_sym_set_entry(ptr: usize, symbol: usize, entry: u8) {
    let bag = bag_of(ptr);
    if !bag.is_null() {
        crate::object::shaped_symbols::set_entry_on(bag, symbol, entry)
    }
}

/// A symbol accessor definition on the function's store.
#[inline(never)]
pub(crate) unsafe fn fn_sym_define_accessor(ptr: usize, symbol: usize, get: u64, set: u64) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && crate::object::shaped_symbols::define_accessor_on(bag, symbol, get, set)
}

/// A symbol key delete on the function's store.
#[inline(never)]
pub(crate) unsafe fn fn_sym_delete(ptr: usize, symbol: usize) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && crate::object::shaped_symbols::delete_on(ptr, bag, symbol)
}

/// The function's own symbol slot (`own_symbol_slot` on its store).
#[inline(never)]
pub(crate) unsafe fn fn_sym_slot(ptr: usize, symbol: f64) -> Option<crate::symbol::OwnSymbolSlot> {
    crate::symbol::own_symbol_slot(crate::value::js_nanbox_pointer(bag_of(ptr) as i64), symbol)
}

/// Does the function's store own `symbol`?
#[inline(never)]
pub(crate) unsafe fn fn_sym_has(ptr: usize, symbol: usize) -> bool {
    crate::object::shaped_symbols::position(bag_of(ptr), symbol).is_some()
}

/// Learn the ConstFn lanes of a native prototype function's own store.
#[inline(never)]
pub(crate) unsafe fn fn_learn_constfn(ptr: usize) {
    let bag = bag_of(ptr);
    if !crate::object::runtime_layouts::on_declared_prototype_layout(bag) {
        crate::object::global_this::learn_prototype_lanes(bag);
    }
}

/// The function's own symbol entries (`shaped_symbols::entries` on the bag).
#[inline(never)]
pub(crate) unsafe fn fn_sym_entries(ptr: usize, include_private: bool) -> Vec<(usize, u64)> {
    crate::object::shaped_symbols::entries(bag_of(ptr) as usize, include_private)
}

/// The function's own symbol entry byte (`shaped_symbols::entry` on the bag).
#[inline(never)]
pub(crate) unsafe fn fn_sym_entry(ptr: usize, symbol: usize) -> Option<u8> {
    crate::object::shaped_symbols::entry(bag_of(ptr) as usize, symbol)
}

/// Is `key` a declared static of the closure at `ptr` while it is an unborn
/// lazy constructor (`BuiltinConstructorDeclaration::statics`)?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cold]
#[inline(never)]
pub(crate) unsafe fn is_declared_static(ptr: usize, key: &[u8]) -> bool {
    crate::object::unborn_declared_statics_of(ptr)
        .iter()
        .any(|declared| declared.as_bytes() == key)
}

/// The bag a value read of the closure at `ptr` sees while it has none: an
/// unborn lazy constructor owns its declared statics (`key`: that one;
/// `None`: any of them), so the read births its family and reads the bag the
/// birth installed. Null for every other closure. The one out-of-line slow
/// path of the bag readers; presence and listing never birth.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cold]
#[inline(never)]
unsafe fn unborn_bag_for_read(ptr: usize, key: Option<&[u8]>) -> *mut ObjectHeader {
    let declared = match key {
        Some(key) => is_declared_static(ptr, key),
        None => !crate::object::unborn_declared_statics_of(ptr).is_empty(),
    };
    if declared && crate::object::birth_unborn_family_of(ptr) {
        bag_of(ptr)
    } else {
        std::ptr::null_mut()
    }
}

/// The holder of a function's own-key attributes, accessors and private
/// elements (DESIGN_FR step 1): today its bag (`create`: allocated on the
/// first attribute, accessor or private definition). Step 2 makes the
/// function itself the holder; every such site of a function reaches its
/// holder here.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn key_holder(ptr: usize, create: bool) -> *mut ObjectHeader {
    if create {
        bag_ensure(ptr)
    } else {
        bag_of(ptr)
    }
}

/// Allocate the bag if absent and install it with the store barrier.
pub(crate) unsafe fn bag_ensure(ptr: usize) -> *mut ObjectHeader {
    let existing = bag_of(ptr);
    if !existing.is_null() {
        return existing;
    }
    if bag_born_with_attrs(ptr, &[], &[]) {
        return bag_of(ptr);
    }
    // Reading a lazy intrinsic name can itself materialize the bag.
    let existing = bag_of(ptr);
    if !existing.is_null() {
        return existing;
    }
    let bag = crate::object::js_object_alloc_null_proto(0, 0);
    install_bag(ptr, bag);
    bag
}

/// Give the closure at `ptr` its bag `bag` with the store barrier.
pub(crate) unsafe fn install_bag(ptr: usize, bag: *mut ObjectHeader) {
    let closure = ptr as *mut ClosureHeader;
    // A closure is born `GC_LAYOUT_POINTER_FREE` when its captures hold no
    // pointer; some collector paths treat that state as "no child edge at
    // all". It now has one, so it leaves that state (#7630's tag-checked
    // `UNKNOWN`, the state a pointer capture store would also produce).
    crate::gc::layout_note_closure_edge_installed(ptr as *mut u8);
    // GC_STORE_AUDIT(BARRIERED): header raw-pointer edge store + object-slot
    // barrier, mirroring `object_meta_ensure`'s `meta` install.
    (*closure).props = bag;
    crate::gc::runtime_write_barrier_slot(ptr, &(*closure).props as *const _ as usize, bag as u64);
}

/// The first own data properties of a function that has none yet, all at
/// once: its bag is born holding `entries` in order, in ONE shape
/// ([`crate::object::alloc::object_alloc_null_proto_with_key_attrs`]), instead of growing
/// one key-add transition per key. This is a define: no setter or attribute is
/// consulted, as for every builtin install. False, with nothing done, when the
/// function already has a bag (or a declared static could hold one of the
/// keys); the caller then installs the ordinary way.
///
/// # Safety
/// `ptr` is a proven, live closure cell, and the keys of `entries` are
/// distinct.
pub(crate) unsafe fn bag_born_with_attrs(
    ptr: usize,
    entries: &[(&str, f64)],
    attrs: &[u8],
) -> bool {
    bag_born::<0>(ptr, entries, attrs)
}

/// [`bag_born_with_attrs`] with [`BAG_ROOM`] inline slots past its keys.
///
/// # Safety
/// As [`bag_born_with_attrs`].
pub(crate) unsafe fn bag_born_with_room(ptr: usize, entries: &[(&str, f64)], attrs: &[u8]) -> bool {
    bag_born::<BAG_ROOM>(ptr, entries, attrs)
}

/// [`bag_born_with_attrs`] with `ROOM` inline slots past its keys.
#[inline(always)]
unsafe fn bag_born<const ROOM: u32>(ptr: usize, entries: &[(&str, f64)], attrs: &[u8]) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    // An unborn lazy builtin family's constructor gets its bag (`length`,
    // `name`, `prototype` and its statics) from its family's birth.
    if bag_of(ptr).is_null() {
        crate::object::birth_unborn_family_of(ptr);
    }
    if !bag_of(ptr).is_null()
        || entries.iter().any(|(key, _)| {
            crate::object::class_value::holds_declared_static_method(ptr, key).is_some()
        })
    {
        return false;
    }
    debug_assert!(entries
        .iter()
        .enumerate()
        .all(|(i, (a, _))| entries[..i].iter().all(|(b, _)| a != b)));
    // A function's intrinsic data slots precede subsequently defined keys.
    // Materializing them at bag birth preserves their creation order when
    // a later definition makes either slot enumerable. Classes already birth
    // their own declaration layout and use that layout unchanged.
    let class = super::shape::is_class_info((*(ptr as *const ClosureHeader)).info);
    let bag = if class {
        crate::object::alloc::object_alloc_null_proto_with_key_attrs(entries, attrs)
    } else {
        let intrinsic_attrs = crate::object::key_attrs::attr_bits_to_entry(
            crate::object::PropertyAttrs::new(false, false, true).bits,
        );
        // A generator function also declares `prototype` (writable,
        // non-enumerable, non-configurable): its bag is born with the slot
        // and its original prototype object, so the slot is a fact of the
        // function's keyed shape, never a later key-add or attribute edit.
        // Its original prototype also stays in the bag's private
        // `[[GeneratorPrototype]]` slot, which no later write of `prototype`
        // replaces: an instance born before the bag names the original
        // through its owner (`generator_state::generator_record_prototype_word`).
        let generator = closure_has_generator_body(ptr as *const ClosureHeader);
        let count = if generator { 4 } else { 2 };
        let original_key = crate::object::generator_original_prototype_key();
        let names = [
            INTRINSIC_OWN_KEYS[0],
            INTRINSIC_OWN_KEYS[1],
            INTRINSIC_OWN_KEYS[2],
            original_key,
        ];
        let names = &names[..count];
        let mut own = [
            ("length", 0.0),
            ("name", 0.0),
            ("prototype", 0.0),
            (original_key, 0.0),
        ];
        let mut own_attrs = [
            intrinsic_attrs,
            intrinsic_attrs,
            crate::object::key_attrs::attr_bits_to_entry(GENERATOR_PROTOTYPE_ATTRS.bits),
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        ];
        let original = if generator {
            crate::object::generator_original_prototype(ptr)
        } else {
            0.0
        };
        for (index, &key) in names.iter().enumerate() {
            if let Some(i) = entries.iter().position(|(name, _)| *name == key) {
                own[index] = entries[i];
                own_attrs[index] = attrs.get(i).copied().unwrap_or(own_attrs[index]);
            } else if index >= 2 {
                own[index].1 = original;
            } else {
                let closure = ptr as *const ClosureHeader;
                let value = if index == 0 {
                    crate::object::bound_native_callable_value_arity(
                        crate::value::js_nanbox_pointer(ptr as i64),
                    )
                    .or_else(|| super::bound_function_length(ptr))
                    .or_else(|| super::closure_length(closure))
                    .unwrap_or(0) as f64
                } else if (*closure).code() == super::BOUND_FUNCTION_FUNC_PTR {
                    // The bag records it: computed, not defined (a define
                    // would birth a bag inside this birth).
                    super::dispatch::bound::bound_function_name_value(ptr).0
                } else {
                    let name =
                        crate::builtins::function_name_for_closure(closure).unwrap_or_default();
                    let name =
                        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    crate::value::js_nanbox_string(name as i64)
                };
                own[index].1 = value;
            }
        }
        if !bag_of(ptr).is_null() {
            return false;
        }
        let own = &own[..count];
        let own_attrs = &own_attrs[..count];
        if entries.len() >= count
            && attrs.len() == entries.len()
            && entries[..count]
                .iter()
                .zip(own)
                .all(|(&(key, value), &(intrinsic, expected))| {
                    key == intrinsic && value.to_bits() == expected.to_bits()
                })
        {
            born_alloc::<ROOM>(entries, attrs)
        } else if entries.iter().all(|(key, _)| names.contains(key)) {
            born_alloc::<ROOM>(own, own_attrs)
        } else {
            let mut all = Vec::with_capacity(entries.len() + count);
            let mut all_attrs = Vec::with_capacity(entries.len() + count);
            all.extend_from_slice(own);
            all_attrs.extend_from_slice(own_attrs);
            for (i, &(key, value)) in entries.iter().enumerate() {
                if names.contains(&key) {
                    continue;
                }
                all.push((key, value));
                all_attrs.push(attrs.get(i).copied().unwrap_or(0));
            }
            born_alloc::<ROOM>(&all, &all_attrs)
        }
    };
    install_bag(ptr, bag);
    true
}

/// A function bag's birth allocation, with `ROOM` inline slots past its keys.
#[inline(always)]
unsafe fn born_alloc<const ROOM: u32>(entries: &[(&str, f64)], attrs: &[u8]) -> *mut ObjectHeader {
    if ROOM == 0 {
        crate::object::alloc::object_alloc_null_proto_with_key_attrs(entries, attrs)
    } else {
        crate::object::alloc::object_alloc_null_proto_with_key_attrs_room::<ROOM>(entries, attrs)
    }
}

/// Own data lookup in an ordinary (or dictionary-mode) bag by key bytes.
unsafe fn object_own_get(obj: *const ObjectHeader, key: &[u8]) -> Option<f64> {
    // One shape lookup for an ordinary bag: keys, count and inline bound all
    // come from the same descriptor. The layout answers alone unless the
    // shadow bit says the bag's own keys extend past it (§3.6).
    if let Some(d) = crate::object::shapes::object_shape_descriptor(obj) {
        if d.object_kind.is_ordinary_layout()
            && d.keys != 0
            && !crate::object::overflow::shadowed(obj)
        {
            let keys = d.keys as usize as *const crate::array::ArrayHeader;
            let slot = crate::object::keys_find_property_slot_by_bytes(
                d.keys_view(),
                d.logical_key_count,
                key,
            )?;
            // An accessor key's slot holds its getter/setter pair, never a
            // data value.
            if d.summary & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
                && crate::object::key_attrs::key_is_accessor_at(keys, slot as u32)
            {
                return None;
            }
            let value = crate::object::object_key_field_at_with_keys(
                obj,
                keys,
                slot,
                d.live_inline_slot_count,
            );
            if value.bits() == crate::value::TAG_HOLE {
                return None;
            }
            return Some(f64::from_bits(value.bits()));
        }
    }
    // With an overflow, the entry's position is not a field slot: the entry
    // holds the value or a reference to its fixed slot (§3.4).
    let t = crate::object::overflow::table(obj);
    if !t.is_null() {
        let i = crate::object::overflow::find_bytes(t, key)?;
        if crate::object::key_attrs::key_is_accessor_at(t.cast(), i) {
            return None;
        }
        let bits = crate::object::overflow::get_from(t, obj, i)?;
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        return Some(f64::from_bits(bits));
    }
    let keys = crate::object::object_keys(obj);
    let arr = keys.arr();
    if arr.is_null() {
        return None;
    }
    let slot = crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key)?;
    if crate::object::key_attrs::key_is_accessor_at(arr, slot as u32) {
        return None;
    }
    let live = crate::object::object_live_slot_count(obj);
    let value = crate::object::object_key_field_at_with_keys(obj, arr, slot, live);
    if value.bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(f64::from_bits(value.bits()))
}

/// The function's `prototype` slot (its first own slot, after its
/// captures), or null when its family has none or its cell was born
/// without it (`alloc::closure_own_slot_count`).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn prototype_slot(ptr: usize) -> *mut u64 {
    let c = ptr as *mut super::ClosureHeader;
    if super::alloc::closure_own_slot_count(c) == 0 {
        return std::ptr::null_mut();
    }
    super::alloc::closure_capture_slots_mut(c)
        .add(super::real_capture_count((*c).capture_count) as usize)
}

/// The function's materialized `prototype` held in its slot (a hole: not
/// made yet, or given up to the bag by a redefinition).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn prototype_slot_value(ptr: usize) -> Option<f64> {
    let slot = prototype_slot(ptr);
    if slot.is_null() || *slot == crate::value::TAG_HOLE {
        return None;
    }
    Some(f64::from_bits(*slot))
}

/// Does the function's bag itself hold `prototype` (a redefinition with
/// other attributes moved it there from the slot)?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_holds_prototype(ptr: usize) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && object_own_get(bag, b"prototype").is_some()
}

/// Does the function's `prototype` slot hold the key: its value, or the hole
/// of a `prototype` not made yet (a redefinition that moved it to the bag
/// leaves the hole with the key in the bag)? Its attributes are then the
/// birth ones (writable, not enumerable, not configurable).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn prototype_slot_holds_key(ptr: usize) -> bool {
    let slot = prototype_slot(ptr);
    !slot.is_null() && (*slot != crate::value::TAG_HOLE || !bag_holds_prototype(ptr))
}

/// The function's `prototype` taken out of its slot (left a hole), when the
/// slot holds it.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn prototype_slot_take(ptr: usize) -> Option<f64> {
    let value = prototype_slot_value(ptr)?;
    *prototype_slot(ptr) = crate::value::TAG_HOLE;
    Some(value)
}

/// Store `prototype` in the function's slot: false (nothing stored) when it
/// has no slot or its bag holds the key (a redefinition with other
/// attributes moved it there).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn prototype_slot_store(ptr: usize, value: f64) -> bool {
    let slot = prototype_slot(ptr);
    if slot.is_null() {
        return false;
    }
    if *slot == crate::value::TAG_HOLE && bag_holds_prototype(ptr) {
        return false;
    }
    let c = ptr as *mut super::ClosureHeader;
    *slot = value.to_bits();
    super::alloc::note_closure_capture_slot(
        c,
        super::real_capture_count((*c).capture_count) as usize,
        value.to_bits(),
    );
    true
}

/// `key` is `prototype` and the function's slot holds its value: that value.
/// Out of line, so the bag paths of user keys stay as they were.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cold]
#[inline(never)]
unsafe fn prototype_slot_get(ptr: usize, key: &[u8]) -> Option<f64> {
    if key != b"prototype" {
        return None;
    }
    prototype_slot_value(ptr)
}

/// `key` is `prototype` and the function's slot stored `value`
/// ([`prototype_slot_store`]). Out of line, as [`prototype_slot_get`].
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cold]
#[inline(never)]
unsafe fn prototype_slot_set(ptr: usize, key: &[u8], value: f64) -> bool {
    key == b"prototype" && prototype_slot_store(ptr, value)
}

/// The function's own data property `key`, if present.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_get(ptr: usize, key: &[u8]) -> Option<f64> {
    // Only a nine-byte key can be `prototype`: every other key takes the
    // bag path with one length compare.
    if key.len() == 9 {
        if let Some(value) = prototype_slot_get(ptr, key) {
            return Some(value);
        }
    }
    let mut bag = bag_of(ptr);
    if bag.is_null() {
        // Reading a declared static of an unborn lazy constructor births its
        // family; the birth installs the static.
        bag = unborn_bag_for_read(ptr, Some(key));
        if bag.is_null() {
            return None;
        }
    }
    object_own_get(bag, key)
}

unsafe fn object_own_set(obj: *mut ObjectHeader, key: &str, value: f64) {
    object_own_set_bytes(obj, key.as_bytes(), value);
}

unsafe fn object_own_set_bytes(obj: *mut ObjectHeader, key: &[u8], value: f64) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    // These are ordinary properties of the bag, even when their spelling
    // matches an internal private storage key. Do not reinterpret the string
    // as a compiled PrivateReference.
    crate::object::set_field_by_name_object_tail(obj, key, value);
}

/// Define/overwrite the function's own data property `key` (plain `[[Set]]`
/// on the null-prototype bag: no inherited setter can run).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_set(ptr: usize, key: &str, value: f64) {
    bag_set_bytes(ptr, key.as_bytes(), value);
}

pub(crate) unsafe fn bag_set_bytes(ptr: usize, key: &[u8], value: f64) {
    if key.len() == 9 && prototype_slot_set(ptr, key, value) {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    object_own_set_bytes(bag, key, value);
    declared_value_replaced_bytes(ptr, key, declared);
}

unsafe fn declared_value_replaced_bytes(ptr: usize, key: &[u8], declared: Option<f64>) {
    let Some(old) = declared else { return };
    if bag_get(ptr, key).is_some_and(|v| v.to_bits() == old.to_bits()) {
        return;
    }
    crate::object::shapes::transition_object_shape_semantics(bag_of(ptr));
}

/// [[DefineOwnProperty]] of own data property `key` with just a value: the
/// value is stored and the key keeps (or, when new, gets default) attributes.
/// Unlike [`bag_set`] it ignores the key's `writable` attribute; a caller
/// that is performing a [[Set]] has checked it (a class function object's
/// statics, whose attributes live with these keys).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_define_value(ptr: usize, key: &str, value: f64) {
    bag_define_value_bytes(ptr, key.as_bytes(), value);
}

pub(crate) unsafe fn bag_define_value_bytes(ptr: usize, key: &[u8], value: f64) {
    if key.len() == 9 && prototype_slot_set(ptr, key, value) {
        return;
    }
    bag_define_value_in_bag(ptr, key, value);
}

/// [`bag_define_value_bytes`] into the bag itself: the `prototype` slot
/// gives its value up to the bag (`HolderEdit::materialize_data_keys`).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_define_value_in_bag(ptr: usize, key: &[u8], value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    // Only a new bag needs a UTF-8 key for its birth declaration. Existing
    // bags use byte keys below, so their updates need no UTF-8 validation.
    if bag_of(ptr).is_null() {
        if let Ok(key) = std::str::from_utf8(key) {
            if bag_born_with_attrs(ptr, &[(key, value)], &[]) {
                return;
            }
        }
    }
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    if !bag_has_own(ptr, key) {
        // A NEW property — including one `delete` removed earlier: it is
        // appended as any new key is, so it enumerates last. The in-place
        // store below would find the deleted key's tombstoned entry and
        // bring the property back at its old position. This is
        // [[DefineOwnProperty]], validated by the caller, so the bag's
        // extensibility is not consulted: the `prototype` slot giving its key
        // up to a non-extensible function's bag moves an existing own key, it
        // does not add one.
        let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
        declared_value_replaced_bytes(ptr, key, declared);
        return;
    }
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
    declared_value_replaced_bytes(ptr, key, declared);
}

/// [[DefineOwnProperty]] of own data property `key` with its value AND its
/// attributes (`attrs`, `PropertyAttrs` bits) in one step: a new key is
/// claimed carrying its final key entry (one key-add, no attribute rewrite
/// after it); an existing key keeps its position, takes the value, and its
/// entry is rewritten only when it differs. The caller has validated the
/// descriptor and refreshes the function's shape.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_define_data_with_attrs_bytes(
    ptr: usize,
    key: &[u8],
    value: f64,
    attrs: u8,
) {
    if key == b"prototype" {
        // The slot holds `prototype` with its birth attributes (writable,
        // not enumerable, not configurable); other attributes live with the
        // key in the bag, and the slot gives the value up to it.
        if attrs == crate::object::PropertyAttrs::new(true, false, false).bits {
            if prototype_slot_store(ptr, value) {
                return;
            }
        } else {
            let slot = prototype_slot(ptr);
            if !slot.is_null() {
                *slot = crate::value::TAG_HOLE;
            }
        }
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    // UTF-8 is required only by the bag's birth declaration. Once the
    // function owns a bag, its shape and byte-key operations are sufficient.
    if bag_of(ptr).is_null() {
        if let Ok(key) = std::str::from_utf8(key) {
            if bag_born_with_attrs(
                ptr,
                &[(key, value)],
                &[crate::object::key_attrs::attr_bits_to_entry(attrs)],
            ) {
                return;
            }
        }
    }
    let entry = crate::object::key_attrs::attr_bits_to_entry(attrs);
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let present = bag_has_own(ptr, key);
    if !present
        && entry != 0
        && crate::object::shapes::object_shape_descriptor(bag).is_some_and(|d| d.hole_count == 0)
    {
        // A fresh key on a tombstone-free bag: claimed with its entry, as an
        // ordinary object's builtin define claims it.
        crate::object::object_ops::ensure_key_in_keys_array_with_entry(bag, key_hdr, entry);
        crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
        // The bag's descriptor bit, without flipping any process gate.
        crate::object::descriptor_state::note_descriptor_target_edits(bag as usize, &[]);
    } else {
        if present {
            crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
        } else {
            // New (possibly after a delete): appended, so it enumerates last.
            object_own_set_bytes(bag, key, value);
        }
        if crate::object::key_attrs::object_key_entry(bag, key) != entry {
            crate::object::descriptor_state::note_descriptor_target_edits(
                bag as usize,
                &[crate::object::key_attrs::AttrsEdit::Data(key, attrs)],
            );
        }
    }
    declared_value_replaced_bytes(ptr, key, declared);
}

/// Remove the function's own data property `key`; true when it existed.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_remove(ptr: usize, key: &str) -> bool {
    bag_remove_bytes(ptr, key.as_bytes())
}

pub(crate) unsafe fn bag_remove_bytes(ptr: usize, key: &[u8]) -> bool {
    let bag = bag_of(ptr);
    if bag.is_null() || !bag_has_own(ptr, key) {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_delete_field(bag, key_hdr);
    // Deleting the declaration must not let a re-add reach the shape that
    // proved it (a removed last key re-added lands on the same key list).
    declared_value_replaced_bytes(ptr, key, declared);
    true
}

/// Does the function own `key` — a data OR an accessor property?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_has_own(ptr: usize, key: &[u8]) -> bool {
    if key.len() == 9 && prototype_slot_get(ptr, key).is_some() {
        return true;
    }
    let bag = bag_of(ptr);
    if bag.is_null() {
        return is_declared_static(ptr, key);
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return false;
    }
    let Some(slot) = crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key)
    else {
        return false;
    };
    crate::object::key_attrs::key_is_accessor_at(arr, slot as u32)
        || crate::object::object_key_field_at_with_keys(
            bag,
            arr,
            slot,
            crate::object::object_live_slot_count(bag),
        )
        .bits()
            != crate::value::TAG_HOLE
}

/// The function's own ACCESSOR property names, in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_accessor_names(ptr: usize) -> Vec<String> {
    bag_accessor_names_bytes(ptr)
        .into_iter()
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect()
}

pub(crate) unsafe fn bag_accessor_names_bytes(ptr: usize) -> Vec<Vec<u8>> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..keys.count() {
        if !crate::object::key_attrs::key_is_accessor_at(arr, i) {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
                continue;
            }
            out.push(bytes.to_vec());
        }
    }
    out
}

/// Every own property name (data AND accessor) in creation order — the
/// order of the bag's key list. A deleted key is gone from that order; a
/// key defined again after a delete is a new key and comes last.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_own_key_names(ptr: usize) -> Vec<String> {
    bag_own_key_names_bytes(ptr)
        .into_iter()
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect()
}

pub(crate) unsafe fn bag_own_key_names_bytes(ptr: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let bag = bag_of(ptr);
    if bag.is_null() {
        // An unborn lazy constructor's declared statics follow its intrinsic
        // slots; listing them births nothing.
        for key in crate::object::unborn_declared_statics_of(ptr) {
            out.push(key.as_bytes().to_vec());
        }
        return out;
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return out;
    }
    let live = crate::object::object_live_slot_count(bag);
    for i in 0..keys.count() {
        if !crate::object::key_attrs::key_is_accessor_at(arr, i)
            && crate::object::object_key_field_at_with_keys(bag, arr, i, live).bits()
                == crate::value::TAG_HOLE
        {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
                continue;
            }
            out.push(bytes.to_vec());
        }
    }
    out
}

/// Every own data property in ECMA-262 own-key order: integer indices
/// ascending, then other strings in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_snapshot(ptr: usize) -> Vec<(String, f64)> {
    let mut bag = bag_of(ptr);
    if bag.is_null() {
        // The snapshot reads values: an unborn lazy constructor with declared
        // statics is born to read them.
        bag = unborn_bag_for_read(ptr, None);
        if bag.is_null() {
            return Vec::new();
        }
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let live = crate::object::object_live_slot_count(bag);
    let mut indexed: Vec<(u32, String, f64)> = Vec::new();
    let mut strings: Vec<(String, f64)> = Vec::new();
    for i in 0..keys.count() {
        let value = crate::object::object_key_field_at_with_keys(bag, arr, i, live);
        if value.bits() == crate::value::TAG_HOLE
            || crate::object::key_attrs::key_is_accessor_at(arr, i)
        {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) else {
            continue;
        };
        if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
            continue;
        }
        let name = String::from_utf8_lossy(bytes).into_owned();
        let v = f64::from_bits(value.bits());
        match crate::object::canonical_array_index(&name) {
            Some(index) => indexed.push((index, name, v)),
            None => strings.push((name, v)),
        }
    }
    crate::cold_sort::sort_by_key(&mut indexed, |(index, _, _)| *index);
    indexed
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(strings)
        .collect()
}

/// The closure's internal state record (null when none).
unsafe fn state_of(ptr: usize) -> *mut ObjectHeader {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return std::ptr::null_mut();
    }
    crate::object::cell_expando_get(bag as usize).unwrap_or(std::ptr::null_mut())
}

unsafe fn state_ensure(ptr: usize) -> Option<*mut ObjectHeader> {
    let bag = bag_ensure(ptr);
    let existing = state_of(ptr);
    if !existing.is_null() {
        return Some(existing);
    }
    // A null-prototype record (its `"p"` key is not a JS property, and no
    // inherited setter may run on it), hung off the bag's `meta.expando` —
    // an ordinary traced edge of the bag.
    let meta = crate::object::object_meta_ensure(
        crate::meta_obj_runtime_born!(bag),
        crate::object::MetaReason::ClosureProps,
    );
    if meta.is_null() {
        return None;
    }
    let state = crate::object::js_object_alloc_null_proto(0, 0);
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
    let boxed = crate::value::js_nanbox_pointer(state as i64).to_bits();
    // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object barrier.
    (*meta).expando = boxed;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).expando as *const _ as usize,
        boxed,
    );
    Some(state)
}

/// Is the #3655 deleted marker for `key` set on the closure at `ptr`?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_is_deleted(ptr: usize, key: &str) -> bool {
    state_is_deleted_bytes(ptr, key.as_bytes())
}

pub(crate) unsafe fn state_is_deleted_bytes(ptr: usize, key: &[u8]) -> bool {
    let state = state_of(ptr);
    if state.is_null() {
        return false;
    }
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    object_own_get(state, &marker).is_some()
}

/// Was the info-derived own key `key` (`length`, `name`) of the closure at
/// `ptr` deleted and defined again ([`TOMBSTONE_PREFIX`])? Its bag entry is
/// then a later key, not a birth key.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_is_tombstoned_bytes(ptr: usize, key: &[u8]) -> bool {
    let state = state_of(ptr);
    if state.is_null() {
        return false;
    }
    let mut marker = Vec::with_capacity(TOMBSTONE_PREFIX.len() + key.len());
    marker.extend_from_slice(TOMBSTONE_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    object_own_get(state, &marker).is_some()
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_mark_deleted(ptr: usize, key: &str) {
    state_mark_deleted_bytes(ptr, key.as_bytes());
}

pub(crate) unsafe fn state_mark_deleted_bytes(ptr: usize, key: &[u8]) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    object_own_set_bytes(state, &marker, f64::from_bits(crate::value::TAG_TRUE));
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_clear_deleted(ptr: usize, key: &str) {
    state_clear_deleted_bytes(ptr, key.as_bytes());
}

pub(crate) unsafe fn state_clear_deleted_bytes(ptr: usize, key: &[u8]) {
    if !state_is_deleted_bytes(ptr, key) {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    if matches!(key, b"length" | b"name") {
        // The deleted info-derived entry leaves its tombstone: the key now
        // being defined again is a later key.
        let mut tomb = Vec::with_capacity(TOMBSTONE_PREFIX.len() + key.len());
        tomb.extend_from_slice(TOMBSTONE_PREFIX.as_bytes());
        tomb.extend_from_slice(key);
        object_own_set_bytes(state, &tomb, f64::from_bits(crate::value::TAG_TRUE));
    }
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
}

/// The recorded `[[Prototype]]` bits, if any.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_prototype(ptr: usize) -> Option<u64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    object_own_get(state, STATE_PROTO.as_bytes()).map(f64::to_bits)
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_set_prototype(ptr: usize, proto_bits: u64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, STATE_PROTO, f64::from_bits(proto_bits));
}

/// A runtime-internal own slot of the function object — never a JS property
/// (class private statics, computed-key records, class captures): kept in the
/// state record under `"i:" + key`, so no reflection or enumeration of the
/// function can reach it.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_get(ptr: usize, key: &str) -> Option<f64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    let mut marker = String::with_capacity(INTERNAL_PREFIX.len() + key.len());
    marker.push_str(INTERNAL_PREFIX);
    marker.push_str(key);
    object_own_get(state, marker.as_bytes())
}

/// [`state_internal_get`] of the key `head + tail`, spelled without a heap
/// allocation for short keys.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_get_joined(
    ptr: usize,
    head: &[u8],
    tail: &[u8],
) -> Option<f64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    let len = INTERNAL_PREFIX.len() + head.len() + tail.len();
    let mut stack = [0u8; 96];
    let mut heap = Vec::new();
    let buf: &mut [u8] = if len <= stack.len() {
        &mut stack[..len]
    } else {
        heap.resize(len, 0);
        &mut heap[..]
    };
    let (prefix, rest) = buf.split_at_mut(INTERNAL_PREFIX.len());
    prefix.copy_from_slice(INTERNAL_PREFIX.as_bytes());
    let (h, t) = rest.split_at_mut(head.len());
    h.copy_from_slice(head);
    t.copy_from_slice(tail);
    object_own_get(state, buf)
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_set(ptr: usize, key: &str, value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, &format!("{INTERNAL_PREFIX}{key}"), value);
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_remove(ptr: usize, key: &str) -> bool {
    if state_internal_get(ptr, key).is_none() {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    let marker = format!("{INTERNAL_PREFIX}{key}");
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
    true
}

/// The number of keys of the closure's internal state record (tests).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cfg(test)]
pub(crate) unsafe fn state_key_count(ptr: usize) -> usize {
    let state = state_of(ptr);
    if state.is_null() {
        return 0;
    }
    crate::object::object_keys(state).count() as usize
}

/// True when the closure carries internal state a base/keyed Function shape
/// cannot describe (a deleted marker or a recorded prototype).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn has_state(ptr: usize) -> bool {
    !state_of(ptr).is_null()
}

// ---- The props API surface for callers outside this file (F2y). Every reader
// of a function's own-key store goes through these, so the store's
// representation is named here and in the GC trace arm only.

/// Whether the function has materialized its own-key store.
#[inline(always)]
pub(crate) unsafe fn has_store(ptr: usize) -> bool {
    !bag_of(ptr).is_null()
}

/// A newborn function: no own-key store yet.
#[inline(always)]
pub(crate) unsafe fn clear_store(closure: *mut ClosureHeader) {
    (*closure).props = std::ptr::null_mut();
}

/// The layout word of the own-key store (`None` without one).
#[inline(always)]
pub(crate) unsafe fn store_stamp_word(ptr: usize) -> Option<u32> {
    let bag = bag_of(ptr);
    (!bag.is_null()).then(|| crate::object::shapes::object_shape_stamp(bag).0)
}

/// The key-attribute entry of `key` in the own-key store (`None` without one).
#[inline(always)]
pub(crate) unsafe fn store_key_entry(ptr: usize, key: &[u8]) -> Option<u8> {
    let bag = bag_of(ptr);
    (!bag.is_null()).then(|| crate::object::key_attrs::object_key_entry(bag, key))
}

/// The attributes the own-key store records for `key`.
#[inline(always)]
pub(crate) unsafe fn store_attrs(ptr: usize, key: &[u8]) -> Option<crate::object::PropertyAttrs> {
    crate::object::descriptor_state::get_property_attrs_bytes(bag_of(ptr) as usize, key)
}

/// Clears the attributes the own-key store records for `key`.
#[inline(always)]
pub(crate) unsafe fn store_clear_attrs(ptr: usize, key: &str) {
    let bag = bag_of(ptr);
    if !bag.is_null() {
        crate::object::clear_property_attrs(bag as usize, key);
    }
}

/// Moves the own-key store's layout to its semantics-changed state.
#[inline(always)]
pub(crate) unsafe fn store_transition_semantics(ptr: usize) {
    let _ = crate::object::shapes::transition_object_shape_semantics(bag_of(ptr));
}

/// A legacy `[[Prototype]]` recorded on the own-key store's meta record.
#[inline(always)]
pub(crate) unsafe fn store_meta_prototype(ptr: usize) -> Option<u64> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
    (!meta.is_null() && (*meta).prototype != 0).then(|| (*meta).prototype)
}

// ---- S2a seam (FR12): operations on a function's own-key store, addressed by the
// closure. Callers outside this file name the operation, not the bag; the
// representation swap (S2c) changes these bodies only.

/// `builtin_proto::note_write` for the function at `ptr` (a no-op without a store).
#[inline(always)]
pub(crate) unsafe fn note_write(
    ptr: usize,
    mutation: crate::object::builtin_proto::Mutation<'_>,
    site: &'static str,
) {
    crate::object::builtin_proto::note_write(bag_of(ptr), mutation, site)
}

/// Is a write of `key` to the function at `ptr` refused by a builtin seal?
#[inline(always)]
pub(crate) unsafe fn sealed_write_refused(ptr: usize, key: f64) -> bool {
    crate::object::builtin_proto::sealed_write_refused(bag_of(ptr), key)
}

/// The function's own keys are proven to hold no symbol (no store proves it).
#[inline(always)]
pub(crate) unsafe fn proves_no_symbols(ptr: usize) -> bool {
    let bag = bag_of(ptr);
    bag.is_null() || crate::object::overflow::home::ObjHome::proves_no_symbols(bag as usize)
}

/// Does the function at `ptr` own symbol key `symbol`?
#[inline(always)]
pub(crate) unsafe fn owns_symbol(ptr: usize, symbol: usize) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null()
        && crate::object::overflow::home::ObjHome::own_symbol_position(bag as usize, symbol)
            .is_some()
}

/// The shape descriptor of the function's own-key store.
#[cfg(test)]
#[inline(always)]
pub(crate) unsafe fn shape_descriptor(
    ptr: usize,
) -> Option<crate::object::shapes::ShapeDescriptor> {
    crate::object::shapes::object_shape_descriptor(bag_of(ptr))
}

/// [`shape_descriptor`]'s record, borrowed in place.
#[inline(always)]
pub(crate) unsafe fn shape_record(ptr: usize) -> Option<crate::object::shapes::ShapeRecordRef> {
    crate::object::shapes::object_shape_record(bag_of(ptr))
}

/// `[[Set]]` of `name` when it is an own key of the function: `None` when the
/// function has no store or no such own key (the store is an add), else
/// whether the key was a writable data key and took `value`. `Some(false)`
/// (a non-writable or accessor own key) leaves the store to the full
/// `[[Set]]`; it is never an add.
#[inline(always)]
pub(crate) unsafe fn overwrite_own_data(
    ptr: usize,
    key: f64,
    name: &[u8],
    value: f64,
) -> Option<bool> {
    let bag = bag_of(ptr);
    if bag.is_null() || !bag_has_own(ptr, name) {
        return None;
    }
    Some(crate::object::try_existing_own_data_overwrite_by_content(
        bag, key, name, value,
    ))
}

/// Give the function its own-key store when it has none (no value returned).
#[inline(always)]
pub(crate) unsafe fn ensure_key_store(ptr: usize) {
    key_holder(ptr, true);
}

/// The function's own accessor `name` with its (enumerable, configurable) flags.
pub(crate) unsafe fn own_accessor(
    ptr: usize,
    name: &[u8],
) -> Option<(crate::object::accessor_pair::Accessor, bool, bool)> {
    use crate::object::key_attrs as ka;
    let bag = key_holder(ptr, false);
    if bag.is_null() {
        return None;
    }
    let entry = ka::object_key_entry(bag, name);
    if entry & ka::ENTRY_ACCESSOR == 0 {
        return None;
    }
    let acc = crate::object::accessor_pair::own_accessor(bag as usize, name)?;
    Some((
        acc,
        entry & ka::ENTRY_NON_ENUMERABLE == 0,
        entry & ka::ENTRY_NON_CONFIGURABLE == 0,
    ))
}

/// Install builtin accessor `name` on the function (creating its store).
pub(crate) unsafe fn set_builtin_accessor_pair(
    ptr: usize,
    name: &[u8],
    acc: crate::object::accessor_pair::Accessor,
    attrs: crate::object::PropertyAttrs,
) {
    let bag = key_holder(ptr, true);
    crate::object::descriptor_state::set_builtin_accessor_pair_bytes(
        bag as usize,
        name,
        acc,
        attrs,
    );
}

/// Set builtin attributes of own key `name` on the function (creating its store).
pub(crate) unsafe fn set_builtin_property_attrs(
    ptr: usize,
    name: &[u8],
    attrs: crate::object::PropertyAttrs,
) {
    let bag = key_holder(ptr, true);
    crate::object::descriptor_state::set_builtin_property_attrs_bytes(bag as usize, name, attrs);
}

/// fr20 op 5: the meta record of the function's store for `reason`, made
/// with the store when `create`; null when the function has no store.
#[inline(always)]
pub(crate) unsafe fn meta_for(
    ptr: usize,
    reason: crate::object::MetaReason,
    create: bool,
) -> *mut crate::object::ObjectMeta {
    let bag = if create { bag_ensure(ptr) } else { bag_of(ptr) };
    if bag.is_null() {
        return std::ptr::null_mut();
    }
    crate::object::key_owner::meta_for(bag as usize, reason, create)
}

/// fr20 op 6: what the function's store says about its layout, by value.
pub(crate) enum StoreShape {
    /// The function never had an own key.
    NoStore,
    /// The store's shape descriptor (None: not described, a dictionary).
    Store(Option<crate::object::shapes::ShapeDescriptor>),
}

#[inline(always)]
pub(crate) unsafe fn store_shape(ptr: usize) -> StoreShape {
    let bag = bag_of(ptr);
    if bag.is_null() {
        StoreShape::NoStore
    } else {
        StoreShape::Store(crate::object::shapes::object_shape_descriptor(bag))
    }
}

/// fr20 op 9: the intrinsic serial a sealed native constructor's store
/// carries (None for every other function).
#[inline(always)]
pub(crate) unsafe fn sealed_serial(ptr: usize) -> Option<u64> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
    if meta.is_null() {
        return None;
    }
    Some((*meta).proto_serial)
}

/// fr20 op 9: the sealed store's live layout word is `word` (its contents
/// and link are the static layout's).
///
/// # Safety
/// `sealed_serial(ptr)` answered Some.
#[inline(always)]
pub(crate) unsafe fn sealed_intact(ptr: usize, word: u32) -> bool {
    (*bag_of(ptr)).parent_class_id == word
}

/// fr20 op 10: the constructor's own keys become the sealed static layout
/// `desc` (its prototype image), keeping its private state edge.
#[inline(always)]
pub(crate) unsafe fn seal_to_layout(
    ptr: usize,
    desc: *const crate::object::layout_desc::PerryLayoutDesc,
    serial: u64,
    module: &str,
    name: &str,
) {
    let old = bag_of(ptr);
    let built = crate::value::js_nanbox_pointer(old as i64);
    let Some(image) = crate::object::layout_desc::prototype_image(&*desc, built, serial, &[])
    else {
        eprintln!("Perry internal error: native constructor bag {module}.{name} differs from its static layout");
        std::process::abort();
    };
    let bag = crate::value::js_nanbox_get_pointer(image) as *mut crate::object::ObjectHeader;
    // The recorded constructor parent (and other private state) remains
    // an owned edge. It is never a JS property of this null-prototype bag.
    if !crate::meta_read!(crate::meta_obj_runtime_born!(old as *mut _)).is_null() {
        let state = (*crate::meta_read!(crate::meta_obj_runtime_born!(old as *mut _))).expando;
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
        // GC_STORE_AUDIT(BARRIERED): metadata-record child edge.
        (*meta).expando = state;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).expando as *const _ as usize,
            state,
        );
    }
    crate::object::builtin_proto::seal_runtime_prototype(bag, serial);
    install_bag(ptr, bag);
}

/// fr20 op 8: a class's computed static method is an own data property of
/// the function: the ordinary force-store and attribute edit on its store.
#[inline(never)]
pub(crate) unsafe fn fn_define_force_attrs(
    ptr: usize,
    key: f64,
    value: f64,
    name: &[u8],
    attrs: u8,
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let bag = bag_ensure(ptr);
    let key_header = crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
    crate::object::object_ops::define_property_force_store_value(bag, key_header, value);
    crate::object::key_attrs::apply_edits(
        bag,
        &[crate::object::key_attrs::AttrsEdit::Data(name, attrs)],
    );
}

// ---- S2b C6a: own-key read and write ops. Callers outside this file address
// the function by its closure; none receives the store object. S2c swaps the
// bodies (family record, then the Table) and nothing else.

/// The own key `key_bits` (hash `hash`) of the function at `ptr`: `Ok(value)`
/// a hit, `Err(true)` proven absent, `Err(false)` the store cannot answer here
/// (forwarded cell, shadowed store, no ordinary record, probe not ready).
/// A store record whose probe word is not derived yet is derived here.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn own_lookup(ptr: usize, key_bits: u64, hash: u64) -> Result<f64, bool> {
    let closure = ptr as *const ClosureHeader;
    let header = crate::gc::header_from_trusted_user_ptr(closure.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return Err(false);
    }
    let bag = bag_of(ptr) as *const ObjectHeader;
    if bag.is_null() {
        return crate::object::layout_probe::bagless_own(closure, key_bits, hash);
    }
    match crate::object::layout_probe::store_own_derived(bag, key_bits, hash) {
        None => Err(false),
        Some(Ok(v)) => Ok(v),
        // A function with a store keeps its `prototype` in its own slot.
        Some(Err(miss)) => {
            use crate::object::runtime_layouts::atom_hash as h;
            const PROTOTYPE: u64 = h(b"prototype");
            if hash == PROTOTYPE {
                let atom = crate::object::runtime_layouts::BP_ATOM_PROTOTYPE.string as usize as u64
                    | crate::value::STRING_TAG;
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if key_bits == atom
                    || crate::string::js_string_key_bytes(
                        crate::JSValue::from_bits(key_bits),
                        &mut scratch,
                    ) == Some(b"prototype".as_slice())
                {
                    if let Some(v) = prototype_slot_value(ptr) {
                        return Ok(v);
                    }
                }
            }
            Err(miss)
        }
    }
}

/// The store [`own_lookup`] reads, for the exotic own arm
/// (`layout_probe::bag_own`) and the exotic read (`layout_probe::exotic_get`),
/// which read it as they read an array's named bag.
///
/// # Safety
/// `ptr` is a live closure cell whose header is not forwarded.
#[inline(always)]
pub(crate) unsafe fn own_store_in(ptr: usize) -> crate::object::layout_probe::OwnStore {
    crate::object::layout_probe::OwnStore::new(bag_of(ptr) as *const ObjectHeader)
}

/// The function's own data `prototype` read through its store's shape (a
/// function's `prototype` is non-configurable, so no accessor or deletion
/// stands in front of the value). `None` when the shape does not answer.
///
/// # Safety
/// `ptr` is a live closure cell.
pub(crate) unsafe fn own_data_prototype(ptr: usize) -> Option<f64> {
    use crate::codegen_abi::LayoutWord;
    let bag = bag_of(ptr);
    let bag_header = crate::value::addr_class::try_read_gc_header(bag as usize)?;
    if crate::gc_type_is_not_object!(bag_header.obj_type)
        || bag_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    const SPILL: u32 = 1 << 31;
    let encoded = {
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
        if !meta.is_null() && (*meta).elements != 0 {
            return None;
        }
        let id = (*bag).shape_word();
        if !crate::object::shapes::is_site_matchable_layout_word(id) {
            return None;
        }
        let record = crate::object::shapes::shape_record_for_word(LayoutWord(id))?;
        let pos = record.own_data_position_of_value(0, b"prototype")??;
        pos | if pos >= record.live_inline_slot_count() {
            SPILL
        } else {
            0
        }
    };
    // The shape assigns the slot's canonical storage.
    let slot = (encoded & !SPILL) as usize;
    let bits = if encoded & SPILL == 0 {
        let fields =
            crate::meta_slots_of!(crate::meta_obj_runtime_born!(bag as *mut _)) as *const u64;
        *fields.add(slot)
    } else {
        let spill = (*crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _))).spill
            as *const crate::array::ArrayHeader;
        *crate::array::array_elements_ptr(spill).add(slot)
    };
    (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
}

/// `[[Set]]`/DefineOwnProperty of the non-intrinsic string key `key`
/// (`key_value` its value form) on the function at `ptr`, whose admission
/// tests the caller ran (`function_own_put`): the first key gives birth to the
/// store, an existing data key is overwritten, a new key is added on the
/// store's chain, and the closure's ShapeId follows the add. `link` is the
/// function's [[Prototype]] word for `[[Set]]` (`define` false).
///
/// # Safety
/// `ptr` is a live closure cell; no collection runs (the caller suppresses it).
#[inline(always)]
pub(crate) unsafe fn put_own(
    ptr: usize,
    key: *const crate::StringHeader,
    key_value: f64,
    value: f64,
    link: u64,
    define: bool,
) -> Option<f64> {
    use crate::object::function_own_put::closure_shape_key_added;
    use crate::object::layout_probe::FunctionPut;
    let c = ptr as *mut ClosureHeader;
    let bag = bag_of(ptr);
    if bag.is_null() {
        // The first own key: the bag is born holding the intrinsic slots and
        // this key in one shape (`props::bag_born_with_attrs`).
        if !define && !crate::object::layout_probe::function_link_admits_add(link, key) {
            return None;
        }
        let name = crate::string::header_str_checked(key)?;
        let prev = (*c).shape_id;
        if !bag_born_with_room(ptr, &[(name, value)], &[]) {
            return None;
        }
        // The born bag lists the intrinsic slots and `key`: the closure's
        // next shape is `prev`'s key-add edge for `key`, as for any add.
        closure_shape_key_added(c, prev, key);
        return Some(value);
    }
    match crate::object::layout_probe::function_put_plan(bag, link, key, define) {
        FunctionPut::Decline => None,
        FunctionPut::Overwrite => {
            let bytes = std::slice::from_raw_parts(
                crate::string::string_data(key),
                (*key).byte_len as usize,
            );
            crate::object::try_existing_own_data_overwrite_by_content(bag, key_value, bytes, value)
                .then_some(value)
        }
        FunctionPut::Add => {
            let prev = (*c).shape_id;
            if !crate::object::leaf_key_add(bag, key, value.to_bits()) {
                if crate::object::shapes::object_shape_record(bag)?.object_kind()
                    == crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
                {
                    crate::object::alloc_plain::mark_object_plain_ordinary(bag);
                }
                let mut refresh = None;
                crate::object::object_set_field_by_name_transition_chain_proven_value(
                    bag,
                    key,
                    value,
                    &mut refresh,
                )?;
            }
            closure_shape_key_added(c, prev, key);
            Some(value)
        }
    }
}

/// `[[Set]]` fast store of an existing own writable data key of the function
/// at `ptr` (`shape_store::try_own_store`); false when it has no store or the
/// store's shape does not answer.
///
/// # Safety
/// `ptr` is a live, unforwarded closure cell.
#[inline(always)]
pub(crate) unsafe fn put_own_overwrite(
    ptr: usize,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && crate::object::shape_store::try_own_store(bag, key, value)
}

/// DefineOwnProperty of the chain-proven new data key `key_root` on the
/// function `owner` (`shape_store::try_function_store`): the store takes the
/// key on its chain (or by the forced store), the key's deletion mark clears
/// and the closure's ShapeId follows. The handles are the caller's, rooted in
/// its `scope`; the store's root joins that scope.
///
/// # Safety
/// `owner` holds a live closure; collection may run.
#[inline(always)]
pub(crate) unsafe fn put_own_define_proven(
    scope: &crate::gc::RuntimeHandleScope,
    owner: &crate::gc::RuntimeHandle<'_>,
    key_root: &crate::gc::RuntimeHandle<'_>,
    value_root: &crate::gc::RuntimeHandle<'_>,
) -> Option<f64> {
    use crate::object::shapes;
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let bag = bag_ensure(ptr);
    let bag_root = scope.root_raw_mut_ptr(bag);
    if shapes::object_shape_record(bag)?.kind_is(shapes::ShapeObjectKind::OrdinaryUnmarked) {
        crate::object::alloc_plain::mark_object_plain_ordinary(bag);
    }
    let key_ptr = key_root.get_raw_const_ptr::<crate::StringHeader>();
    let mut refresh = None;
    if crate::object::object_set_field_by_name_transition_chain_proven_value(
        bag,
        key_ptr,
        value_root.get_nanbox_f64(),
        &mut refresh,
    )
    .is_none()
    {
        crate::object::object_ops::define_property_force_store_value(
            bag_root.get_raw_mut_ptr::<ObjectHeader>(),
            key_root.get_raw_const_ptr::<crate::StringHeader>(),
            value_root.get_nanbox_f64(),
        );
    }
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    // The ordinary function writer also revives a deleted configurable key.
    // The bag's new data binding and the function's deletion state must agree.
    let name =
        crate::string::header_str_checked(key_root.get_raw_const_ptr::<crate::StringHeader>())?;
    state_clear_deleted(ptr, name);
    crate::closure::shape::refresh_closure_shape(ptr);
    Some(value_root.get_nanbox_f64())
}

// ---- S2b C6b: descriptor key leaves for `KeyTarget::Function`. Each is one
// call on the function's store; S2c swaps the bodies and nothing else.
use crate::object::accessor_pair::Accessor as FnAccessor;
use crate::object::descriptor_state::AttrsEdit as FnAttrsEdit;
use crate::object::overflow::home::ObjHome as FnObjHome;

/// The attribute entry of own key `key` (0 when absent or default).
#[inline(never)]
pub(crate) unsafe fn fn_key_entry(ptr: usize, key: &[u8]) -> u8 {
    let bag = bag_of(ptr);
    if bag.is_null() {
        0
    } else {
        crate::object::key_attrs::object_key_entry(bag, key)
    }
}
#[inline(never)]
pub(crate) unsafe fn fn_key_is_accessor(ptr: usize, key: &[u8]) -> bool {
    let bag = bag_of(ptr);
    !bag.is_null() && crate::object::key_attrs::object_key_is_accessor(bag, key)
}
#[inline(never)]
pub(crate) unsafe fn fn_summary(ptr: usize) -> u8 {
    let bag = bag_of(ptr);
    if bag.is_null() {
        0
    } else {
        crate::object::key_attrs::object_summary(bag)
    }
}
#[inline(never)]
pub(crate) unsafe fn fn_accessor_names(ptr: usize) -> Vec<String> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        Vec::new()
    } else {
        crate::object::key_attrs::object_accessor_key_names(bag)
    }
}
/// `Some(entry)` when `key` is an own key (entry 0: default attributes).
#[inline(never)]
pub(crate) unsafe fn fn_own_key_entry(ptr: usize, key: &[u8]) -> Option<u8> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    let entry = crate::object::key_attrs::object_key_entry(bag, key);
    let keys = crate::object::object_keys(bag);
    (entry != 0
        || crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key).is_some())
    .then_some(entry)
}
#[inline(never)]
pub(crate) unsafe fn fn_own_accessor_entry(ptr: usize, key: &[u8]) -> Option<FnAccessor> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        None
    } else {
        crate::object::accessor_pair::own_accessor(bag as usize, key)
    }
}
#[inline(never)]
pub(crate) unsafe fn fn_note_edits(ptr: usize, edits: &[FnAttrsEdit<'_>]) {
    crate::object::descriptor_state::note_descriptor_target_edits(bag_of(ptr) as usize, edits)
}
#[inline(never)]
pub(crate) unsafe fn fn_accessor_replaced(
    ptr: usize,
    key: &[u8],
    previous: Option<FnAccessor>,
    now: FnAccessor,
) {
    crate::object::descriptor_state::note_accessor_function_replaced_bytes(
        bag_of(ptr) as usize,
        key,
        previous,
        now,
    )
}
#[inline(never)]
pub(crate) unsafe fn fn_note_named_mutation(ptr: usize, key: &[u8]) {
    crate::typedarray_named::note_named_mutation(bag_of(ptr) as usize, key)
}
#[inline(never)]
pub(crate) unsafe fn fn_note_descriptors(ptr: usize, edits: &[FnAttrsEdit<'_>]) {
    FnObjHome::note_descriptors(bag_of(ptr) as usize, edits)
}
#[inline(never)]
pub(crate) unsafe fn fn_own_accessor_pair(ptr: usize, key: &[u8]) -> Option<FnAccessor> {
    FnObjHome::own_accessor_pair(bag_of(ptr) as usize, key)
}
#[inline(never)]
pub(crate) unsafe fn fn_store_accessor_pair(ptr: usize, key: &[u8], pair: Option<FnAccessor>) {
    FnObjHome::store_accessor_pair(bag_of(ptr) as usize, key, pair)
}
#[inline(never)]
pub(crate) unsafe fn fn_attrs_live_in_keys(ptr: usize) -> bool {
    FnObjHome::attrs_live_in_keys(bag_of(ptr) as usize)
}
#[cfg(test)]
#[inline(never)]
pub(crate) unsafe fn fn_clear_descriptors(ptr: usize) {
    let bag = bag_of(ptr);
    if !bag.is_null() {
        crate::object::descriptor_state::clear_object_descriptors(bag as usize)
    }
}

// ---- S2b C6c: private elements. A function's private elements live in its
// own-key store; callers outside this file address the function by its
// closure, and each leaf re-enters the ordinary holder's body on the store.
use crate::object::field_get_set::ic_miss::{PrivatePlace, PrivateStorageKey};

/// `KeyTarget::meta` for a function: its store's meta record.
#[inline(never)]
pub(crate) unsafe fn fn_meta(
    ptr: usize,
    reason: crate::object::MetaReason,
    create: bool,
) -> *mut crate::object::ObjectMeta {
    crate::object::descriptor_state::KeyTarget::object_meta(bag_of(ptr), reason, create)
}

/// `KeyTarget::restrict` for a function: the level applied to its store.
#[inline(never)]
pub(crate) unsafe fn fn_restrict(ptr: usize, level: crate::object::descriptor_state::Restrict) {
    crate::object::descriptor_state::KeyTarget::restrict_object(bag_of(ptr), level)
}

/// `KeyTarget::satisfies` for a function: its store's integrity level.
#[inline(never)]
pub(crate) unsafe fn fn_satisfies(ptr: usize, frozen: bool) -> bool {
    crate::object::descriptor_state::KeyTarget::object_satisfies(bag_of(ptr), frozen)
}

/// Does the function have an own-key store (where its private elements live)?
#[inline(always)]
pub(crate) unsafe fn fn_has_key_store(ptr: usize) -> bool {
    !bag_of(ptr).is_null()
}
#[inline(never)]
pub(crate) unsafe fn fn_private_has_brand(ptr: usize, brand: u64) -> bool {
    crate::object::shapes::object_has_brand(bag_of(ptr), brand)
}
#[inline(never)]
pub(crate) unsafe fn fn_private_add_brand(ptr: usize, brand: u64) -> bool {
    crate::object::shapes::transition_object_shape_add_brand(bag_of(ptr), brand)
}
#[inline(never)]
pub(crate) unsafe fn fn_private_set(
    ptr: usize,
    storage: &PrivateStorageKey,
    present: bool,
    obj: f64,
    value: f64,
) -> bool {
    crate::object::field_get_set::ic_miss::private_element_set_on(
        bag_of(ptr),
        storage,
        present,
        obj,
        value,
    )
}
/// The place of private field `storage`'s value in the function's store (the
/// private-storage lookup answered here: the place, never the store).
#[inline(never)]
pub(crate) unsafe fn fn_private_lookup(
    ptr: usize,
    storage: &PrivateStorageKey,
) -> Option<PrivatePlace> {
    storage.locate_on(bag_of(ptr))
}
#[inline(never)]
pub(crate) unsafe fn fn_private_read(ptr: usize, place: PrivatePlace) -> u64 {
    place.read(bag_of(ptr))
}
#[inline(never)]
pub(crate) unsafe fn fn_private_store(ptr: usize, place: PrivatePlace, value: f64) -> bool {
    place.store_on(bag_of(ptr), value)
}
#[inline(never)]
pub(crate) unsafe fn fn_private_fill_optional(
    ptr: usize,
    storage: &PrivateStorageKey,
    value: f64,
) -> bool {
    storage.fill_optional_on(bag_of(ptr), value)
}
#[inline(never)]
pub(crate) unsafe fn fn_private_has_entry(ptr: usize, key: &[u8]) -> bool {
    crate::object::key_attrs::object_key_has_private_entry(bag_of(ptr), key)
}
#[inline(never)]
pub(crate) unsafe fn fn_private_define(ptr: usize, key: &[u8]) {
    crate::object::key_attrs::apply_edits(
        bag_of(ptr),
        &[crate::object::key_attrs::AttrsEdit::Private(key)],
    );
}
/// A runtime intrinsic's private element on the function (`None` when the
/// function has no store or no such element).
#[inline]
pub(crate) unsafe fn fn_private_intrinsic_read(
    ptr: usize,
    site: &crate::object::IntrinsicPrivateReadSite,
) -> Option<f64> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    site.read(crate::value::js_nanbox_pointer(bag as i64))
}
