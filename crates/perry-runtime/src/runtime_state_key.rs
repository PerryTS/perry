//! Rust call sites read and write fixed public names with ordinary Get/Set
//! semantics, through the same probe as an emitted read. Nothing is kept per
//! site: no key registry, no memo, no object-address cache.
use crate::object::ObjectHeader;

/// A fixed key of a Rust call site. Static bytes are not GC pointers.
#[derive(Clone, Copy)]
pub(crate) struct NamedStateKey {
    pub(crate) bytes: &'static [u8],
    /// `key_bytes_hash(bytes)`, computed when the site is compiled.
    pub(crate) hash: u64,
}

/// `object::key_bytes_hash` (FNV-1a) as a `const fn`, for a fixed key.
pub(crate) const fn const_key_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

#[macro_export]
macro_rules! runtime_state_key {
    ($key:expr) => {{
        const KEY: &[u8] = $key;
        const HASH: u64 = $crate::runtime_state_key::const_key_hash(KEY);
        $crate::runtime_state_key::NamedStateKey {
            bytes: KEY,
            hash: HASH,
        }
    }};
}

impl NamedStateKey {
    /// Read and call this site's fixed method, preserving ordinary Get and
    /// the original receiver. Native handles keep their existing dispatch.
    /// # Safety
    /// `args` is readable for `argc` live JavaScript values.
    pub(crate) unsafe fn call_value(self, target: f64, args: *const f64, argc: usize) -> f64 {
        js_runtime_state_key_call(target, self.bytes.as_ptr(), self.bytes.len(), args, argc)
    }

    /// Overwrite a proven own writable data slot found in the receiver's
    /// layout. Adds, accessors, and exotic receivers keep ordinary Set.
    pub(crate) fn write_value(self, target: f64, value: f64) {
        if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(target)
        {
            if unsafe { self.write_existing(obj, value) || self.add_family(obj, value) } {
                return;
            }
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(target);
        let value = scope.root_nanbox_f64(value);
        let key = crate::string::intern_ascii_literal(self.bytes);
        let key = crate::value::JSValue::string_ptr(key as *mut _);
        unsafe {
            crate::object::js_object_set_property_key(
                target.get_nanbox_f64(),
                f64::from_bits(key.bits()),
                value.get_nanbox_f64(),
            );
        }
    }

    /// The add of this key to a family object with no overflow, as the store
    /// entries make it (`layout_desc::family_store`: in key order into its
    /// slot, else through the overflow), when the add is plain (an extensible
    /// receiver, a chain that does not intercept it). `false`, having stored
    /// nothing, for every other receiver or add, and for a key text with no
    /// atom yet (no layout names it).
    pub(crate) unsafe fn add_family(self, obj: *mut ObjectHeader, value: f64) -> bool {
        if crate::object::layout_desc::family_own_key_hashed(obj, self.bytes, Some(self.hash)).is_none() {
            return false;
        }
        let Some(atom) = crate::string::atom_lookup(self.bytes, self.hash)
            .filter(|&atom| crate::string::is_property_atom(atom))
        else {
            return false;
        };
        crate::object::layout_desc::family_store(
            obj,
            crate::value::STRING_TAG | atom as u64,
            self.bytes,
            value.to_bits(),
        )
    }

    pub(crate) unsafe fn write_existing(self, obj: *mut ObjectHeader, value: f64) -> bool {
        let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        // An object with an overflow keeps its keys' attributes and any key
        // added after it there, not in its layout (object model step 4): the
        // ordinary Set decides.
        if header._reserved & crate::gc::OBJ_FLAG_FROZEN != 0
            || (header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
                && !crate::object::key_attrs::attrs_live_in_keys(obj as usize))
            || crate::object::overflow::has_overflow(obj)
        {
            return false;
        }
        let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
            return false;
        };
        if !shape.object_kind.is_ordinary_layout() {
            return false;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        // A family object answers from its layout's index (its keys are
        // plain writable data); any other layout from its key list.
        let slot = match crate::object::layout_desc::family_own_key_hashed(obj, self.bytes, Some(self.hash)) {
            Some(Some((_, slot))) => slot,
            Some(None) => return false,
            None => {
                let Some(position) = crate::object::keys_find_property_slot_by_bytes(
                    keys,
                    shape.logical_key_count,
                    self.bytes,
                ) else {
                    return false;
                };
                let entry = crate::object::key_attrs::keys_entry(keys, position);
                if entry
                    & (crate::object::key_attrs::ENTRY_NON_WRITABLE
                        | crate::object::key_attrs::ENTRY_ACCESSOR)
                    != 0
                {
                    return false;
                }
                crate::object::key_attrs::key_storage_slot(keys, position)
            }
        };
        if crate::object::object_field_at_with_live(obj, slot, shape.live_inline_slot_count).bits()
            == crate::value::TAG_HOLE
        {
            return false;
        }
        if slot < shape.live_inline_slot_count {
            crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
        } else {
            crate::object::overflow_set(obj as usize, slot as usize, value.to_bits());
        }
        true
    }

    pub(crate) fn read_value(self, value: f64) -> f64 {
        if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(value) {
            return unsafe { self.read_object(obj) };
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let key = crate::string::intern_ascii_literal(self.bytes);
        let key = crate::value::JSValue::string_ptr(key as *mut _);
        unsafe {
            crate::object::js_object_get_property_key(
                value.get_nanbox_f64(),
                f64::from_bits(key.bits()),
            )
        }
    }

    /// Receiver is already checked by the module's existing receiver gate.
    pub(crate) unsafe fn read_object(self, obj: *mut ObjectHeader) -> f64 {
        crate::object::field_get_set::runtime_read_site::get(obj, self.bytes)
    }

    /// Runtime-private state keeps own-only semantics, through the same word.
    pub(crate) unsafe fn read_own(self, obj: *mut ObjectHeader) -> Option<f64> {
        // An object with an overflow lists every own key there; a deleted
        // layout key's slot holds the hole marker (object model step 4).
        let table = crate::object::overflow::table(obj);
        if !table.is_null() {
            let entry = crate::object::overflow::find_bytes(table, self.bytes)?;
            if crate::object::overflow::entry_attrs(table.cast(), entry)
                & crate::object::key_attrs::ENTRY_ACCESSOR
                != 0
            {
                return None;
            }
            return crate::object::overflow::get_from(table, obj, entry)
                .filter(|&bits| bits != crate::value::TAG_HOLE)
                .map(f64::from_bits);
        }
        let shape = crate::object::shapes::object_shape_descriptor(obj)?;
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        let slot = match crate::object::layout_desc::family_own_key_hashed(obj, self.bytes, Some(self.hash)) {
            Some(found) => found?.1,
            None => {
                let position = crate::object::keys_find_property_slot_by_bytes(
                    keys,
                    shape.logical_key_count,
                    self.bytes,
                )?;
                if crate::object::key_attrs::key_is_accessor_at(keys, position) {
                    return None;
                }
                crate::object::key_attrs::key_storage_slot(keys, position)
            }
        };
        let live = crate::object::object_live_slot_count(obj);
        // A slot holding the hole is an absent optional key (`key_view`).
        let bits = crate::object::object_field_at_with_live(obj, slot, live).bits();
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }
}

/// Dynamic event names and symbols retain their existing computed-key Get.
/// Only fixed names go through this module.
pub(crate) fn read_computed(value: f64, key: f64) -> f64 {
    unsafe { crate::object::js_object_get_property_key(value, key) }
}

/// Ordinary fixed-key Get for an extension's call site.
/// # Safety
/// `key` is the site's fixed name, readable for `len` bytes for the duration
/// of this call.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_runtime_state_key_get(
    value: f64,
    key: *const u8,
    len: usize,
) -> f64 {
    let bytes = std::slice::from_raw_parts(key, len);
    if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(value) {
        return crate::object::field_get_set::runtime_read_site::get(obj, bytes);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::intern_ascii_literal(bytes);
    let key = crate::value::JSValue::string_ptr(key as *mut _);
    crate::object::js_object_get_property_key(value.get_nanbox_f64(), f64::from_bits(key.bits()))
}

/// Call the method `key` of `value`, with the receiver as `this`. Exotic
/// method receivers keep the existing dispatch.
/// # Safety
/// As js_runtime_state_key_get; args is readable for argc values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_runtime_state_key_call(
    value: f64,
    key: *const u8,
    len: usize,
    args: *const f64,
    argc: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let args = if argc == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, argc)
    };
    let args = scope.root_nanbox_f64_slice(args);
    if let Some(obj) =
        crate::object::field_get_set::runtime_read_site::object_receiver(value.get_nanbox_f64())
    {
        let method = crate::object::field_get_set::runtime_read_site::get(
            obj,
            std::slice::from_raw_parts(key, len),
        );
        let live = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
        return crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(value.get_nanbox_f64()),
            live.as_ptr(),
            live.len(),
        );
    }
    let live = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    crate::object::js_native_call_method(
        value.get_nanbox_f64(),
        key as *const i8,
        len,
        live.as_ptr(),
        live.len(),
    )
}

#[cfg(test)]
#[path = "runtime_state_key_tests.rs"]
mod tests;
