//! Owned canonical readers in a guest. JS never sees a canonical handle.
//! Finalization only queues integer identities. C drains that queue after
//! returning from the runtime, where canonical operations may safely suspend.
//! Reads/writes and guest-created writer pairs are separate scheduler work.
use super::frame::{frame, Frame};
use super::*;
use crate::object::owned_state_bag::{
    COMPONENT_GUEST_ENDPOINT, COMPONENT_GUEST_ENDPOINT_FINALIZATION,
};
use std::ffi::c_void;
pub(super) mod io;
pub(super) mod producer;
mod writer;

const OWNER: u32 = 0;
const TYPE: u32 = 1;
const HANDLE: u32 = 2;
const STATE: u32 = 3;
const OPEN: f64 = 1.0;
const TRANSFERRED: f64 = 2.0;
const CLOSED: f64 = 3.0;
const EOF: f64 = 4.0;
const PENDING: u32 = 4;
const ROLE: u32 = 5;
const WRITER: f64 = 1.0;

fn get(fields: f64, index: u32) -> f64 {
    crate::array::js_array_get_f64(array_pointer(fields).unwrap(), index)
}
fn set(fields: f64, index: u32, value: f64) {
    crate::array::js_array_set_f64(array_pointer(fields).unwrap(), index, value);
}
fn data(wrapper: f64) -> f64 {
    let pointer = object_pointer(wrapper).unwrap_or_else(|| fail("Expected a guest async value"));
    let fields = unsafe { bag::load(pointer as usize, COMPONENT_GUEST_ENDPOINT) };
    if array_pointer(fields).is_none() {
        fail("Expected a guest async value");
    }
    fields
}
fn live(wrapper: f64) -> f64 {
    let fields = data(wrapper);
    if get(fields, ROLE) == WRITER {
        fail("Expected a guest readable endpoint");
    }
    let owner = get(fields, OWNER);
    if owner.to_bits() != super::maybe_registry().to_bits() {
        fail("Async value belongs to another guest instance");
    }
    if !unsafe { (*context::get(owner)).alive } {
        fail("Guest instance has been disposed");
    }
    if get(fields, STATE) != OPEN {
        fail("Guest async value is closed or transferred");
    }
    fields
}

// Match the immutable manifest's imported tN / exported eN identities.
// Alias expansion is structural, but own/borrow resource keys stay nominal.
fn schema(owner: f64, ty: u32) -> Result<Value, &'static str> {
    let key = format!("{}{}", if ty & 1 == 0 { 't' } else { 'e' }, ty / 2);
    let world = unsafe { &(*context::get(owner)).world };
    let schema = producer::expanded_schema(world, &world["types"][key]["ty"], 0)?;
    if !matches!(schema["kind"].as_str(), Some("future" | "stream")) {
        return Err("Unknown guest async value type");
    }
    Ok(schema)
}

fn same_type(owner: f64, actual: u32, expected: u32) -> Result<bool, &'static str> {
    Ok(schema(owner, actual)? == schema(owner, expected)?)
}

fn finalization(owner: f64) -> f64 {
    unsafe {
        bag::load(
            object_pointer(owner).unwrap() as usize,
            COMPONENT_GUEST_ENDPOINT_FINALIZATION,
        )
    }
}
fn register(wrapper: f64, fields: f64) {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(wrapper);
    let fields = scope.root_nanbox_f64(fields);
    let owner = scope.root_nanbox_f64(get(fields.get_nanbox_f64(), OWNER));
    let registry = scope.root_nanbox_f64(finalization(owner.get_nanbox_f64()));
    if JSValue::from_bits(registry.get_nanbox_f64().to_bits()).is_undefined() {
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(collected, 1),
            0,
        ));
        registry.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_finreg_new(crate::value::js_nanbox_pointer(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            )) as i64,
        ));
        unsafe {
            bag::store(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_GUEST_ENDPOINT_FINALIZATION,
                registry.get_nanbox_f64(),
            );
        }
    }
    crate::weakref::js_finreg_register(
        registry.get_nanbox_f64(),
        wrapper.get_nanbox_f64(),
        fields.get_nanbox_f64(),
        wrapper.get_nanbox_f64(),
    );
}

fn wrap(ty: u32, handle: u32) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(super::registry());
    if !unsafe { (*context::get(owner.get_nanbox_f64())).alive } {
        fail("Guest instance has been disposed");
    }
    // Validate before adopting a handle. An unknown generated type retires the
    // component rather than publishing a capability with the wrong drop ABI.
    let schema = schema(owner.get_nanbox_f64(), ty).unwrap_or_else(|message| fail(message));
    let stream = schema["kind"] == "stream";
    drop(schema);
    let fields = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(6) as i64,
    ));
    for (index, value) in [
        owner.get_nanbox_f64(),
        ty as f64,
        handle as f64,
        OPEN,
        0.0,
        0.0,
    ]
    .into_iter()
    .enumerate()
    {
        set(fields.get_nanbox_f64(), index as u32, value);
    }
    let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        bag::store(
            object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST_ENDPOINT,
            fields.get_nanbox_f64(),
        );
    }
    // A future endpoint is a capability, not a Promise/thenable.
    set_field(wrapper.get_nanbox_f64(), b"then", undefined());
    let read = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        if stream {
            crate::fn_info!(io::read, 1)
        } else {
            crate::fn_info!(io::read_future, 0)
        },
        1,
    ));
    crate::closure::js_closure_set_capture_f64(read.get_raw_mut_ptr(), 0, wrapper.get_nanbox_f64());
    crate::object::set_builtin_closure_non_constructable(
        read.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
    );
    set_field(
        wrapper.get_nanbox_f64(),
        b"read",
        crate::value::js_nanbox_pointer(
            read.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    let dispose = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(dispose, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        dispose.get_raw_mut_ptr(),
        0,
        wrapper.get_nanbox_f64(),
    );
    crate::object::set_builtin_closure_non_constructable(
        dispose.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
    );
    set_field(
        wrapper.get_nanbox_f64(),
        b"dispose",
        crate::value::js_nanbox_pointer(
            dispose.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
    register(wrapper.get_nanbox_f64(), fields.get_nanbox_f64());
    wrapper.get_nanbox_f64()
}

// No JS calls, collection or canonical operations occur here. The state is
// shared by explicit disposal, transfer and the eventual finalization job.
fn release(fields: f64) {
    if get(fields, ROLE) == WRITER {
        writer::release(fields);
        return;
    }
    if get(fields, STATE) != OPEN {
        return;
    }
    let owner = get(fields, OWNER);
    if owner.to_bits() != super::maybe_registry().to_bits() {
        return;
    }
    let ctx = context::get(owner);
    if !unsafe { (*ctx).alive } {
        return;
    }
    let identity = (get(fields, TYPE) as u32, get(fields, HANDLE) as u32);
    set(fields, STATE, CLOSED);
    unsafe {
        (*ctx).pending_guest_endpoints.push_back(identity);
    }
}
extern "C" fn collected(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    fields: f64,
) -> f64 {
    release(fields);
    undefined()
}
extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let wrapper = crate::closure::js_closure_get_capture_f64(closure, 0);
    let fields = data(wrapper);
    if get(fields, STATE) == CLOSED || get(fields, STATE) == EOF {
        return undefined();
    }
    let fields = live(wrapper);
    release(fields);
    crate::weakref::js_finreg_unregister(finalization(get(fields, OWNER)), wrapper);
    undefined()
}

fn lower(frame: &mut Frame, slot: u32, ty: u32) -> u32 {
    let fields = live(frame.get(slot));
    if get(fields, PENDING) != 0.0 {
        fail("Cannot transfer a guest async value with a pending read");
    }
    let owner = get(fields, OWNER);
    if !same_type(owner, get(fields, TYPE) as u32, ty).unwrap_or_else(|message| fail(message)) {
        fail("Guest async value type mismatch");
    }
    if frame
        .endpoint_transfers
        .iter()
        .any(|other| data(frame.get(*other)).to_bits() == fields.to_bits())
    {
        fail("Guest async value is transferred more than once");
    }
    let handle = get(fields, HANDLE) as u32;
    frame.endpoint_transfers.push(slot);
    handle
}
pub(super) fn validate_transfers(frame: &Frame) {
    for slot in &frame.endpoint_transfers {
        let fields = live(frame.get(*slot));
        if get(fields, PENDING) != 0.0 {
            fail("Cannot transfer a guest async value with a pending read");
        }
    }
}
pub(super) fn commit_transfers(frame: &mut Frame) {
    // resources::commit has validated the ENTIRE resource/endpoint batch.
    for slot in std::mem::take(&mut frame.endpoint_transfers) {
        let fields = data(frame.get(slot));
        producer::notify(get(fields, TYPE) as u32, get(fields, HANDLE) as u32, 0, 3);
        set(fields, STATE, TRANSFERRED);
    }
}

/// # Safety
/// `pointer` is an open frame on its creating agent. `ty` is the immutable
/// generated type identity and `handle` is an unclaimed canonical reader.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_lift(
    pointer: *mut c_void,
    ty: u32,
    handle: u32,
) -> u32 {
    frame(pointer).guard(|frame| {
        let value = wrap(ty, handle);
        frame.add(value)
    })
}
/// Stage, without consuming, a typed ownership transfer. The existing resource
/// commit entry point atomically commits both resource and endpoint custody.
/// # Safety
/// `pointer` is an open, exclusively borrowed frame; `slot` belongs to it.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_lower(
    pointer: *mut c_void,
    slot: u32,
    ty: u32,
) -> u32 {
    frame(pointer).guard(|frame| lower(frame, slot, ty))
}
/// Pop a queued readable drop. Returns 1 / 0 empty / -1 frame error. C must
/// return from this function before invoking the typed canonical drop helper.
/// No callback table or changed Guest ABI 2 layout is required.
/// # Safety
/// `pointer` is an open frame. Non-null outputs are distinct writable u32s.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_pop(
    pointer: *mut c_void,
    ty: *mut u32,
    handle: *mut u32,
) -> i32 {
    if !ty.is_null() {
        *ty = 0;
    }
    if !handle.is_null() {
        *handle = 0;
    }
    let frame = frame(pointer);
    let result = frame.guard(|_| {
        if ty.is_null() || handle.is_null() || ty == handle {
            fail("Guest endpoint cleanup requires distinct output pointers");
        }
        let ctx = context::get(super::registry());
        if let Some((type_id, reader)) = (*ctx).pending_guest_endpoints.pop_front() {
            *ty = type_id;
            *handle = reader;
            1
        } else {
            0
        }
    });
    if frame.error().is_some() {
        -1
    } else {
        result
    }
}

#[cfg(test)]
mod tests;
