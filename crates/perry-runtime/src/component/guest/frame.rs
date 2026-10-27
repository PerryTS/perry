//! Stable native cells root every value retained by a canonical C adapter.
//! Every potentially throwing operation catches before returning to C.
use super::super::*;
use std::{cell::UnsafeCell, ffi::c_void};

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

pub(super) struct Frame {
    // Field drop order matters: remove roots before freeing their cells.
    scope: RuntimeHandleScope,
    values: Vec<Box<UnsafeCell<f64>>>,
    failed: bool,
    pub(super) transfers: Vec<u32>,
    pub(super) borrows: Vec<u32>,
    lifetime: Option<u32>,
}

impl Frame {
    pub fn new() -> Self {
        let mut frame = Self {
            scope: RuntimeHandleScope::new(),
            values: Vec::new(),
            failed: false,
            transfers: Vec::new(),
            borrows: Vec::new(),
            lifetime: None,
        };
        frame.add(undefined()); // Slot zero retains the original exception.
        frame
    }

    pub fn add(&mut self, value: f64) -> u32 {
        let index = u32::try_from(self.values.len())
            .unwrap_or_else(|_| fail("Too many component frame values"));
        let cell = Box::new(UnsafeCell::new(value));
        unsafe {
            self.scope.root_nanbox_cell(&cell);
        }
        self.values.push(cell);
        index
    }

    pub fn get(&self, index: u32) -> f64 {
        let cell = self
            .values
            .get(index as usize)
            .unwrap_or_else(|| fail("Invalid component frame slot"));
        unsafe { *cell.get() }
    }

    pub(super) fn lifetime(&mut self) -> f64 {
        if let Some(slot) = self.lifetime {
            return self.get(slot);
        }
        let value =
            crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(1) as i64);
        let slot = self.add(value);
        crate::array::js_array_set_f64(array(self.get(slot)), 0, 1.0);
        self.lifetime = Some(slot);
        self.get(slot)
    }

    pub fn error(&self) -> Option<f64> {
        self.failed.then(|| self.get(0))
    }

    pub fn guard<T: Default>(&mut self, body: impl FnOnce(&mut Self) -> T) -> T {
        if self.failed {
            return T::default();
        }
        match crate::exception::catch_js_throw(|| body(self)) {
            Ok(value) => value,
            Err(error) => {
                // Slot zero predates the trap and survives its root restore.
                unsafe {
                    crate::gc::runtime_store_root_nanbox_f64_raw_slot(self.values[0].get(), error);
                }
                self.failed = true;
                T::default()
            }
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let Some(slot) = self.lifetime {
            crate::array::js_array_set_f64(array(self.get(slot)), 0, 0.0);
        }
    }
}

pub(super) unsafe fn frame<'a>(pointer: *mut c_void) -> &'a mut Frame {
    &mut *pointer.cast::<Frame>()
}
unsafe fn span<'a>(pointer: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(pointer, len)
    }
}

#[no_mangle]
pub extern "C" fn perry_component_frame_new() -> *mut c_void {
    Box::into_raw(Box::new(Frame::new())).cast()
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_drop(pointer: *mut c_void) {
    drop(Box::from_raw(pointer.cast::<Frame>()));
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_failed(pointer: *mut c_void) -> i32 {
    i32::from(frame(pointer).failed)
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_initialize(
    pointer: *mut c_void,
    initializer: *const c_void,
) {
    frame(pointer).guard::<()>(|_| {
        if initializer.is_null() {
            fail("Missing component module initializer");
        }
        let initializer: unsafe extern "C" fn() = std::mem::transmute(initializer);
        initializer();
    });
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_fail(
    pointer: *mut c_void,
    message: *const u8,
    len: usize,
) {
    frame(pointer).guard::<()>(|_| {
        fail(std::str::from_utf8(span(message, len)).unwrap_or("Invalid component value"))
    });
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_number(pointer: *mut c_void, value: f64) -> u32 {
    frame(pointer).guard(|frame| frame.add(f64::from_bits(JSValue::number(value).bits())))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_bool(pointer: *mut c_void, value: i32) -> u32 {
    frame(pointer).guard(|frame| frame.add(f64::from_bits(JSValue::bool(value != 0).bits())))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_s64(pointer: *mut c_void, value: i64) -> u32 {
    frame(pointer).guard(|frame| {
        frame.add(crate::value::js_nanbox_bigint(
            crate::bigint_hooks::js_bigint_from_i64(value) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_u64(pointer: *mut c_void, value: u64) -> u32 {
    frame(pointer).guard(|frame| {
        frame.add(crate::value::js_nanbox_bigint(
            crate::bigint_hooks::js_bigint_from_u64(value) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_string(
    pointer: *mut c_void,
    bytes: *const u8,
    len: usize,
) -> u32 {
    frame(pointer).guard(|frame| {
        let len = u32::try_from(len).unwrap_or_else(|_| fail("Component string is too long"));
        frame.add(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(bytes, len) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_char(pointer: *mut c_void, value: u32) -> u32 {
    frame(pointer).guard(|frame| {
        let value = char::from_u32(value).unwrap_or_else(|| fail("Invalid WIT character"));
        let mut bytes = [0; 4];
        let text = value.encode_utf8(&mut bytes);
        frame.add(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_bytes(
    pointer: *mut c_void,
    bytes: *const u8,
    len: usize,
) -> u32 {
    frame(pointer).guard(|frame| {
        let len = u32::try_from(len).unwrap_or_else(|_| fail("Component byte list is too long"));
        let array = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_UINT8, len);
        let slot = frame.add(crate::value::js_nanbox_pointer(array as i64));
        for (i, byte) in span(bytes, len as usize).iter().enumerate() {
            let array = JSValue::from_bits(frame.get(slot).to_bits())
                .as_pointer::<crate::typedarray::TypedArrayHeader>()
                as *mut _;
            crate::typedarray::js_typed_array_set(array, i as i32, *byte as f64);
        }
        slot
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_array(pointer: *mut c_void, len: u32) -> u32 {
    frame(pointer).guard(|frame| {
        frame.add(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(len) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_object(pointer: *mut c_void) -> u32 {
    frame(pointer).guard(|frame| {
        frame.add(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_set(
    pointer: *mut c_void,
    object: u32,
    key: *const u8,
    len: usize,
    value: u32,
) {
    frame(pointer).guard(|frame| set_field(frame.get(object), span(key, len), frame.get(value)));
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get(
    pointer: *mut c_void,
    object: u32,
    key: *const u8,
    len: usize,
) -> u32 {
    frame(pointer).guard(|frame| {
        let value = field(frame.get(object), span(key, len));
        frame.add(value)
    })
}

fn array(value: f64) -> *mut crate::array::ArrayHeader {
    array_pointer(value).unwrap_or_else(|| fail("Expected a WIT list or tuple"))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_array_set(
    pointer: *mut c_void,
    list: u32,
    index: u32,
    value: u32,
) {
    frame(pointer).guard(|frame| {
        crate::array::js_array_set_f64(array(frame.get(list)), index, frame.get(value))
    });
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_array_get(
    pointer: *mut c_void,
    list: u32,
    index: u32,
) -> u32 {
    frame(pointer).guard(|frame| {
        let value = crate::array::js_array_get_f64(array(frame.get(list)), index);
        frame.add(value)
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_array_len(pointer: *mut c_void, list: u32) -> u32 {
    frame(pointer).guard(|frame| crate::array::js_array_length(array(frame.get(list))))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_number(pointer: *mut c_void, slot: u32) -> f64 {
    frame(pointer).guard(|frame| {
        let value = JSValue::from_bits(frame.get(slot).to_bits());
        if !value.is_number() {
            fail("Expected a WIT number");
        }
        value.as_number()
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_integer(
    pointer: *mut c_void,
    slot: u32,
    minimum: f64,
    maximum: f64,
) -> f64 {
    frame(pointer).guard(|frame| {
        let value = JSValue::from_bits(frame.get(slot).to_bits());
        if !value.is_number() {
            fail("Expected a WIT integer");
        }
        let value = value.as_number();
        if !value.is_finite() || value.fract() != 0.0 || value < minimum || value > maximum {
            fail("WIT integer is out of range");
        }
        value
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_bool(pointer: *mut c_void, slot: u32) -> i32 {
    frame(pointer).guard(|frame| {
        let value = JSValue::from_bits(frame.get(slot).to_bits());
        if !value.is_bool() {
            fail("Expected a WIT boolean");
        }
        i32::from(value.as_bool())
    })
}

fn bigint_bits(value: f64, signed: bool) -> u64 {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_bigint() {
        fail("WIT s64 and u64 require BigInt");
    }
    let limbs = unsafe { crate::bigint::BigIntHeader::all_limbs(value.as_bigint_ptr()) };
    let low = limbs.first().copied().unwrap_or(0);
    let fill = if signed && low >> 63 != 0 {
        u64::MAX
    } else {
        0
    };
    if (!signed && limbs.last().is_some_and(|last| last >> 63 != 0))
        || !limbs.iter().skip(1).all(|limb| *limb == fill)
    {
        fail("WIT 64-bit integer is out of range");
    }
    low
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_u64(pointer: *mut c_void, slot: u32) -> u64 {
    frame(pointer).guard(|frame| bigint_bits(frame.get(slot), false))
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_s64(pointer: *mut c_void, slot: u32) -> i64 {
    frame(pointer).guard(|frame| bigint_bits(frame.get(slot), true) as i64)
}

// Keep converted native strings outside the trap so invalid values and a
// caller's undersized destination cannot leak scratch allocations.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_string_len(
    pointer: *mut c_void,
    slot: u32,
) -> usize {
    let mut text = String::new();
    let result = frame(pointer).guard(|frame| {
        text = string(frame.get(slot));
        text.len()
    });
    drop(text);
    result
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_string_copy(
    pointer: *mut c_void,
    slot: u32,
    destination: *mut u8,
    len: usize,
) {
    let mut text = String::new();
    frame(pointer).guard(|frame| {
        text = string(frame.get(slot));
        if len != text.len() {
            fail("Component string destination has wrong size");
        }
        if len > 0 {
            std::ptr::copy_nonoverlapping(text.as_ptr(), destination, len);
        }
    });
    drop(text);
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_get_char(pointer: *mut c_void, slot: u32) -> u32 {
    let mut text = String::new();
    let result = frame(pointer).guard(|frame| {
        text = string(frame.get(slot));
        let mut chars = text.chars();
        let value = chars
            .next()
            .unwrap_or_else(|| fail("WIT char must contain one Unicode scalar"));
        if chars.next().is_some() {
            fail("WIT char must contain one Unicode scalar");
        }
        value as u32
    });
    drop(text);
    result
}

#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_bytes_len(pointer: *mut c_void, slot: u32) -> usize {
    frame(pointer).guard(|frame| {
        crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes(frame.get(slot), scope).map(|bytes| bytes.len())
        })
        .unwrap_or_else(|_| fail("Expected a component byte list"))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_bytes_copy(
    pointer: *mut c_void,
    slot: u32,
    destination: *mut u8,
    len: usize,
) {
    frame(pointer).guard(|frame| {
        let copied = crate::buffer::bytes::no_gc(|scope| {
            let bytes = crate::buffer::bytes::bytes(frame.get(slot), scope).ok()?;
            if bytes.len() != len {
                return None;
            }
            if len > 0 {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, len);
            }
            Some(())
        });
        if copied.is_none() {
            fail("Invalid component byte-list destination");
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_call(
    pointer: *mut c_void,
    interface: u32,
    function: u32,
    args: u32,
) -> u32 {
    let mut values = Vec::new();
    let result = frame(pointer).guard(|frame| {
        let scope = RuntimeHandleScope::new();
        let registry = scope.root_nanbox_f64(super::registry());
        let context = context::get(registry.get_nanbox_f64());
        let world = &(*context).world;
        let interface = &world["exports"][interface as usize];
        let function = &interface["functions"][function as usize];
        let namespace = scope.root_nanbox_f64(super::exports(registry.get_nanbox_f64()));
        let name = interface["name"]
            .as_str()
            .unwrap_or_else(|| fail("Unknown component export interface"));
        let namespace = scope.root_nanbox_f64(if name.is_empty() {
            namespace.get_nanbox_f64()
        } else {
            field(namespace.get_nanbox_f64(), name.as_bytes())
        });
        let name = function["name"]
            .as_str()
            .unwrap_or_else(|| fail("Unknown component export function"));
        let callback = scope.root_nanbox_f64(field(namespace.get_nanbox_f64(), name.as_bytes()));
        let count = crate::array::js_array_length(array(frame.get(args)));
        if function["params"].as_array().map(Vec::len) != Some(count as usize) {
            fail("Component export argument count mismatch");
        }
        for index in 0..count {
            values.push(crate::array::js_array_get_f64(
                array(frame.get(args)),
                index,
            ));
        }
        let value = crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            values.as_ptr(),
            values.len(),
        );
        // A frame slot must outlive this call's temporary handles. Publishing
        // it before their scope ends would let that scope truncate its root.
        drop(scope);
        frame.add(value)
    });
    drop(values);
    // A reactor has no command event loop. Complete a job checkpoint after
    // a successful export while its arguments and result remain rooted in
    // this frame, before the C adapter reads them. This delivers resource
    // finalizers recorded by explicit or automatic collection as ordinary
    // jobs; it does not wait for timers, I/O or asynchronous WIT results.
    frame(pointer).guard(|_| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    result
}

// Immutable code addresses keep canonical adapter entry points in static archives.
#[cfg(feature = "keepalive-anchors")]
struct AdapterEntry {
    _code: *const (),
}
#[cfg(feature = "keepalive-anchors")]
unsafe impl Sync for AdapterEntry {}
#[cfg(feature = "keepalive-anchors")]
const fn adapter_entry(code: *const ()) -> AdapterEntry {
    AdapterEntry { _code: code }
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ADAPTER_ENTRIES: [AdapterEntry; 36] = [
    adapter_entry(super::resources::payload::js_component_resource_create as *const ()),
    adapter_entry(super::resources::payload::js_component_resource_value as *const ()),
    adapter_entry(super::resources::payload::perry_component_frame_resource_destroy as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_lift as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_lower as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_commit as *const ()),
    adapter_entry(perry_component_frame_new as *const ()),
    adapter_entry(perry_component_frame_drop as *const ()),
    adapter_entry(perry_component_frame_failed as *const ()),
    adapter_entry(perry_component_frame_initialize as *const ()),
    adapter_entry(perry_component_frame_fail as *const ()),
    adapter_entry(perry_component_frame_number as *const ()),
    adapter_entry(perry_component_frame_bool as *const ()),
    adapter_entry(perry_component_frame_s64 as *const ()),
    adapter_entry(perry_component_frame_u64 as *const ()),
    adapter_entry(perry_component_frame_string as *const ()),
    adapter_entry(perry_component_frame_char as *const ()),
    adapter_entry(perry_component_frame_bytes as *const ()),
    adapter_entry(perry_component_frame_array as *const ()),
    adapter_entry(perry_component_frame_object as *const ()),
    adapter_entry(perry_component_frame_set as *const ()),
    adapter_entry(perry_component_frame_get as *const ()),
    adapter_entry(perry_component_frame_array_set as *const ()),
    adapter_entry(perry_component_frame_array_get as *const ()),
    adapter_entry(perry_component_frame_array_len as *const ()),
    adapter_entry(perry_component_frame_get_number as *const ()),
    adapter_entry(perry_component_frame_get_integer as *const ()),
    adapter_entry(perry_component_frame_get_bool as *const ()),
    adapter_entry(perry_component_frame_get_u64 as *const ()),
    adapter_entry(perry_component_frame_get_s64 as *const ()),
    adapter_entry(perry_component_frame_string_len as *const ()),
    adapter_entry(perry_component_frame_string_copy as *const ()),
    adapter_entry(perry_component_frame_get_char as *const ()),
    adapter_entry(perry_component_frame_bytes_len as *const ()),
    adapter_entry(perry_component_frame_bytes_copy as *const ()),
    adapter_entry(perry_component_frame_call as *const ()),
];
