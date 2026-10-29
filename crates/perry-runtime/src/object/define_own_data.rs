//! `CreateDataPropertyOrThrow(O, key, value)` answered from the receiver
//! shape: the definition an object literal performs for each of its own
//! properties and a class performs for each public field.
//!
//! A definition never consults the prototype chain. On an ordinary,
//! extensible object it either creates `key` as a data property whose
//! attributes are all true, or, when `key` is already an own data property
//! with exactly those attributes, replaces its value. Both are facts of the
//! receiver ShapeId:
//!
//! 1. **Creation: the key-add edge.** The transition lattice holds an edge
//!    `(ShapeId, key) -> target` only for a shape that lacks `key`, so a hit
//!    is the creation, at the slot the target shape places the key. The edge
//!    is found by the ShapeId and the key's interned word, never by name.
//! 2. **Replacement: the own slot.** A key the shape already holds is
//!    overwritten in place when no own descriptor covers it.
//!
//! Anything else (no edge yet, an own accessor or non-default attributes for
//! the key, a non-extensible, exotic or dictionary receiver) is `None`, and
//! the caller runs the general definition, which also teaches the lattice the
//! edge for the next object of this shape.
//!
//! Nothing here runs user code. The only allocation is the store itself
//! (spill growth), and the stored value is handed back through the store's
//! own root.

use super::ObjectHeader;

/// Define `key = value` on `obj_value` from its shape (module docs), or
/// `None` when the shape does not answer; nothing was written then.
///
/// # Safety
/// `obj_value`, `key_value` and `value` are live values.
#[inline(never)]
pub(crate) unsafe fn define_own_data_from_shape(
    obj_value: f64,
    key_value: f64,
    value: f64,
) -> Option<f64> {
    let bits = obj_value.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    // Replacement is already answered by an ordinary all-default shape.
    // Reuse the named store's own-slot proof before probing a key-ADD edge:
    // the latter cannot answer an overwrite and was doing its full key
    // interning, transition and receiver checks first on every literal tail.
    let id = (*obj).shape_word();
    // Declared class fields commonly CREATE; retain their key-add-first
    // admission instead of scanning all preceding own keys at each birth.
    let record = if (*obj).class_id == 0
        && id.wrapping_sub(super::shapes::SHAPE_ID_BASE)
            < (2 << crate::codegen_abi::LAYOUT_BAND_SHIFT)
    {
        // `obj`'s own header word: the receiver carries its row.
        super::shapes::shape_record_of_receiver(id)
    } else {
        None
    };
    if let Some(record) = record {
        if record.object_kind() == super::shapes::ShapeObjectKind::Ordinary
            && record.summary() == 0
            && record.proto_id() == super::shapes::PROTO_ID_DEFAULT
            && record.logical_key_count() != 0
        {
            if let Some(slot) = record.plain_own_identity_slot(key_value.to_bits()) {
                if super::shape_store::store_proven_own_slot(obj, record, slot as usize, value) {
                    return Some(value);
                }
            }
        }
    }
    define_other(obj_value, key_value, value, record)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn define_other(
    obj_value: f64,
    key_value: f64,
    value: f64,
    record: Option<super::shapes::ShapeRecordRef>,
) -> Option<f64> {
    let addr = (obj_value.to_bits() & crate::value::POINTER_MASK) as usize;
    let obj = addr as *mut ObjectHeader;
    if let Some(record) = record {
        if record.object_kind() == super::shapes::ShapeObjectKind::Ordinary
            && record.summary() == 0
            && record.proto_id() == super::shapes::PROTO_ID_DEFAULT
            && record.logical_key_count() != 0
        {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            if let Some(bytes) = crate::string::js_string_key_bytes(
                crate::JSValue::from_bits(key_value.to_bits()),
                &mut scratch,
            ) {
                if let Some(Some(slot)) = record.own_data_slot_of_value(key_value.to_bits(), bytes)
                {
                    if super::shape_store::store_proven_own_slot(obj, record, slot as usize, value)
                    {
                        return Some(value);
                    }
                }
            }
        }
    }
    // The word the lattice is keyed on: the interned string, found without
    // allocating (a pooled literal or an SSO immediate resolves to its
    // interned twin). Private names and canonical indices are refused there:
    // they are not ordinary named properties.
    let Some(key) = super::store_key::interned_key_for_store(key_value) else {
        // The interned-key store funnels validate this same header themselves.
        // Content lookup reads the shape directly, so validate only this lane.
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
        if !crate::gc_type_is_object!(header.obj_type)
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & BLOCKING != 0
        {
            return None;
        }
        return define_listed_key_by_content(obj, key_value, value);
    };
    // 1. Creation. The chain-proven form is exactly the definition's append:
    // it vets the receiver kind, its flags and an own descriptor for the key,
    // and skips only the prototype-chain question a definition never asks.
    let mut refreshed: Option<(f64, f64, f64)> = None;
    if let Some(stored) = super::object_set_field_by_name_transition_chain_proven_value(
        obj,
        key,
        value,
        &mut refreshed,
    ) {
        return Some(stored);
    }
    // The key is interned, so the miss allocated nothing; the helper's
    // re-rooted copies are used whenever it hands them back all the same.
    let (obj_value, value) = match refreshed {
        Some((o, _, v)) => (o, v),
        None => (obj_value, value),
    };
    let obj = (obj_value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    // 2. Replacement of an own data property with default attributes.
    if super::try_existing_own_data_overwrite(obj, key, value) {
        return Some(value);
    }
    None
}

/// Step 2 for a key whose content was never interned. No key-add edge names
/// such a key (the lattice is keyed by interned words, and the first append
/// of a key interns it), but the receiver's key list may already hold it: a
/// list holds its keys by content, and a class's declared fields are listed
/// under the module's literal strings, which are not interned. So the key is
/// found the way the list is keyed, by its bytes, and a default-attribute
/// data property is overwritten in place. `None` for anything else.
///
/// # Safety
/// `obj` is a live heap object; `key_value` and `value` are live values.
unsafe fn define_listed_key_by_content(
    obj: *mut ObjectHeader,
    key_value: f64,
    value: f64,
) -> Option<f64> {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key_value.to_bits()),
        &mut sso,
    )?;
    // Refused for the reason `interned_key_for_store` refuses them.
    match bytes.first() {
        Some(&first) if first != b'#' && !first.is_ascii_digit() => {}
        _ => return None,
    }
    // A key list that carries attributes must give this key none.
    if super::key_attrs::object_key_entry(obj, bytes) != 0 {
        return None;
    }
    super::try_existing_own_data_overwrite_by_content(obj, key_value, bytes, value).then_some(value)
}

/// `[[DefineOwnProperty]]` of a key the receiver does not have, on an
/// ordinary extensible object (`ValidateAndApplyPropertyDescriptor` with an
/// undefined current property): the key arrives WITH its attributes, one
/// key-list edge (`key_attrs::apply_edits` claims an absent key with its
/// entry), and its value or accessor pair is then stored in its slot. This is
/// exactly what the general definition does for a new key, without its
/// redefinition machinery (the existing-property reads, the retained
/// attributes, a second key-list rebuild for the attributes): zod v4 defines
/// `_zod` non-enumerable and a lazy accessor per schema property.
///
/// `false`, having changed nothing, for anything else: the key is present,
/// the receiver is not a plain ordinary extensible object (exotic, typed
/// array, arguments, prototype, native module), or the key is not an interned
/// ordinary name. An authoritative absent overflow entry, or an absent family
/// key before its first overflow, uses the same final-data insertion. Accessors
/// in those cases retain the general definition.
///
/// # Safety
/// `receiver`, `key` and `descriptor` are live rooted values.
pub(crate) unsafe fn define_fresh_own_from_shape(
    scope: &crate::gc::RuntimeHandleScope,
    receiver: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    use crate::object::object_ops::{DESC_GET, DESC_SET, DESC_VALUE};
    let key_ptr = match definition_admitted(receiver.get_nanbox_u64(), key.get_nanbox_f64()) {
        Some(Admitted::Fresh(key_ptr)) => key_ptr,
        Some(Admitted::Overflow(key_ptr)) => {
            return define_fresh_overflow_data(receiver, key_ptr, descriptor)
        }
        Some(Admitted::Own(pos)) => {
            return redefine_unchanged_own_data(receiver.get_nanbox_u64(), pos, descriptor)
        }
        None => return false,
    };
    let key_bytes = || {
        std::slice::from_raw_parts(
            crate::string::string_data(key_ptr),
            (*key_ptr).byte_len as usize,
        )
    };
    let enumerable = descriptor.flag(b"enumerable").unwrap_or(false);
    let configurable = descriptor.flag(b"configurable").unwrap_or(false);
    let (has_get, has_set) = (descriptor.has(DESC_GET), descriptor.has(DESC_SET));
    if !has_get && !has_set {
        // A data or generic descriptor: an omitted `value` is `undefined`,
        // an omitted attribute `false` (a new property).
        let attrs = super::PropertyAttrs::new(
            descriptor.flag(b"writable").unwrap_or(false),
            enumerable,
            configurable,
        );
        let addr = (receiver.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
        super::builtin_proto::note_write_addr(addr, "descriptor");
        crate::typedarray_named::note_named_mutation(addr, key_bytes());
        super::prop_plan::prop_plan_epoch_bump_for_owner(addr);
        // Claims the absent key with its entry and marks the receiver.
        super::descriptor_state::note_descriptor_target_edits(
            addr,
            &[super::key_attrs::AttrsEdit::Data(key_bytes(), attrs.bits)],
        );
        // The claim allocated in a no-move window: `addr` is current. The
        // store writes the now-own slot directly, whatever its attributes.
        super::object_ops::define_property_force_store_value(
            addr as *mut ObjectHeader,
            key_ptr,
            f64::from_bits(descriptor.read(DESC_VALUE).bits()),
        );
        return true;
    }
    // An accessor: each present function half is bound to the receiver as
    // the general definition binds it (#450); an omitted or undefined half
    // is absent. The clones may collect: re-read the receiver after them.
    let half = |present: bool, field: usize| -> u64 {
        let bits = descriptor.read(field).bits();
        if !present || bits == crate::value::TAG_UNDEFINED {
            return 0;
        }
        let recv = f64::from_bits(receiver.get_nanbox_u64());
        crate::closure::clone_closure_rebind_this(bits, recv)
    };
    let get = scope.root_nanbox_u64(half(has_get, DESC_GET));
    let set = scope.root_nanbox_u64(half(has_set, DESC_SET));
    let addr = (receiver.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
    let name = String::from_utf8_lossy(key_bytes()).into_owned();
    super::builtin_proto::note_write_addr(addr, "descriptor");
    crate::typedarray_named::note_named_mutation(addr, name.as_bytes());
    // A new accessor keeps its internal writable bit (as the general
    // definition's `writable = has_accessor` does).
    super::descriptor_state::install_fresh_accessor_property(
        addr,
        name,
        super::AccessorDescriptor {
            get: get.get_nanbox_u64(),
            set: set.get_nanbox_u64(),
        },
        super::PropertyAttrs::new(true, enumerable, configurable),
    );
    true
}

/// What [`definition_admitted`] found for the key.
enum Admitted {
    /// The receiver does not have the key (its interned string).
    Fresh(*const crate::StringHeader),
    /// An absent key defined through the owned overflow, installed if needed.
    Overflow(*const crate::StringHeader),
    /// The receiver lists the key at this position.
    Own(u32),
}

/// A redefinition of an own DATA property (`pos` of the receiver's list)
/// that changes nothing: every attribute the descriptor names equals the
/// property's (an omitted one is retained), and its `value`, when present,
/// is the current value (bit-identical, so SameValue). That is a successful
/// `[[DefineOwnProperty]]` with no effect, whatever the property's
/// configurability (ValidateAndApplyPropertyDescriptor's step for an
/// unchanged field set). zod's `$constructor` runs its `init` once per trait
/// of a schema, and every run redefines `_zod` with its own value. `false`
/// for anything else: the general definition then validates and applies it.
///
/// # Safety
/// `obj_bits` is the receiver [`definition_admitted`] admitted, unchanged.
unsafe fn redefine_unchanged_own_data(
    obj_bits: u64,
    pos: u32,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    use super::key_attrs::{
        ENTRY_ACCESSOR_MASK, ENTRY_NON_CONFIGURABLE, ENTRY_NON_ENUMERABLE, ENTRY_NON_WRITABLE,
        ENTRY_OPTIONAL, ENTRY_PRIVATE,
    };
    use crate::object::object_ops::{DESC_GET, DESC_SET, DESC_VALUE};
    if descriptor.has(DESC_GET) || descriptor.has(DESC_SET) {
        return false;
    }
    let obj = (obj_bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::ArrayHeader;
    if keys.is_null() || pos >= record.logical_key_count() {
        return false;
    }
    let entry = super::key_attrs::keys_entry(keys, pos);
    if entry & (ENTRY_ACCESSOR_MASK | ENTRY_PRIVATE | ENTRY_OPTIONAL) != 0 {
        return false;
    }
    let same = |name: &[u8], bit: u8| {
        descriptor
            .flag(name)
            .is_none_or(|v| v == (entry & bit == 0))
    };
    if !same(b"writable", ENTRY_NON_WRITABLE)
        || !same(b"enumerable", ENTRY_NON_ENUMERABLE)
        || !same(b"configurable", ENTRY_NON_CONFIGURABLE)
    {
        return false;
    }
    if !descriptor.has(DESC_VALUE) {
        return true;
    }
    let slot = super::key_attrs::key_storage_slot(keys, pos);
    let current = if slot < record.live_inline_slot_count() {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64)
    } else {
        match super::spill::spill_get_present(obj as usize, slot as usize) {
            Some(bits) => bits,
            None => return false,
        }
    };
    current != crate::value::TAG_HOLE && current == descriptor.read(DESC_VALUE).bits()
}

/// The admission of [`define_fresh_own_from_shape`], reading only headers,
/// the shape and the key list (no allocation): the interned key when
/// `obj_bits` is a plain ordinary extensible object that does not have it,
/// or the key's position when it does.
unsafe fn definition_admitted(obj_bits: u64, key_value: f64) -> Option<Admitted> {
    if obj_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (obj_bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
        | crate::gc::OBJ_FLAG_ARGUMENTS;
    if !crate::object_has_meta_word!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & BLOCKING != 0
    {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    let record = super::shapes::object_shape_record(obj)?;
    if record.object_kind() != super::shapes::ShapeObjectKind::Ordinary
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
    {
        return None;
    }
    let meta = crate::object::header::MetaObj::from_type(obj as *mut _, header.obj_type).meta();
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (super::OBJECT_META_FLAG_IS_PROTOTYPE
                    | super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | super::OBJECT_META_FLAG_NATIVE_ALIAS
                    | super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC)
                != 0)
    {
        return None;
    }
    if super::exotic_expando::exotic_expando_kind(addr).is_some()
        || !super::key_attrs::attrs_live_in_keys_of_bits(obj_bits)
        || crate::buffer::is_registered_buffer(addr)
    {
        return None;
    }
    // An interned ordinary name (no index, no private name).
    let key = super::store_key::interned_key_for_store(key_value)?;
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    if bytes == b"__proto__" {
        return None;
    }
    // A reserved hole is absent from the authoritative overflow too. Its
    // static spelling and slot do not make it a present property.
    let table = super::overflow::table(obj);
    if !table.is_null() {
        let present = crate::string::existing_property_atom(key).is_some_and(|atom| {
            super::overflow::find(table, crate::JSValue::string_ptr(atom.cast_mut()).bits())
                .is_some()
        });
        return (!present).then_some(Admitted::Overflow(key));
    }
    if super::overflow::shadowed(obj) {
        return None;
    }
    if super::layout_desc::is_family_object(obj) {
        // Before its first overflow, the checked family descriptor proves
        // an unlisted name absent. An optional listed slot must be a hole.
        let absent = matches!(super::layout_desc::family_own_key(obj, bytes), Some(None))
            || super::key_view::own_key_is_hole(
                obj,
                crate::JSValue::from_bits(key_value.to_bits()),
            );
        return absent.then_some(Admitted::Overflow(key));
    }
    let keys = super::object_keys(obj);
    if !keys.is_null() {
        if let Some(pos) = super::keys_find_property_slot_by_bytes(keys, keys.count(), bytes) {
            return Some(Admitted::Own(pos as u32));
        }
    }
    Some(Admitted::Fresh(key))
}

/// An absent owned key is defined once with its final data attributes.
/// A checked family absence may establish its overflow for the first time.
/// The fixed optional slot stays a hole; the new entry owns value and order.
/// Present properties and accessor definitions keep the complete operation.
unsafe fn define_fresh_overflow_data(
    receiver: &crate::gc::RuntimeHandle<'_>,
    key: *const crate::StringHeader,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    use crate::object::object_ops::{DESC_GET, DESC_SET, DESC_VALUE};
    if descriptor.has(DESC_GET) || descriptor.has(DESC_SET) {
        return false;
    }
    let addr = (receiver.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    let attrs = super::PropertyAttrs::new(
        descriptor.flag(b"writable").unwrap_or(false),
        descriptor.flag(b"enumerable").unwrap_or(false),
        descriptor.flag(b"configurable").unwrap_or(false),
    );
    super::builtin_proto::note_write_addr(addr, "descriptor");
    crate::typedarray_named::note_named_mutation(addr, bytes);
    super::prop_plan::prop_plan_epoch_bump_for_owner(addr);
    super::descriptor_state::note_attrs_born_with_keys(addr);
    let entry = super::key_attrs::AttrsEdit::Data(bytes, attrs.bits).apply(0);
    // insert roots owner/value before canonicalizing an interned spelling
    // to its property atom, then uses the existing insertion and barriers.
    super::overflow::insert(
        addr as *mut ObjectHeader,
        key,
        descriptor.read(DESC_VALUE).bits(),
        entry,
    );
    true
}
