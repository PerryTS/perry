//! A C operation owns one traced frame. Its public Promise starts weak and
//! observation upgrades the private row through promise::operation_owner.
//! Canonical I/O, cancellation acknowledgement and typed payload disposal
//! belong to C; no native buffers or managed addresses cross that boundary.
use super::*;
use crate::async_context::{self as als, job_domain};
use perry_abi::component_guest::{ComponentGuestEndpointOps, GUEST_ENDPOINT_ABI_VERSION};
use std::sync::atomic::{AtomicPtr, Ordering};

static OPS: AtomicPtr<ComponentGuestEndpointOps> = AtomicPtr::new(std::ptr::null_mut());
const OBSERVER: u32 = 0;
const FIELDS: u32 = 1;
const SNAPSHOT: u32 = 2;
const PREPARED: u32 = 3;
const INPUT: u32 = 4;

pub(in crate::component::guest) struct Operation {
    row: u32,
    id: u32,
    stream: bool,
    writing: bool,
    committed: bool,
    finished: bool,
}

/// # Safety
/// The descriptor is immutable image data with image-lifetime code pointers.
/// This additive table never changes the published Guest ABI 2 layout.
#[no_mangle]
pub unsafe extern "C" fn js_component_set_guest_endpoint_ops(
    ops: *const ComponentGuestEndpointOps,
) -> i32 {
    if ops.is_null()
        || (*ops).abi_version != GUEST_ENDPOINT_ABI_VERSION
        || (*ops).size != std::mem::size_of::<ComponentGuestEndpointOps>()
    {
        return 0;
    }
    match OPS.compare_exchange(
        std::ptr::null_mut(),
        ops.cast_mut(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 1,
        Err(previous) => i32::from(previous == ops.cast_mut()),
    }
}

pub(super) extern "C" fn read(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    capacity: f64,
) -> f64 {
    begin(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        capacity,
    )
}
pub(super) extern "C" fn read_future(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    begin(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        undefined(),
    )
}

fn settled_eof() -> f64 {
    let scope = RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(
        result.get_nanbox_f64(),
        b"done",
        f64::from_bits(JSValue::bool(true).bits()),
    );
    let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    crate::promise::js_promise_resolve(promise.get_raw_mut_ptr(), result.get_nanbox_f64());
    crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
}

fn begin(wrapper: f64, capacity: f64) -> f64 {
    begin_operation(wrapper, capacity, false)
}
pub(super) fn write(wrapper: f64, input: f64) -> f64 {
    begin_operation(wrapper, input, true)
}
fn begin_operation(wrapper: f64, argument: f64, writing: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(wrapper);
    let argument = scope.root_nanbox_f64(argument);
    let fields = scope.root_nanbox_f64(if writing {
        writer::data(wrapper.get_nanbox_f64())
    } else {
        data(wrapper.get_nanbox_f64())
    });
    let owner = scope.root_nanbox_f64(get(fields.get_nanbox_f64(), OWNER));
    if owner.get_nanbox_f64().to_bits() != super::super::maybe_registry().to_bits()
        || !unsafe { (*context::get(owner.get_nanbox_f64())).alive }
    {
        fail("Guest async value belongs to another or disposed instance");
    }
    if !writing && get(fields.get_nanbox_f64(), STATE) == EOF {
        return settled_eof();
    }
    if writing {
        writer::live(wrapper.get_nanbox_f64());
    } else {
        live(wrapper.get_nanbox_f64());
    }
    if get(fields.get_nanbox_f64(), PENDING) != 0.0 {
        fail("Guest async value already has a pending operation");
    }
    let ty = get(fields.get_nanbox_f64(), TYPE) as u32;
    let schema = schema(owner.get_nanbox_f64(), ty).unwrap_or_else(|message| fail(message));
    let stream = schema["kind"] == "stream";
    drop(schema);
    let capacity = if writing {
        0.0
    } else if JSValue::from_bits(argument.get_nanbox_f64().to_bits()).is_undefined() {
        if stream {
            4096.0
        } else {
            1.0
        }
    } else {
        argument.get_nanbox_f64()
    };
    if !writing
        && (!JSValue::from_bits(capacity.to_bits()).is_number()
            || capacity.fract() != 0.0
            || !(1.0..=65536.0).contains(&capacity)
            || (!stream && capacity != 1.0))
    {
        fail("Invalid guest async read capacity");
    }
    if !job_domain::current().is_some_and(|token| token.is_current_live()) {
        fail("Guest endpoint I/O requires a live canonical task");
    }
    let ops = OPS.load(Ordering::Acquire);
    if writing {
        producer::ops();
    } else if ops.is_null() {
        fail("Guest adapter does not support endpoint I/O");
    }
    let ctx = context::get(owner.get_nanbox_f64());
    let id = unsafe { (*ctx).next_guest_endpoint_operation };
    unsafe {
        (*ctx).next_guest_endpoint_operation = id
            .checked_add(1)
            .unwrap_or_else(|| fail("Guest endpoint operation identity exhausted"));
    }
    set(fields.get_nanbox_f64(), PENDING, id as f64);
    let mut frame = Box::new(Frame::allocate_guest_frame());
    let promise = scope.root_nanbox_f64(undefined());
    let mut adopted = false;
    let outcome = crate::exception::catch_js_throw(|| {
        let saved = scope.root_raw_mut_ptr(als::alloc_snapshot_holder(als::capture_context()));
        promise.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::promise::js_promise_new() as i64,
        ));
        let pointer = || {
            JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut()
        };
        crate::promise::set_promise_context_snapshot(pointer(), unsafe {
            (*saved.get_raw_mut_ptr::<als::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        });
        let weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(promise.get_nanbox_f64()) as i64,
        ));
        let row = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc_with_length(5) as i64,
        ));
        for (index, value) in [
            weak.get_nanbox_f64(),
            fields.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(
                saved.get_raw_mut_ptr::<als::AsyncSnapshotHolder>() as i64
            ),
            undefined(),
            if writing {
                argument.get_nanbox_f64()
            } else {
                undefined()
            },
        ]
        .into_iter()
        .enumerate()
        {
            set(row.get_nanbox_f64(), index as u32, value);
        }
        crate::promise::operation_owner::register(
            pointer(),
            row.get_nanbox_f64(),
            OBSERVER,
            wrapper.get_nanbox_f64(),
        );
        if writing {
            writer::live(wrapper.get_nanbox_f64());
        } else {
            live(wrapper.get_nanbox_f64());
        }
        if !job_domain::current().is_some_and(|token| token.is_current_live()) {
            fail("Guest endpoint task changed during Promise initialization");
        }
        if get(fields.get_nanbox_f64(), PENDING) != id as f64 {
            fail("Guest async value changed during Promise initialization");
        }
        let row = frame.add(row.get_nanbox_f64());
        frame.endpoint_io = Some(Operation {
            row,
            id,
            stream,
            writing,
            committed: false,
            finished: false,
        });
        let handle = get(fields.get_nanbox_f64(), HANDLE) as u32;
        let status = if writing {
            let input = frame.add(argument.get_nanbox_f64());
            unsafe { (producer::ops().write)((&raw mut *frame).cast(), ty, handle, id, input) }
        } else {
            unsafe { ((*ops).operation)((&raw mut *frame).cast(), ty, handle, 0, capacity as u32) }
        };
        adopted = status == 0;
        if let Some(error) = frame.error() {
            crate::exception::js_throw(error);
        }
        if !adopted {
            fail("Guest adapter rejected endpoint I/O without an error");
        }
        promise.get_nanbox_f64()
    });
    if outcome.is_err() {
        if get(fields.get_nanbox_f64(), PENDING) == id as f64 {
            set(fields.get_nanbox_f64(), PENDING, 0.0);
        }
        if crate::promise::js_value_is_promise(promise.get_nanbox_f64()) != 0 {
            crate::promise::operation_owner::clear(
                JSValue::from_bits(promise.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::promise::Promise>()
                    .cast_mut(),
            );
        }
    }
    if adopted {
        let _ = Box::into_raw(frame);
    } else {
        drop(frame);
    }
    finish(outcome)
}

fn row(frame: &Frame) -> f64 {
    let operation = frame
        .endpoint_io
        .as_ref()
        .unwrap_or_else(|| fail("Expected a guest endpoint operation"));
    frame.get(operation.row)
}
fn observer(row: f64) -> f64 {
    // WeakRef's named-slot lookup can allocate its internal key. Keep the
    // frame's unrooted local words stable during this finite read; its normal
    // weak-to-strong barrier still shades an incremental-cycle target.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let value = get(row, OBSERVER);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return undefined();
    }
    if crate::promise::js_value_is_promise(value) != 0 {
        value
    } else {
        crate::weakref::js_weakref_deref(value)
    }
}
fn active(frame: &Frame) -> bool {
    let operation = frame
        .endpoint_io
        .as_ref()
        .unwrap_or_else(|| fail("Expected a guest endpoint operation"));
    if operation.finished {
        return false;
    }
    let row = row(frame);
    let fields = get(row, FIELDS);
    let owner = get(fields, OWNER);
    if !unsafe { (*context::get(owner)).alive }
        || get(fields, PENDING) != operation.id as f64
        || !(get(fields, STATE) == OPEN
            || (operation.writing && operation.stream && get(fields, STATE) == CLOSED))
    {
        return false;
    }
    let snapshot =
        JSValue::from_bits(get(row, SNAPSHOT).to_bits()).as_pointer::<als::AsyncSnapshotHolder>();
    !unsafe { (*snapshot).snapshot.job_is_cancelled() }
        && crate::promise::js_value_is_promise(observer(row)) != 0
}

/// 1 live observer, 0 cancel/retire, -1 frame error. No jobs are driven.
/// # Safety
/// `pointer` is an open operation frame on its creating agent.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_status(pointer: *mut c_void) -> i32 {
    let frame = frame(pointer);
    let result = frame.guard(|frame| i32::from(active(frame)));
    if frame.error().is_some() {
        -1
    } else {
        result
    }
}

/// Pin a still-live observer for one finite typed completion. A zero return
/// means C must discard its owned payload without lifting it into JS.
/// # Safety
/// `pointer` is an open operation frame, called in its originating task.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_prepare(pointer: *mut c_void) -> i32 {
    let frame = frame(pointer);
    let result = frame.guard(|frame| {
        if !active(frame) {
            return 0;
        }
        let row = row(frame);
        set(row, PREPARED, observer(row));
        1
    });
    if frame.error().is_some() {
        -1
    } else {
        result
    }
}

/// Finish once. flags: 1 terminal reader, 2 aborted (payload ignored).
/// C retains ownership of discarded payloads when prepare returned zero.
/// # Safety
/// `pointer` is an open operation frame; value is a rooted slot if not aborted.
/// C must have acknowledged the canonical operation before calling this.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_finish(
    pointer: *mut c_void,
    value: u32,
    flags: u32,
) -> i32 {
    let frame = frame(pointer);
    frame.guard(|frame| complete(frame, value, flags));
    if frame.error().is_some() {
        -1
    } else {
        0
    }
}

fn complete(frame: &mut Frame, value: u32, flags: u32) {
    if flags & !3 != 0 {
        fail("Invalid guest endpoint completion flags");
    }
    let operation = frame
        .endpoint_io
        .as_ref()
        .unwrap_or_else(|| fail("Expected a guest endpoint operation"));
    if operation.finished {
        fail("Guest endpoint operation already completed");
    }
    let (stream, writing, id) = (operation.stream, operation.writing, operation.id);
    let scope = RuntimeHandleScope::new();
    let row = scope.root_nanbox_f64(row(frame));
    let fields = scope.root_nanbox_f64(get(row.get_nanbox_f64(), FIELDS));
    let promise = scope.root_nanbox_f64(if flags & 2 == 0 {
        get(row.get_nanbox_f64(), PREPARED)
    } else {
        observer(row.get_nanbox_f64())
    });
    let owner = get(fields.get_nanbox_f64(), OWNER);
    let alive = unsafe { (*context::get(owner)).alive };
    let snapshot = unsafe {
        (*JSValue::from_bits(get(row.get_nanbox_f64(), SNAPSHOT).to_bits())
            .as_pointer::<als::AsyncSnapshotHolder>())
        .snapshot
        .clone()
    };
    if flags & 2 == 0
        && !snapshot
            .job_domain_token()
            .is_some_and(|token| token.is_current_live())
    {
        drop(snapshot);
        fail("Guest endpoint completed outside its originating task");
    }
    // Commit before hooks can reenter, including rejection and thenable hooks.
    frame.endpoint_io.as_mut().unwrap().finished = true;
    if get(fields.get_nanbox_f64(), PENDING) == id as f64 {
        set(fields.get_nanbox_f64(), PENDING, 0.0);
    }
    if flags & 1 != 0 && get(fields.get_nanbox_f64(), STATE) == OPEN {
        if writing {
            writer::release(fields.get_nanbox_f64());
        } else {
            release(fields.get_nanbox_f64());
        }
        if stream && !writing {
            set(fields.get_nanbox_f64(), STATE, EOF);
        }
    }
    set(row.get_nanbox_f64(), OBSERVER, undefined());
    set(row.get_nanbox_f64(), PREPARED, undefined());
    if !alive || crate::promise::js_value_is_promise(promise.get_nanbox_f64()) == 0 {
        drop(snapshot);
        return;
    }
    let pointer = || {
        JSValue::from_bits(promise.get_nanbox_f64().to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut()
    };
    crate::promise::operation_owner::clear(pointer());
    let previous = als::enter_context(&snapshot);
    drop(snapshot);
    als::push_context_guard(als::ContextGuardAction::RestoreSnapshot(previous));
    if flags & 2 != 0 {
        let text = if writing {
            b"Guest endpoint write cancelled".as_slice()
        } else {
            b"Guest endpoint read cancelled".as_slice()
        };
        let message = scope.root_string_ptr(crate::string::js_string_from_bytes(
            text.as_ptr(),
            text.len() as u32,
        ));
        let error =
            crate::error::js_error_new_with_name_message(b"AbortError", message.get_raw_mut_ptr());
        crate::promise::js_promise_reject(pointer(), crate::value::js_nanbox_pointer(error as i64));
    } else {
        crate::promise::assimilate::promise_resolve_assimilating(pointer(), frame.get(value));
    }
    if let Some(action) = als::pop_context_guard() {
        als::apply_context_guard(action);
    }
}

/// Commit a complete typed write batch, then sever all temporary conversion
/// edges. C now owns the native payload, so its frame must not accidentally
/// retain an unobserved Promise through arbitrary input expandos/getters.
/// # Safety
/// Called once before canonical I/O on a freshly lowered write frame. All
/// input/conversion slots become invalid; only the private operation row stays.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_write_commit(pointer: *mut c_void) -> i32 {
    let frame = frame(pointer);
    frame.guard(|frame| {
        let operation = frame
            .endpoint_io
            .as_ref()
            .unwrap_or_else(|| fail("Expected a guest write operation"));
        if !operation.writing
            || operation.finished
            || operation.committed
            || !frame.borrows.is_empty()
            || !frame.committed_borrows.is_empty()
        {
            fail("Invalid guest write custody commit");
        }
        let slot = operation.row;
        super::super::resources::commit(frame);
        frame.endpoint_io.as_mut().unwrap().committed = true;
        set(frame.get(slot), INPUT, undefined());
        frame.clear_conversion_slots_except(slot);
    });
    if frame.error().is_some() {
        -1
    } else {
        0
    }
}

pub(in crate::component::guest) fn retire(frame: &mut Frame) {
    let Some(operation) = &frame.endpoint_io else {
        return;
    };
    if operation.finished {
        return;
    }
    let id = operation.id;
    let row = row(frame);
    let fields = get(row, FIELDS);
    if get(fields, PENDING) == id as f64 {
        set(fields, PENDING, 0.0);
    }
    let promise = observer(row);
    if crate::promise::js_value_is_promise(promise) != 0 {
        crate::promise::operation_owner::clear(
            JSValue::from_bits(promise.to_bits())
                .as_pointer::<crate::promise::Promise>()
                .cast_mut(),
        );
    }
    set(row, OBSERVER, undefined());
    set(row, PREPARED, undefined());
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_ENDPOINT_OPS: unsafe extern "C" fn(*const ComponentGuestEndpointOps) -> i32 =
    js_component_set_guest_endpoint_ops;

#[cfg(test)]
mod tests;
