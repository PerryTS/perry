//! Minimal node:domain surface on an ordinary native-payload object.

use perry_runtime::array::{js_array_alloc, js_array_length, js_array_push_f64, ArrayHeader};
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::native_payload::{self, NativePayloadFamily, PayloadPrototype};
use perry_runtime::object::ObjectHeader;
use perry_runtime::string::{js_string_from_bytes, StringHeader};
use perry_runtime::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue};
use std::cell::{Cell, RefCell};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

#[derive(Default)]
struct DomainPayload;

static DOMAIN_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: perry_runtime::native_class_ids::DOMAIN,
    name: "Domain",
    constructor_export: Some(("domain", "Domain")),
    constructor_length: 0,
    links_owner: false,
    install_prototype: install_domain_prototype,
};

thread_local! {
    static ACTIVE_DOMAINS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_DOMAINS_TOUCHED: Cell<bool> = const { Cell::new(false) };
    static DOMAIN_GC_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}
fn js_bool(v: bool) -> f64 {
    f64::from_bits(JSValue::bool(v).bits())
}
fn receiver_value(receiver: i64) -> f64 {
    js_nanbox_pointer(receiver)
}
fn receiver_raw(this: JsThis) -> i64 {
    js_nanbox_get_pointer(this.as_f64())
}
fn object_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let ptr = value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if !perry_runtime::value::addr_class::is_above_handle_band(ptr as usize) {
        return None;
    }
    let header = unsafe {
        &*((ptr as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
            as *const perry_runtime::gc::GcHeader)
    };
    (header.obj_type == perry_runtime::gc::GC_TYPE_OBJECT).then_some(ptr)
}

unsafe fn object_get(value: f64, key: &str) -> f64 {
    let Some(obj) = object_from_value(value) else {
        return undefined();
    };
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    key.with_const_ptr::<StringHeader, _>(|key| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
        })
    })
}

unsafe fn object_set(value: f64, key: &str, field: f64) {
    let Some(obj) = object_from_value(value) else {
        return;
    };
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let field = scope.root_nanbox_f64(field);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    key.with_const_ptr::<StringHeader, _>(|key| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            perry_runtime::object::js_object_set_field_by_name(obj, key, field.get_nanbox_f64())
        })
    });
}

fn value_as_array(value: f64) -> Option<*mut ArrayHeader> {
    let value = JSValue::from_bits(value.to_bits());
    value
        .is_pointer()
        .then(|| value.as_pointer::<ArrayHeader>() as *mut ArrayHeader)
}

unsafe fn members_array(domain: f64) -> Option<*mut ArrayHeader> {
    value_as_array(object_get(domain, "members"))
}

fn ensure_gc_scanner_registered() {
    DOMAIN_GC_REGISTERED.with(|registered| {
        if !registered.replace(true) {
            perry_runtime::gc::gc_register_mutable_root_scanner_named(
                "stdlib:domain",
                scan_domain_roots_mut,
            );
        }
    });
}

fn scan_domain_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    ACTIVE_DOMAINS.with(|domains| {
        for domain in domains.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(domain);
        }
    });
}

unsafe fn collect_array_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    (0..js_array_length(args))
        .map(|i| perry_runtime::array::js_array_get_f64(args, i))
        .collect()
}

fn enter_domain(domain: f64) {
    ACTIVE_DOMAINS_TOUCHED.with(|t| t.set(true));
    ACTIVE_DOMAINS.with(|stack| stack.borrow_mut().push(domain));
}

fn exit_domain(domain: f64) {
    ACTIVE_DOMAINS_TOUCHED.with(|t| t.set(true));
    ACTIVE_DOMAINS.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(pos) = stack.iter().rposition(|v| v.to_bits() == domain.to_bits()) {
            stack.truncate(pos);
        }
    });
}

fn active_domain_value() -> f64 {
    ACTIVE_DOMAINS
        .with(|s| s.borrow().last().copied())
        .unwrap_or_else(|| {
            if ACTIVE_DOMAINS_TOUCHED.with(Cell::get) {
                undefined()
            } else {
                null()
            }
        })
}

fn active_stack_value() -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let values = ACTIVE_DOMAINS.with(|stack| stack.borrow().clone());
    let values = scope.root_nanbox_f64_slice(&values);
    let mut array = scope.root_nanbox_f64(js_nanbox_pointer(js_array_alloc(0) as i64));
    for domain in values {
        let next = js_array_push_f64(
            js_nanbox_get_pointer(array.get_nanbox_f64()) as *mut ArrayHeader,
            domain.get_nanbox_f64(),
        );
        array = scope.root_nanbox_f64(js_nanbox_pointer(next as i64));
    }
    array.get_nanbox_f64()
}

unsafe fn set_member_domain(member: f64, domain: f64) {
    let Some(obj) = object_from_value(member) else {
        return;
    };
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let member = scope.root_nanbox_f64(js_nanbox_pointer(obj as i64));
    let domain = scope.root_nanbox_f64(domain);
    let descriptor = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 4));
    for (name, mut value) in [
        ("writable", js_bool(true)),
        ("enumerable", js_bool(false)),
        ("configurable", js_bool(true)),
        ("value", undefined()),
    ] {
        if name == "value" {
            value = domain.get_nanbox_f64();
        }
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_set_field_by_name(
            descriptor.get_raw_mut_ptr(),
            key,
            value,
        );
    }
    let key =
        perry_runtime::value::js_nanbox_string(js_string_from_bytes(b"domain".as_ptr(), 6) as i64);
    perry_runtime::object::js_object_define_property(
        member.get_nanbox_f64(),
        key,
        js_nanbox_pointer(descriptor.get_raw_mut_ptr::<ObjectHeader>() as i64),
    );
}

unsafe fn member_domain(member: f64) -> f64 {
    object_get(member, "domain")
}
unsafe fn set_error_field(error: f64, name: &str, value: f64) {
    object_set(error, name, value);
}

unsafe fn annotate_error(domain: f64, error: f64, emitter: f64, bound: f64, thrown: bool) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(domain);
    let error = scope.root_nanbox_f64(error);
    let emitter = scope.root_nanbox_f64(emitter);
    let bound = scope.root_nanbox_f64(bound);
    set_error_field(error.get_nanbox_f64(), "domain", domain.get_nanbox_f64());
    set_error_field(error.get_nanbox_f64(), "domainThrown", js_bool(thrown));
    if emitter.get_nanbox_f64().to_bits() != TAG_UNDEFINED {
        set_error_field(
            error.get_nanbox_f64(),
            "domainEmitter",
            emitter.get_nanbox_f64(),
        );
    }
    if bound.get_nanbox_f64().to_bits() != TAG_UNDEFINED {
        set_error_field(
            error.get_nanbox_f64(),
            "domainBound",
            bound.get_nanbox_f64(),
        );
    }
}

unsafe fn event_method(domain: f64, method: &'static [u8], args: &[f64]) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(domain);
    let args = scope.root_nanbox_f64_slice(args);
    let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    perry_runtime::object::js_native_call_method(
        domain.get_nanbox_f64(),
        method.as_ptr() as *const i8,
        method.len(),
        args.as_ptr(),
        args.len(),
    )
}

unsafe fn emit_domain_event(domain: f64, event: &str, args: &[f64]) -> bool {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(domain);
    let args = scope.root_nanbox_f64_slice(args);
    let event_value = perry_runtime::value::js_nanbox_string(js_string_from_bytes(
        event.as_ptr(),
        event.len() as u32,
    ) as i64);
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(event_value);
    call_args.extend(perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args));
    perry_runtime::value::js_is_truthy(event_method(domain.get_nanbox_f64(), b"emit", &call_args))
        != 0
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_emit_error(
    receiver: i64,
    error: f64,
    emitter: f64,
    domain_thrown: bool,
) -> bool {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(receiver_value(receiver));
    let error = scope.root_nanbox_f64(error);
    annotate_error(
        domain.get_nanbox_f64(),
        error.get_nanbox_f64(),
        emitter,
        undefined(),
        domain_thrown,
    );
    emit_domain_event(domain.get_nanbox_f64(), "error", &[error.get_nanbox_f64()])
}

unsafe fn call_with_domain(domain: f64, callback: f64, args: &[f64]) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(domain);
    let callback = scope.root_nanbox_f64(callback);
    let args = scope.root_nanbox_f64_slice(args);
    enter_domain(domain.get_nanbox_f64());
    let outcome = perry_runtime::exception::catch_js_throw(|| unsafe {
        let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
        perry_runtime::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            perry_runtime::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len(),
        )
    });
    exit_domain(domain.get_nanbox_f64());
    match outcome {
        Ok(result) => result,
        Err(error) => {
            let _ = emit_domain_event(domain.get_nanbox_f64(), "error", &[error]);
            undefined()
        }
    }
}

#[no_mangle]
pub extern "C" fn js_domain_create() -> f64 {
    ensure_gc_scanner_registered();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let events = scope.root_nanbox_f64(js_nanbox_pointer(
        perry_runtime::object::js_object_alloc_null_proto(0, 0) as i64,
    ));
    let members = scope.root_nanbox_f64(js_nanbox_pointer(js_array_alloc(0) as i64));
    native_payload::alloc(
        &DOMAIN_FAMILY,
        DomainPayload,
        std::mem::size_of::<DomainPayload>(),
        &[
            (b"_events", events.get_nanbox_f64()),
            (b"_eventsCount", 2.0),
            (b"_maxListeners", undefined()),
            (b"members", members.get_nanbox_f64()),
        ],
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_run(
    receiver: i64,
    callback: f64,
    args: *const ArrayHeader,
) -> f64 {
    call_with_domain(
        receiver_value(receiver),
        callback,
        &collect_array_args(args),
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_bind(receiver: i64, callback: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(receiver_value(receiver));
    let callback = scope.root_nanbox_f64(callback);
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(domain_bound_wrapper, 1; with_rest(0)),
        2,
    );
    perry_runtime::closure::js_closure_set_capture_f64(closure, 0, domain.get_nanbox_f64());
    perry_runtime::closure::js_closure_set_capture_f64(closure, 1, callback.get_nanbox_f64());
    js_nanbox_pointer(closure as i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_intercept(receiver: i64, callback: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(receiver_value(receiver));
    let callback = scope.root_nanbox_f64(callback);
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(domain_intercept_wrapper, 1; with_rest(0)),
        2,
    );
    perry_runtime::closure::js_closure_set_capture_f64(closure, 0, domain.get_nanbox_f64());
    perry_runtime::closure::js_closure_set_capture_f64(closure, 1, callback.get_nanbox_f64());
    js_nanbox_pointer(closure as i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_add(receiver: i64, member: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(receiver_value(receiver));
    let member = scope.root_nanbox_f64(member);
    let Some(members) = members_array(domain.get_nanbox_f64()) else {
        return undefined();
    };
    let exists = (0..js_array_length(members)).any(|i| {
        perry_runtime::array::js_array_get_f64(members, i).to_bits()
            == member.get_nanbox_f64().to_bits()
    });
    if !exists {
        let members = js_array_push_f64(members, member.get_nanbox_f64());
        object_set(
            domain.get_nanbox_f64(),
            "members",
            js_nanbox_pointer(members as i64),
        );
    }
    set_member_domain(member.get_nanbox_f64(), domain.get_nanbox_f64());
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_remove(receiver: i64, member: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let domain = scope.root_nanbox_f64(receiver_value(receiver));
    let member = scope.root_nanbox_f64(member);
    if let Some(mut members) = members_array(domain.get_nanbox_f64()) {
        if let Some(index) = (0..js_array_length(members)).position(|i| {
            perry_runtime::array::js_array_get_f64(members, i).to_bits()
                == member.get_nanbox_f64().to_bits()
        }) {
            let _ = perry_runtime::array::js_array_splice(
                members,
                index as i32,
                1,
                std::ptr::null(),
                0,
                &mut members,
            );
            object_set(
                domain.get_nanbox_f64(),
                "members",
                js_nanbox_pointer(members as i64),
            );
        }
    }
    if member_domain(member.get_nanbox_f64()).to_bits() == domain.get_nanbox_f64().to_bits() {
        set_member_domain(member.get_nanbox_f64(), null());
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_domain_enter(receiver: i64) -> f64 {
    enter_domain(receiver_value(receiver));
    undefined()
}
#[no_mangle]
pub extern "C" fn js_domain_exit(receiver: i64) -> f64 {
    exit_domain(receiver_value(receiver));
    undefined()
}

extern "C" fn domain_bound_wrapper(closure: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let domain = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 0,
        ));
        let callback = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 1,
        ));
        let this = scope.root_nanbox_f64(this.as_f64());
        let args = collect_array_args(js_nanbox_get_pointer(rest) as *const ArrayHeader);
        enter_domain(domain.get_nanbox_f64());
        let outcome = perry_runtime::exception::catch_js_throw(|| {
            perry_runtime::closure::js_native_call_value(
                callback.get_nanbox_f64(),
                JsThis::from_f64(this.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            )
        });
        exit_domain(domain.get_nanbox_f64());
        match outcome {
            Ok(value) => value,
            Err(error) => {
                let error = scope.root_nanbox_f64(error);
                annotate_error(
                    domain.get_nanbox_f64(),
                    error.get_nanbox_f64(),
                    undefined(),
                    callback.get_nanbox_f64(),
                    false,
                );
                let _ =
                    emit_domain_event(domain.get_nanbox_f64(), "error", &[error.get_nanbox_f64()]);
                undefined()
            }
        }
    }
}

extern "C" fn domain_intercept_wrapper(
    closure: *const ClosureHeader,
    this: JsThis,
    rest: f64,
) -> f64 {
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let domain = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 0,
        ));
        let callback = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 1,
        ));
        let this = scope.root_nanbox_f64(this.as_f64());
        let args = collect_array_args(js_nanbox_get_pointer(rest) as *const ArrayHeader);
        let first = args.first().copied().unwrap_or_else(undefined);
        let first_value = JSValue::from_bits(first.to_bits());
        if !first_value.is_null() && !first_value.is_undefined() {
            let first = scope.root_nanbox_f64(first);
            annotate_error(
                domain.get_nanbox_f64(),
                first.get_nanbox_f64(),
                undefined(),
                callback.get_nanbox_f64(),
                false,
            );
            let _ = emit_domain_event(domain.get_nanbox_f64(), "error", &[first.get_nanbox_f64()]);
            return undefined();
        }
        enter_domain(domain.get_nanbox_f64());
        let args = args.get(1..).unwrap_or(&[]);
        let outcome = perry_runtime::exception::catch_js_throw(|| {
            perry_runtime::closure::js_native_call_value(
                callback.get_nanbox_f64(),
                JsThis::from_f64(this.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            )
        });
        exit_domain(domain.get_nanbox_f64());
        match outcome {
            Ok(value) => value,
            Err(error) => {
                let error = scope.root_nanbox_f64(error);
                annotate_error(
                    domain.get_nanbox_f64(),
                    error.get_nanbox_f64(),
                    undefined(),
                    callback.get_nanbox_f64(),
                    false,
                );
                let _ =
                    emit_domain_event(domain.get_nanbox_f64(), "error", &[error.get_nanbox_f64()]);
                undefined()
            }
        }
    }
}

fn install_domain_prototype(proto: &mut PayloadPrototype) {
    let emitter =
        perry_runtime::object::bound_native_callable_export_value("events", "EventEmitter");
    proto.inherit(perry_runtime::object::js_function_prototype_value_for_read(
        emitter,
    ));
    proto.data("members", undefined(), true, true, true);
    macro_rules! method {
        ($name:literal, $body:ident, $n:tt) => {
            proto.method($name, perry_runtime::fn_info!(
                $body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN)
            ), $n)
        };
    }
    method!("_errorHandler", domain_error_handler_thunk, 2);
    method!("enter", domain_enter_thunk, 0);
    method!("exit", domain_exit_thunk, 0);
    method!("add", domain_add_thunk, 1);
    method!("remove", domain_remove_thunk, 1);
    proto.method("run", perry_runtime::fn_info!(
        domain_run_thunk, 2; with_rest(1), with_declared(1), with_flags(perry_runtime::closure::FN_BUILTIN)
    ), 1);
    method!("intercept", domain_intercept_thunk, 1);
    method!("bind", domain_bind_thunk, 1);
}

extern "C" fn domain_error_handler_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    error: f64,
    _e: f64,
) -> f64 {
    unsafe { js_bool(emit_domain_event(this.as_f64(), "error", &[error])) }
}
extern "C" fn domain_enter_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    js_domain_enter(receiver_raw(this))
}
extern "C" fn domain_exit_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    js_domain_exit(receiver_raw(this))
}
extern "C" fn domain_add_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe { js_domain_add(receiver_raw(this), v) }
}
extern "C" fn domain_remove_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe { js_domain_remove(receiver_raw(this), v) }
}
extern "C" fn domain_run_thunk(_c: *const ClosureHeader, this: JsThis, cb: f64, rest: f64) -> f64 {
    unsafe {
        js_domain_run(
            receiver_raw(this),
            cb,
            js_nanbox_get_pointer(rest) as *const ArrayHeader,
        )
    }
}
extern "C" fn domain_bind_thunk(_c: *const ClosureHeader, this: JsThis, cb: f64) -> f64 {
    unsafe { js_domain_bind(receiver_raw(this), cb) }
}
extern "C" fn domain_intercept_thunk(_c: *const ClosureHeader, this: JsThis, cb: f64) -> f64 {
    unsafe { js_domain_intercept(receiver_raw(this), cb) }
}

pub fn domain_module_property(property: &str) -> Option<f64> {
    match property {
        "_stack" => Some(active_stack_value()),
        "active" => Some(active_domain_value()),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    _args: *const f64,
    _args_len: usize,
) -> f64 {
    let method = if method_ptr.is_null() {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    match method {
        "Domain" | "createDomain" | "create" => js_domain_create(),
        "_stack" => active_stack_value(),
        "active" => active_domain_value(),
        _ => undefined(),
    }
}
