//! Rust call sites use the same memo as native JS state, with ordinary Get
//! semantics for public names. No key registry or object-address cache.
use crate::native_payload::StateKeyMemo;
use crate::object::ObjectHeader;

/// A fixed key and the words of the Rust site that uses it. Static bytes are
/// not GC pointers; holder roots remain in the existing emitted-read PIC.
#[derive(Clone, Copy)]
pub(crate) struct NamedStateKey {
    pub(crate) bytes: &'static [u8],
    pub(crate) memo: &'static StateKeyMemo,
}

#[macro_export]
macro_rules! runtime_state_key {
    ($key:expr) => {{
        const KEY: &[u8] = $key;
        $crate::state_key_memo!(static SITE);
        $crate::runtime_state_key::NamedStateKey { bytes: KEY, memo: &SITE }
    }};
}

impl NamedStateKey {
    /// Read and call this site's fixed method, preserving ordinary Get and
    /// the original receiver. Native handles keep their existing dispatch.
    /// # Safety
    /// `args` is readable for `argc` live JavaScript values.
    pub(crate) unsafe fn call_value(self, target: f64, args: *const f64, argc: usize) -> f64 {
        self.memo.with(|site| {
            js_runtime_state_key_call(
                target,
                self.bytes.as_ptr(),
                self.bytes.len(),
                site,
                args,
                argc,
            )
        })
    }

    /// Overwrite a proven own writable data slot through the same compact
    /// word as reads. Adds, accessors, and exotic receivers keep ordinary Set.
    pub(crate) fn write_value(self, target: f64, value: f64) {
        if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(target)
        {
            if unsafe { self.write_existing(obj, value) } {
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

    pub(crate) unsafe fn write_existing(self, obj: *mut ObjectHeader, value: f64) -> bool {
        let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        if header._reserved & crate::gc::OBJ_FLAG_FROZEN != 0
            || (header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
                && !crate::object::key_attrs::attrs_live_in_keys(obj as usize))
        {
            return false;
        }
        self.memo.with(|site| {
            let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
                return false;
            };
            if !shape.object_kind.is_ordinary_layout() {
                return false;
            }
            let keys = shape.keys as usize as *const crate::array::ArrayHeader;
            let slot = match site.own_slot(obj) {
                Some(slot) => slot,
                None => {
                    let Some(slot) = crate::object::keys_find_property_slot_by_bytes(
                        keys,
                        shape.logical_key_count,
                        self.bytes,
                    ) else {
                        return false;
                    };
                    slot
                }
            };
            let entry = crate::object::key_attrs::keys_entry(keys, slot);
            if entry
                & (crate::object::key_attrs::ENTRY_NON_WRITABLE
                    | crate::object::key_attrs::ENTRY_ACCESSOR)
                != 0
            {
                return false;
            }
            if crate::object::object_field_at_with_live(obj, slot, shape.live_inline_slot_count)
                .bits()
                == crate::value::TAG_HOLE
            {
                return false;
            }
            site.prime_own_slot(
                crate::object::shapes::object_shape_stamp(obj),
                slot,
                shape.live_inline_slot_count,
            );
            if slot < shape.live_inline_slot_count {
                crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
            } else {
                crate::object::overflow_set(obj as usize, slot as usize, value.to_bits());
            }
            true
        })
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
        self.memo.with(|site| site.read(obj, self.bytes))
    }

    /// Runtime-private state keeps own-only semantics, through the same word.
    pub(crate) unsafe fn read_own(self, obj: *mut ObjectHeader) -> Option<f64> {
        self.memo.with(|site| {
            let slot = if let Some(slot) = site.own_slot(obj) {
                slot
            } else {
                let shape = crate::object::shapes::object_shape_descriptor(obj)?;
                let keys = shape.keys as usize as *const crate::array::ArrayHeader;
                let slot = crate::object::keys_find_property_slot_by_bytes(
                    keys,
                    shape.logical_key_count,
                    self.bytes,
                )?;
                if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
                    return None;
                }
                site.prime_own_slot(
                    crate::object::shapes::object_shape_stamp(obj),
                    slot,
                    shape.live_inline_slot_count,
                );
                slot
            };
            let live = crate::object::object_live_slot_count(obj);
            Some(f64::from_bits(
                crate::object::object_field_at_with_live(obj, slot, live).bits(),
            ))
        })
    }
}

/// Dynamic event names and symbols retain their existing computed-key Get.
/// Only fixed names are memoized by this module.
pub(crate) fn read_computed(value: f64, key: f64) -> f64 {
    unsafe { crate::object::js_object_get_property_key(value, key) }
}

/// Ordinary fixed-key Get for an extension's static per-agent site.
/// # Safety
/// `site` has the StateKeySite ABI, and `key` is this site's fixed name,
/// readable for `len` bytes for the duration of this call.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_runtime_state_key_get(
    value: f64,
    key: *const u8,
    len: usize,
    site: *const crate::native_payload::StateKeySite,
) -> f64 {
    let bytes = std::slice::from_raw_parts(key, len);
    if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(value) {
        return (*site).read(obj, bytes);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::intern_ascii_literal(bytes);
    let key = crate::value::JSValue::string_ptr(key as *mut _);
    crate::object::js_object_get_property_key(value.get_nanbox_f64(), f64::from_bits(key.bits()))
}

/// Call the method read through an extension's static site, with the receiver
/// as `this`. Exotic method receivers keep the existing dispatch.
/// # Safety
/// As js_runtime_state_key_get; args is readable for argc values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_runtime_state_key_call(
    value: f64,
    key: *const u8,
    len: usize,
    site: *const crate::native_payload::StateKeySite,
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
        let method = (*site).read(obj, std::slice::from_raw_parts(key, len));
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
