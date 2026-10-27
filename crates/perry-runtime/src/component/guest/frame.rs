//! Owned traced arrays retain canonical values across suspension and interleaving.
//! Every potentially throwing operation catches before returning to C.
use super::super::*;
#[cfg(test)]
mod bootstrap_tests;
mod promise;
mod suspended;
mod task;
use crate::native_payload::{self as payload, NativePayloadFamily, OwnerLink};
pub use promise::perry_component_frame_promise_poll;
use std::{
    ffi::c_void,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub use suspended::{
    perry_component_frame_import_begin, perry_component_frame_import_reject,
    perry_component_frame_import_resolve,
};
pub use task::{
    perry_component_frame_task_call, perry_component_frame_task_cancel,
    perry_component_frame_task_checkpoint, perry_component_frame_task_new,
    perry_component_frame_task_ready, perry_component_frame_task_resume,
};

static FRAME: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::COMPONENT_GUEST_FRAME,
    links_owner: true,
    name: "ComponentGuestFrame",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: |_| {},
};

// Teardown may finalize the GC cell before a cancelled C task frees its frame.
// This shared native flag outlives that cell; its Drop never touches JS or TLS.
struct Anchor(Arc<AtomicBool>);
impl Drop for Anchor {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

pub(super) struct Frame {
    owner: OwnerLink,
    alive: Arc<AtomicBool>,
    length: u32,
    // OwnerLink must only be resolved on its creating agent. The C ABI has the
    // same affinity requirement; Rust callers cannot move a frame to a worker.
    _thread: PhantomData<Rc<()>>,
    failed: bool,
    pub(super) transfers: Vec<u32>,
    pub(super) borrows: Vec<u32>,
    // Each entry roots both its wrapper and its private owner state in slots.
    // Native metadata never caches a managed address across suspension.
    pub(super) borrow_states: Vec<u32>,
    pub(super) committed_borrows: Vec<(u32, u32)>,
    lifetime: Option<u32>,
    task: Option<task::TaskContext>,
    suspended: Option<suspended::SuspendedImport>,
}

impl Frame {
    pub fn allocate_guest_frame() -> Self {
        let scope = RuntimeHandleScope::new();
        let values = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(1) as i64,
        ));
        let owner = scope.root_nanbox_f64(payload::alloc_closed(&FRAME, &[]));
        payload::state_set(
            owner.get_nanbox_f64(),
            &FRAME,
            b"values",
            values.get_nanbox_f64(),
        );
        let alive = Arc::new(AtomicBool::new(true));
        payload::attach(
            owner.get_nanbox_f64(),
            &FRAME,
            Anchor(alive.clone()),
            std::mem::size_of::<Self>(),
        )
        .expect("new private canonical frame");
        let owner =
            payload::owner_link(owner.get_nanbox_f64(), &FRAME).expect("canonical frame owner");
        unsafe { payload::link_ref(owner) };
        Self {
            owner,
            alive,
            length: 1,
            _thread: PhantomData,
            failed: false,
            transfers: Vec::new(),
            borrows: Vec::new(),
            borrow_states: Vec::new(),
            committed_borrows: Vec::new(),
            lifetime: None,
            task: None,
            suspended: None,
        }
    }

    /// An async C import owns this stable heap frame until its canonical child
    /// and typed result storage are retired. Sync imports release immediately.
    /// The callback must retain the same pointer before beginning deferral.
    pub(super) fn release_import(self: Box<Self>) {
        if self.suspended.is_some() {
            let _ = Box::into_raw(self);
        }
    }

    #[cfg(test)]
    pub(super) fn import_is_deferred(&self) -> bool {
        self.suspended.is_some()
    }

    #[cfg(test)]
    pub(super) fn weak_anchor(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.alive)
    }

    // This read never allocates or calls JS: the private state object has only
    // direct data slots. No transient handle remains registered after the read.
    fn values(&self) -> f64 {
        assert!(
            self.alive.load(Ordering::Acquire),
            "canonical frame used after heap teardown"
        );
        let owner = unsafe { payload::link_event_owner(self.owner) }
            .expect("canonical frame on owner thread");
        payload::state_get(owner, &FRAME, b"values")
    }

    pub fn add(&mut self, value: f64) -> u32 {
        let index = self.length;
        let next = index
            .checked_add(1)
            .unwrap_or_else(|| fail("Too many component frame values"));
        // Growing the owned array can collect. Root the incoming value before
        // any managed allocation, then let the array's ordinary barrier own it.
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let values = scope.root_nanbox_f64(self.values());
        let grown = crate::array::js_array_set_f64_extend(
            array(values.get_nanbox_f64()),
            index,
            value.get_nanbox_f64(),
        );
        // Array growth can replace its header outside the collector. Publish
        // the returned head, not an old forwarding stub, into the owner edge.
        let values = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
        let owner =
            unsafe { payload::link_event_owner(self.owner) }.expect("canonical frame owner");
        payload::state_set(owner, &FRAME, b"values", values.get_nanbox_f64());
        self.length = next;
        index
    }

    pub fn get(&self, index: u32) -> f64 {
        if index >= self.length {
            fail("Invalid component frame slot");
        }
        crate::array::js_array_get_f64(array(self.values()), index)
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
                // The owned slot array survives transient exception-root
                // restoration. Keep the original error before later cleanup.
                let scope = RuntimeHandleScope::new();
                let error = scope.root_nanbox_f64(error);
                let values = scope.root_nanbox_f64(self.values());
                crate::array::js_array_set_f64(
                    array(values.get_nanbox_f64()),
                    0,
                    error.get_nanbox_f64(),
                );
                self.failed = true;
                T::default()
            }
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        // The GC-cell finalizer flips this flag before heap unmapping. A late
        // native C-task drop must free only Rust state after that point.
        if self.alive.load(Ordering::Acquire) {
            super::resources::release_borrows(self);
            if let Some(slot) = self.lifetime {
                crate::array::js_array_set_f64(array(self.get(slot)), 0, 0.0);
            }
            let owner = unsafe { payload::link_event_owner(self.owner) }
                .expect("canonical frame on owner thread");
            payload::close(owner, &FRAME);
            unsafe { payload::link_unref(self.owner) };
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
    Box::into_raw(Box::new(Frame::allocate_guest_frame())).cast()
}
/// Initialize the image before allocating any managed adapter state. Module
/// initialization binds the image's static layout prefix; an earlier frame
/// allocation would instead bind the runtime-only prefix permanently.
///
/// # Safety
/// A non-null initializer must be a C ABI callback taking `*mut c_void`;
/// Perry exceptions are caught here. `data` must remain valid until the
/// callback returns. No callback or data is retained.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_bootstrap(
    initializer: *const c_void,
    data: *mut c_void,
) -> *mut c_void {
    if initializer.is_null() {
        return std::ptr::null_mut();
    }
    let initializer: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(initializer);
    let result = crate::exception::catch_js_throw(|| initializer(data));
    // Trap setup and the initializer precede every managed frame allocation.
    // Root a thrown value before constructing the frame that will own it.
    let scope = RuntimeHandleScope::new();
    let error = result.err().map(|error| scope.root_nanbox_f64(error));
    let mut frame = Frame::allocate_guest_frame();
    if let Some(error) = error {
        let values = scope.root_nanbox_f64(frame.values());
        crate::array::js_array_set_f64(
            array(values.get_nanbox_f64()),
            0,
            error.get_nanbox_f64(),
        );
        frame.failed = true;
    }
    Box::into_raw(Box::new(frame)).cast()
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
            crate::bigint::js_bigint_from_i64(value) as i64,
        ))
    })
}
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_u64(pointer: *mut c_void, value: u64) -> u32 {
    frame(pointer).guard(|frame| {
        frame.add(crate::value::js_nanbox_bigint(
            crate::bigint::js_bigint_from_u64(value) as i64,
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
        // The new array has no aliases. Copy its canonical byte list in one
        // allocation-free borrow; per-element JS setters spend guest fuel on
        // receiver validation for every byte of an otherwise contiguous list.
        let copied = crate::buffer::bytes::no_gc(|scope| {
            let destination = crate::buffer::bytes::bytes_mut(frame.get(slot), scope).ok()?;
            destination.copy_from_slice(span(bytes, len as usize));
            Some(())
        });
        if copied.is_none() {
            fail("Invalid component byte-list allocation");
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

/// Copy a rooted value into an independently owned frame. This permits C to
/// convert a task result in a distinct frame under task_resume without
/// reborrowing the task frame. No JS or jobs run here.
/// # Safety
/// Both pointers are open frames on this agent, with no outstanding borrows.
/// They may be equal. The slot belongs to source; errors belong to destination.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_copy(
    destination: *mut c_void,
    source: *mut c_void,
    slot: u32,
) -> u32 {
    // End the source borrow before taking the destination borrow (which may
    // alias it). add roots the incoming value before its first allocation.
    frame(destination).guard(|destination| {
        // In the equal-pointer case use the existing exclusive borrow.
        let value = if std::ptr::eq(destination as *mut Frame, source.cast()) {
            destination.get(slot)
        } else {
            frame(source).get(slot)
        };
        destination.add(value)
    })
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
    let result = call_export(frame(pointer), interface, function, args);
    frame(pointer).guard(|_| {
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    });
    result
}

unsafe fn call_export(frame: &mut Frame, interface: u32, function: u32, args: u32) -> u32 {
    let mut values = Vec::new();
    let result = frame.guard(|frame| {
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
        // Publish into the owned frame before releasing this call's temporary
        // handles. Its lifetime is independent of their stack checkpoint.
        frame.add(value)
    });
    drop(values);
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
static KEEP_ADAPTER_ENTRIES: [AdapterEntry; 48] = [
    adapter_entry(super::resources::payload::js_component_resource_create as *const ()),
    adapter_entry(super::resources::payload::js_component_resource_value as *const ()),
    adapter_entry(super::resources::payload::perry_component_frame_resource_destroy as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_lift as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_lower as *const ()),
    adapter_entry(super::resources::perry_component_frame_resource_commit as *const ()),
    adapter_entry(perry_component_frame_new as *const ()),
    adapter_entry(perry_component_frame_bootstrap as *const ()),
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
    adapter_entry(perry_component_frame_copy as *const ()),
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
    adapter_entry(perry_component_frame_promise_poll as *const ()),
    adapter_entry(perry_component_frame_task_new as *const ()),
    adapter_entry(perry_component_frame_task_call as *const ()),
    adapter_entry(perry_component_frame_task_checkpoint as *const ()),
    adapter_entry(perry_component_frame_task_ready as *const ()),
    adapter_entry(perry_component_frame_task_resume as *const ()),
    adapter_entry(perry_component_frame_task_cancel as *const ()),
    adapter_entry(perry_component_frame_import_begin as *const ()),
    adapter_entry(perry_component_frame_import_resolve as *const ()),
    adapter_entry(perry_component_frame_import_reject as *const ()),
];
