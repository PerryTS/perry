//! Async generator births and their object-owned request record.
//!
//! An `async function*` call is born on one agent-lifetime layout: no own
//! properties, the private `[[GeneratorState]]` slot, the async generator
//! brand, and a [[Prototype]] its record holds (`PROTO_ID_RECORD_HELD`, as
//! for sync generators). `next`/`return`/`throw` are inherited from
//! `%AsyncGeneratorPrototype%`; the brand proves the receiver and its record.
//!
//! The record is a runtime closure whose captures are fixed JSValue slots:
//! the compiled step body, the request queue (AsyncGeneratorEnqueue), the
//! active/completed flags and the prototype holder. Queued requests are GC
//! nodes linked from the record, so everything a generator owns is reachable
//! only through the generator itself, or through the settle closure of a step
//! still in flight. No address registry, no per-instance table.

use super::generator::*;
use super::generator_state::{
    birth_prototype_holder, classify_prototype, generator_prototype_bits, record_birth_shape,
    shape_held_birth_shape_for, Holder, PROTO_SLOT, STATE_READ,
};
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::codegen_abi::LayoutWord;
use crate::promise::{
    js_promise_attach_settle_listener, js_promise_new, js_promise_reject, js_promise_resolve,
    js_value_is_promise, Promise, PromiseState,
};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, TAG_UNDEFINED};

/// The intrinsic async-generator brand, carried by all own-key transitions.
/// Sync generators hold `u64::MAX - 2`.
pub(crate) const ASYNC_GENERATOR_STATE_BRAND: u64 = u64::MAX - 3;

/// Record slots. `PROTO_SLOT` (5) is shared with the sync record, so the
/// record-held prototype readers address both kinds alike.
const STEP_SLOT: usize = 0;
const QUEUE_HEAD_SLOT: usize = 1;
const QUEUE_TAIL_SLOT: usize = 2;
const FLAGS_SLOT: usize = 3;
const RECORD_SLOTS: u32 = 6;
/// A request is being served (spec: the generator is executing or awaiting
/// for the front request).
const ACTIVE: u32 = 1;
/// The generator reached `completed` (a `done: true` or rejected step).
const COMPLETED: u32 = 2;

/// Queue node slots: the next node, the request's argument, its promise and
/// its operation.
const NODE_NEXT: usize = 0;
const NODE_ARG: usize = 1;
const NODE_PROMISE: usize = 2;
const NODE_OP: usize = 3;
const NODE_SLOTS: u32 = 4;

/// `%AsyncGeneratorPrototype%` operations (the sync thunks' numbering).
pub(crate) const OP_NEXT: u32 = 0;
pub(crate) const OP_RETURN: u32 = 1;
pub(crate) const OP_THROW: u32 = 2;

crate::perry_thread_local! {
    /// This agent's async generator birth ShapeId: an immutable id, held for
    /// the agent's life like the sync generator's.
    static BIRTH_SHAPE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn birth_shape() -> LayoutWord {
    let shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    if shape != 0 {
        return LayoutWord(shape);
    }
    let shape = record_birth_shape(ASYNC_GENERATOR_STATE_BRAND);
    BIRTH_SHAPE.with(|memo| memo.set(shape.0));
    shape
}

/// The body of the record and of queue nodes: private slot containers that
/// no program can reach or call.
extern "C" fn async_generator_private_body(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn slot_container(slots: u32) -> *mut ClosureHeader {
    let cell = js_closure_alloc(crate::fn_info!(async_generator_private_body, 1), slots);
    // Every slot holds a JSValue for the container's whole life: the tag
    // scanner traces it, and each store takes the exact write barrier.
    unsafe { crate::gc::layout_mark_unknown(cell.cast()) };
    for index in 0..slots as usize {
        // GC_STORE_AUDIT(POINTER_FREE): a newborn's undefined initial slots.
        unsafe { cell.add(1).cast::<u64>().add(index).write(TAG_UNDEFINED) };
    }
    cell
}

#[inline(always)]
unsafe fn slot(cell: *const ClosureHeader, index: usize) -> u64 {
    *cell.add(1).cast::<u64>().add(index)
}

#[inline]
fn set_slot(cell: *const ClosureHeader, index: usize, bits: u64) {
    unsafe { js_generator_set_slot(cell.cast_mut(), index as u32, bits, 0) }
}

/// A rooted record, re-read at every use.
struct Rec<'s>(crate::gc::RuntimeHandle<'s>);

impl Rec<'_> {
    fn ptr(&self) -> *const ClosureHeader {
        js_nanbox_get_pointer(self.0.get_nanbox_f64()) as *const ClosureHeader
    }
    fn get(&self, index: usize) -> u64 {
        unsafe { slot(self.ptr(), index) }
    }
    fn set(&self, index: usize, bits: u64) {
        set_slot(self.ptr(), index, bits)
    }
    fn flags(&self) -> u32 {
        f64::from_bits(self.get(FLAGS_SLOT)) as u32
    }
    fn set_flags(&self, flags: u32) {
        self.set(FLAGS_SLOT, (flags as f64).to_bits())
    }
    fn completed(&self) -> bool {
        self.flags() & COMPLETED != 0
    }
}

fn root_rec<'s>(scope: &'s crate::gc::RuntimeHandleScope, rec: *const ClosureHeader) -> Rec<'s> {
    Rec(scope.root_nanbox_f64(js_nanbox_pointer(rec as i64)))
}

/// The async birth layout once an async generator birth published it, else 0.
#[inline(always)]
pub(super) fn published_birth_shape() -> LayoutWord {
    LayoutWord(BIRTH_SHAPE.with(std::cell::Cell::get))
}

/// The record of a proven live `ObjectHeader` carrying the async brand. On
/// the birth layout it is slot 0; a transitioned instance keeps the brand and
/// its private slot. Allocation-free.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell.
#[inline(always)]
pub(crate) unsafe fn async_generator_record_of_object(
    obj: *const ObjectHeader,
) -> Option<*const ClosureHeader> {
    let shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    if shape == 0 {
        return None;
    }
    let bits = if crate::object::shapes::object_shape_stamp(obj) == LayoutWord(shape) {
        *(obj as *const u8)
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>()
    } else {
        transitioned_record_bits(obj)?
    };
    let record = JSValue::from_bits(bits);
    record
        .is_pointer()
        .then(|| record.as_pointer::<ClosureHeader>() as *const _)
}

#[cold]
#[inline(never)]
pub(super) unsafe fn transitioned_record_bits(obj: *const ObjectHeader) -> Option<u64> {
    if !crate::object::shapes::object_has_brand(obj, ASYNC_GENERATOR_STATE_BRAND) {
        return None;
    }
    Some(
        STATE_READ
            .read(crate::value::js_nanbox_pointer(obj as i64))?
            .to_bits(),
    )
}

/// The record of an async generator instance value, or `None`.
pub(crate) fn async_generator_record(value: f64) -> Option<*const ClosureHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if crate::gc_type_is_not_object!(header.obj_type) {
            return None;
        }
        async_generator_record_of_object(addr as *const ObjectHeader)
    }
}

pub(crate) fn is_async_generator_instance_value(value: f64) -> bool {
    async_generator_record(value).is_some()
}

/// The birth of an `async function*` call: `step` is the compiled step body
/// (`__val`, `__gen_op`), `owner` the generator function (null for a direct
/// class-method body, as for sync births).
#[no_mangle]
pub extern "C" fn js_async_generator_create(step: f64, owner: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let step = scope.root_nanbox_f64(step);
    let owner = if owner.is_null() {
        let state = js_nanbox_get_pointer(step.get_nanbox_f64()) as *const ClosureHeader;
        unsafe { crate::closure::js_closure_alloc_singleton((*state).info) }
    } else {
        owner as *mut ClosureHeader
    };
    let owner = scope.root_raw_const_ptr(owner);
    ensure_generator_intrinsics();
    // GetPrototypeFromConstructor, read from the owner's declared slot
    // without materializing it.
    let holder = owner
        .with_const_ptr::<ClosureHeader, _>(|owner| unsafe { birth_prototype_holder(owner, true) });
    let holder = match holder {
        Holder::Materialize => {
            let proto = owner.with_const_ptr::<ClosureHeader, _>(|owner| {
                generator_function_prototype_of(owner as usize)
            });
            match proto {
                Some(value) => classify_prototype(value, true),
                None => Holder::Record(generator_prototype_bits(true)),
            }
        }
        other => other,
    };
    let (holder_bits, record_held) = match holder {
        Holder::Record(bits) => (scope.root_nanbox_u64(bits), true),
        Holder::Shape(bits) => (scope.root_nanbox_u64(bits), false),
        Holder::Materialize => unreachable!("resolved above"),
    };
    let record = root_rec(&scope, slot_container(RECORD_SLOTS));
    record.set(STEP_SLOT, step.get_nanbox_u64());
    record.set_flags(0);
    record.set(PROTO_SLOT, holder_bits.get_nanbox_u64());
    let (obj, rep, shape) = if record_held {
        let shape = birth_shape();
        let (born, rep) = crate::object::object_alloc_plain_born(1, shape);
        (scope.root_raw_mut_ptr(born), rep, shape)
    } else {
        let born = crate::object::alloc_basic::object_alloc_unpublished(0, 1);
        let obj = scope.root_raw_mut_ptr(born);
        let shape = shape_held_birth_shape_for(
            f64::from_bits(holder_bits.get_nanbox_u64()),
            ASYNC_GENERATOR_STATE_BRAND,
        );
        (obj, None, shape)
    };
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        if rep.is_none() {
            crate::object::shapes::store_kind::premark_plain_ordinary(obj);
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, shape);
        }
        crate::object::store_object_field_slot_layout_deferred(obj, 0, record.0.get_nanbox_u64());
    });
    unsafe {
        let state = js_nanbox_get_pointer(step.get_nanbox_f64()) as *mut ClosureHeader;
        if (*state).capture_count & crate::closure::CAPTURES_THIS_FLAG != 0 {
            // GC_STORE_AUDIT(POINTER_FREE): the lexical receiver is never rebound.
            (*state).capture_count |= crate::closure::NO_THIS_REBIND_FLAG;
        }
    }
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| js_nanbox_pointer(obj as i64))
}

/// `%AsyncGeneratorPrototype%.{next,return,throw}` on a branded receiver:
/// AsyncGeneratorEnqueue, then resume at once when the generator is idle.
pub(crate) fn async_generator_request(record: *const ClosureHeader, op: u32, arg: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let rec = root_rec(&scope, record);
    let arg = scope.root_nanbox_f64(arg);
    let flags = rec.flags();
    if flags & ACTIVE != 0 || rec.get(QUEUE_HEAD_SLOT) != TAG_UNDEFINED {
        let promise = scope.root_raw_mut_ptr(js_promise_new());
        enqueue(&rec, op, arg.get_nanbox_f64(), &promise);
        return promise.with_mut_ptr::<Promise, _>(boxed_promise);
    }
    rec.set_flags(flags | ACTIVE);
    // `.return(v)` Awaits `v` before resuming or closing the generator
    // (AsyncGeneratorUnwrapYieldResumption / AsyncGeneratorAwaitReturn).
    if op == OP_RETURN {
        let out = scope.root_raw_mut_ptr(js_promise_new());
        let completed = out.with_mut_ptr::<Promise, _>(|out| {
            dispatch_return_with_await(&rec, arg.get_nanbox_f64(), out)
        });
        if completed {
            drain_queue(&rec);
        }
        return out.with_mut_ptr::<Promise, _>(boxed_promise);
    }
    let result = scope.root_nanbox_f64(call_step(rec.ptr(), op, arg.get_nanbox_f64()));
    after_step_result(&rec, std::ptr::null_mut(), result.get_nanbox_f64());
    result.get_nanbox_f64()
}

fn enqueue(rec: &Rec<'_>, op: u32, arg: f64, promise: &crate::gc::RuntimeHandle<'_>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arg = scope.root_nanbox_f64(arg);
    let node = scope.root_nanbox_f64(js_nanbox_pointer(slot_container(NODE_SLOTS) as i64));
    let node_ptr = || js_nanbox_get_pointer(node.get_nanbox_f64()) as *const ClosureHeader;
    set_slot(node_ptr(), NODE_ARG, arg.get_nanbox_u64());
    set_slot(
        node_ptr(),
        NODE_PROMISE,
        promise.with_mut_ptr::<Promise, _>(|p| js_nanbox_pointer(p as i64).to_bits()),
    );
    set_slot(node_ptr(), NODE_OP, (op as f64).to_bits());
    let tail = rec.get(QUEUE_TAIL_SLOT);
    if tail == TAG_UNDEFINED {
        rec.set(QUEUE_HEAD_SLOT, node.get_nanbox_u64());
    } else {
        set_slot(
            js_nanbox_get_pointer(f64::from_bits(tail)) as *const ClosureHeader,
            NODE_NEXT,
            node.get_nanbox_u64(),
        );
    }
    rec.set(QUEUE_TAIL_SLOT, node.get_nanbox_u64());
}

/// The front request, removed from the queue. Allocation-free.
fn dequeue(rec: *const ClosureHeader) -> Option<(u32, f64, *mut Promise)> {
    unsafe {
        let head = slot(rec, QUEUE_HEAD_SLOT);
        if head == TAG_UNDEFINED {
            return None;
        }
        let node = js_nanbox_get_pointer(f64::from_bits(head)) as *const ClosureHeader;
        let next = slot(node, NODE_NEXT);
        set_slot(rec, QUEUE_HEAD_SLOT, next);
        if next == TAG_UNDEFINED {
            set_slot(rec, QUEUE_TAIL_SLOT, TAG_UNDEFINED);
        }
        Some((
            f64::from_bits(slot(node, NODE_OP)) as u32,
            f64::from_bits(slot(node, NODE_ARG)),
            js_nanbox_get_pointer(f64::from_bits(slot(node, NODE_PROMISE))) as *mut Promise,
        ))
    }
}

/// Resume the step body with one operation. It returns the promise for this
/// step's IteratorResult (or a rejected promise).
fn call_step(rec: *const ClosureHeader, op: u32, arg: f64) -> f64 {
    let step = f64::from_bits(unsafe { slot(rec, STEP_SLOT) });
    let op = match op {
        OP_NEXT => f64::from_bits(crate::value::TAG_FALSE),
        OP_THROW => f64::from_bits(crate::value::TAG_TRUE),
        _ => 2.0,
    };
    crate::promise::async_step::js_async_generator_resume(step, arg, op)
}

/// Await the `.return(v)` value, then resume the step's return arm with the
/// unwrapped value and settle `out` from its result. Return whether an
/// abrupt PromiseResolve completed synchronously, so the caller continues the
/// same iterative drain rather than recursing.
fn dispatch_return_with_await(rec: &Rec<'_>, arg: f64, out: *mut Promise) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let out = scope.root_raw_mut_ptr(out);
    let arg_promise = match crate::promise::js_promise_resolved_catching(arg) {
        Ok(promise) => promise,
        Err(reason) => {
            let reason = scope.root_nanbox_f64(reason);
            if !rec.completed() {
                let result = call_step(rec.ptr(), OP_THROW, reason.get_nanbox_f64());
                return out.with_mut_ptr(|out| settle_step_result(rec, out, result));
            }
            out.with_mut_ptr(|out| complete_request(rec, out, false, reason.get_nanbox_f64()));
            return true;
        }
    };
    let arg_promise = scope.root_raw_mut_ptr(arg_promise);
    let fulfill = scope.root_raw_mut_ptr(make_record_wrapper(
        crate::fn_info!(async_generator_return_step_fulfill, 1),
        rec,
        &out,
    ));
    let reject = make_record_wrapper(
        crate::fn_info!(async_generator_return_step_reject, 1),
        rec,
        &out,
    );
    arg_promise.with_mut_ptr(|arg_promise| {
        fulfill
            .with_mut_ptr(|fulfill| js_promise_attach_settle_listener(arg_promise, fulfill, reject))
    });
    false
}

/// A runtime closure over (record, out promise).
fn make_record_wrapper(
    info: *const crate::closure::JsFunctionInfo,
    rec: &Rec<'_>,
    out: &crate::gc::RuntimeHandle<'_>,
) -> *mut ClosureHeader {
    let wrapper = js_closure_alloc(info, 2);
    js_closure_set_capture_ptr(wrapper, 0, rec.ptr() as i64);
    out.with_mut_ptr::<Promise, _>(|out| js_closure_set_capture_ptr(wrapper, 1, out as i64));
    wrapper
}

fn wrapper_parts(closure: *const ClosureHeader) -> (*const ClosureHeader, *mut Promise) {
    (
        js_closure_get_capture_ptr(closure, 0) as *const ClosureHeader,
        js_closure_get_capture_ptr(closure, 1) as *mut Promise,
    )
}

extern "C" fn async_generator_return_step_fulfill(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (rec, out) = wrapper_parts(closure);
    let rec = root_rec(&scope, rec);
    let out = scope.root_raw_mut_ptr(out);
    // The return arm runs pending finallys and completes with
    // `{ value, done: true }` (or a finally-overridden completion).
    let result = call_step(rec.ptr(), OP_RETURN, value);
    out.with_mut_ptr(|out| after_step_result(&rec, out, result));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn async_generator_return_step_reject(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (rec, out) = wrapper_parts(closure);
    let rec = root_rec(&scope, rec);
    let out = scope.root_raw_mut_ptr(out);
    if !rec.completed() {
        // The throw arm closes suspendedStart without entering the body, or
        // throws at suspendedYield so catch/finally runs.
        let result = call_step(rec.ptr(), OP_THROW, reason);
        out.with_mut_ptr(|out| after_step_result(&rec, out, result));
    } else {
        out.with_mut_ptr(|out| finish_step(&rec, out, false, reason));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn async_generator_complete_step(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
    fulfilled: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (rec, out) = wrapper_parts(closure);
    let rec = root_rec(&scope, rec);
    finish_step(&rec, out, crate::value::js_is_truthy(fulfilled) != 0, value);
    f64::from_bits(TAG_UNDEFINED)
}

fn after_step_result(rec: &Rec<'_>, out: *mut Promise, result: f64) {
    if settle_step_result(rec, out, result) {
        drain_queue(rec);
    }
}

/// Return whether this step completed without another Await.
fn settle_step_result(rec: &Rec<'_>, out: *mut Promise, result: f64) -> bool {
    if let Some(promise) = promise_ptr(result) {
        crate::promise::mark_rejection_handled(promise);
        match unsafe { (*promise).state } {
            PromiseState::Pending => {
                attach_pending_settle(rec, promise, out);
                return false;
            }
            PromiseState::Fulfilled => {
                complete_request(rec, out, true, unsafe { (*promise).value });
            }
            PromiseState::Rejected => {
                complete_request(rec, out, false, unsafe { (*promise).reason });
            }
        }
    } else {
        complete_request(rec, out, true, result);
    }
    true
}

fn attach_pending_settle(rec: &Rec<'_>, promise: *mut Promise, out: *mut Promise) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise = scope.root_raw_mut_ptr(promise);
    let out = scope.root_raw_mut_ptr(out);
    let completion =
        make_record_wrapper(crate::fn_info!(async_generator_complete_step, 2), rec, &out);
    promise.with_mut_ptr(|promise| {
        crate::promise::step_completion::attach_step_completion(promise, completion)
    });
}

fn drain_queue(rec: &Rec<'_>) {
    // A completed generator may have an arbitrarily long tail of requests
    // that complete synchronously. Drain it without jobs or recursion.
    loop {
        // An empty queue (a serial consumer's every step) ends allocation-free.
        let Some((op, arg, promise)) = dequeue(rec.ptr()) else {
            rec.set_flags(rec.flags() & !ACTIVE);
            return;
        };
        if op == OP_RETURN {
            // Return's argument really is awaited: a suspension, unlike the
            // producer completion that brought us into the drain.
            if dispatch_return_with_await(rec, arg, promise) {
                continue;
            }
            return;
        }
        let step_scope = crate::gc::RuntimeHandleScope::new();
        let out = step_scope.root_raw_mut_ptr(promise);
        let result = call_step(rec.ptr(), op, arg);
        if !out.with_mut_ptr(|out| settle_step_result(rec, out, result)) {
            return;
        }
    }
}

fn finish_step(rec: &Rec<'_>, out: *mut Promise, fulfilled: bool, value: f64) {
    complete_request(rec, out, fulfilled, value);
    drain_queue(rec);
}

/// AsyncGeneratorCompleteStep: settle the front request; Yield / DrainQueue
/// continue with the next queued completion in the same job.
fn complete_request(rec: &Rec<'_>, out: *mut Promise, fulfilled: bool, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let out = scope.root_raw_mut_ptr(out);
    let value = scope.root_nanbox_f64(value);
    note_step_settlement(rec, fulfilled, value.get_nanbox_f64());
    out.with_mut_ptr(|out| settle_out(out, fulfilled, value.get_nanbox_f64()));
}

fn note_step_settlement(rec: &Rec<'_>, fulfilled: bool, value: f64) {
    let completed = !fulfilled || {
        let field = js_object_get_own_field_or_undef(value, b"done".as_ptr(), 4);
        let jv = JSValue::from_bits(field.to_bits());
        !jv.is_undefined() && crate::value::js_is_truthy(field) != 0
    };
    if completed {
        rec.set_flags(rec.flags() | COMPLETED);
    }
}

fn settle_out(out: *mut Promise, fulfilled: bool, value: f64) {
    if out.is_null() {
        return;
    }
    if fulfilled {
        js_promise_resolve(out, value);
    } else {
        js_promise_reject(out, value);
    }
}

fn promise_ptr(value: f64) -> Option<*mut Promise> {
    if js_value_is_promise(value) == 0 {
        return None;
    }
    let ptr = js_nanbox_get_pointer(value) as *mut Promise;
    (!ptr.is_null()).then_some(ptr)
}

fn boxed_promise(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        js_nanbox_pointer(promise as i64)
    }
}

#[cfg(test)]
#[path = "async_generator_state_tests.rs"]
mod tests;
