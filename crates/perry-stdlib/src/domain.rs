//! Minimal node:domain surface.

use crate::common::{for_each_handle_mut_of, get_handle, get_handle_mut, register_handle, Handle};
use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_nanbox_get_pointer, js_nanbox_pointer,
    js_string_from_bytes, ArrayHeader, ClosureHeader, JSValue, ObjectHeader, StringHeader,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::Once;

// `events` is feature-gated behind `bundled-events`; when the well-known
// bindings table routes `import 'events'` to perry-ext-events the in-tree
// module may still be compiled into stdlib. Keep the local fast path for
// bundled EventEmitter handles, but fall through to runtime hooks so external
// EventEmitter implementations can participate in node:domain routing.
#[inline]
fn ee_is_event_emitter_handle_hook(handle: Handle) -> bool {
    perry_runtime::object::event_emitter_handle_probe()
        .is_some_and(|probe| unsafe { probe(handle) })
}

#[inline]
fn ee_get_domain_hook(handle: Handle) -> Handle {
    perry_runtime::object::event_emitter_get_domain()
        .map(|get_domain| unsafe { get_domain(handle) })
        .unwrap_or(0)
}

#[inline]
fn ee_set_domain_hook(handle: Handle, domain: Handle) {
    if let Some(set_domain) = perry_runtime::object::event_emitter_set_domain() {
        let _ = unsafe { set_domain(handle, domain) };
    }
}

#[cfg(feature = "bundled-events")]
#[inline]
fn ee_is_event_emitter_handle(handle: Handle) -> bool {
    crate::events::is_event_emitter_handle(handle) || ee_is_event_emitter_handle_hook(handle)
}
#[cfg(not(feature = "bundled-events"))]
#[inline]
fn ee_is_event_emitter_handle(handle: Handle) -> bool {
    ee_is_event_emitter_handle_hook(handle)
}

#[cfg(feature = "bundled-events")]
#[inline]
fn ee_get_domain(handle: Handle) -> Handle {
    if crate::events::is_event_emitter_handle(handle) {
        crate::events::js_event_emitter_get_domain(handle)
    } else {
        ee_get_domain_hook(handle)
    }
}
#[cfg(not(feature = "bundled-events"))]
#[inline]
fn ee_get_domain(handle: Handle) -> Handle {
    ee_get_domain_hook(handle)
}

#[cfg(feature = "bundled-events")]
#[inline]
fn ee_set_domain(handle: Handle, domain: Handle) {
    if crate::events::is_event_emitter_handle(handle) {
        let _ = crate::events::js_event_emitter_set_domain(handle, domain);
    } else {
        ee_set_domain_hook(handle, domain);
    }
}
#[cfg(not(feature = "bundled-events"))]
#[inline]
fn ee_set_domain(handle: Handle, domain: Handle) {
    ee_set_domain_hook(handle, domain);
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

pub struct DomainHandle {
    listeners: HashMap<String, Vec<f64>>,
    members: Vec<f64>,
}

unsafe impl Send for DomainHandle {}
unsafe impl Sync for DomainHandle {}

impl DomainHandle {
    fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            members: Vec::new(),
        }
    }
}

static DOMAIN_GC_REGISTERED: Once = Once::new();
static DOMAIN_WRAPPERS_REGISTERED: Once = Once::new();

thread_local! {
    static ACTIVE_DOMAINS: RefCell<Vec<Handle>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_DOMAINS_TOUCHED: Cell<bool> = const { Cell::new(false) };
}

fn ensure_gc_scanner_registered() {
    DOMAIN_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:domain",
            scan_domain_roots_mut,
        );
    });
}

fn ensure_wrapper_closures_registered() {
    DOMAIN_WRAPPERS_REGISTERED.call_once(|| {
        perry_runtime::closure::js_register_closure_rest(domain_bound_wrapper as *const u8, 0);
        perry_runtime::closure::js_register_closure_rest(domain_intercept_wrapper as *const u8, 0);
    });
}

fn scan_domain_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    for_each_handle_mut_of::<DomainHandle, _>(|domain| {
        for callbacks in domain.listeners.values_mut() {
            for callback in callbacks.iter_mut() {
                visitor.visit_nanbox_f64_slot(callback);
            }
        }
        for member in domain.members.iter_mut() {
            visitor.visit_nanbox_f64_slot(member);
        }
    });
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn nanbox_handle(handle: Handle) -> f64 {
    js_nanbox_pointer(handle)
}

fn handle_from_value(value: f64) -> Handle {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as Handle
    } else if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
        value as Handle
    } else {
        bits as Handle
    }
}

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn event_name_from_value(value: f64) -> Option<*const StringHeader> {
    let ptr = perry_runtime::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn collect_array_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = js_array_length(args) as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(perry_runtime::array::js_array_get_f64(args, i as u32));
    }
    out
}

fn array_from_values(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(0);
    for value in values {
        arr = js_array_push_f64(arr, *value);
    }
    arr
}

fn enter_domain(handle: Handle) {
    ACTIVE_DOMAINS_TOUCHED.with(|touched| touched.set(true));
    ACTIVE_DOMAINS.with(|stack| stack.borrow_mut().push(handle));
}

fn exit_domain(handle: Handle) {
    ACTIVE_DOMAINS_TOUCHED.with(|touched| touched.set(true));
    ACTIVE_DOMAINS.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(pos) = stack.iter().rposition(|candidate| *candidate == handle) {
            stack.truncate(pos);
        }
    });
}

fn active_domain_value() -> f64 {
    if let Some(handle) = ACTIVE_DOMAINS.with(|stack| stack.borrow().last().copied()) {
        return nanbox_handle(handle);
    }
    if ACTIVE_DOMAINS_TOUCHED.with(|touched| touched.get()) {
        undefined()
    } else {
        null()
    }
}

fn active_stack_value() -> f64 {
    let mut arr = js_array_alloc(0);
    ACTIVE_DOMAINS.with(|stack| {
        for handle in stack.borrow().iter().copied() {
            arr = js_array_push_f64(arr, nanbox_handle(handle));
        }
    });
    js_nanbox_pointer(arr as i64)
}

unsafe fn set_error_field(error: f64, name: &str, value: f64) {
    let jsv = JSValue::from_bits(error.to_bits());
    if !jsv.is_pointer() {
        return;
    }
    let obj = jsv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null() || (obj as usize) < 0x10000 {
        return;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_set_field_by_name(obj, key, value);
}

unsafe fn annotate_error(
    handle: Handle,
    error: f64,
    emitter: f64,
    bound: f64,
    domain_thrown: bool,
) {
    set_error_field(error, "domain", nanbox_handle(handle));
    set_error_field(error, "domainThrown", js_bool(domain_thrown));
    if emitter.to_bits() != TAG_UNDEFINED {
        set_error_field(error, "domainEmitter", emitter);
    }
    if bound.to_bits() != TAG_UNDEFINED {
        set_error_field(error, "domainBound", bound);
    }
}

unsafe fn emit_domain_event(handle: Handle, event: &str, args: &[f64]) -> bool {
    let listeners = get_handle::<DomainHandle>(handle)
        .and_then(|domain| domain.listeners.get(event).cloned())
        .unwrap_or_default();
    if listeners.is_empty() {
        return false;
    }
    let receiver = nanbox_handle(handle);
    for listener in listeners {
        let previous_this = perry_runtime::object::js_implicit_this_set(receiver);
        let _ = perry_runtime::closure::js_native_call_value(listener, args.as_ptr(), args.len());
        perry_runtime::object::js_implicit_this_set(previous_this);
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_emit_error(
    handle: Handle,
    error: f64,
    emitter: f64,
    domain_thrown: bool,
) -> bool {
    annotate_error(handle, error, emitter, undefined(), domain_thrown);
    emit_domain_event(handle, "error", &[error])
}

unsafe fn call_with_domain(handle: Handle, callback: f64, args: &[f64]) -> f64 {
    enter_domain(handle);
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let result =
            perry_runtime::closure::js_native_call_value(callback, args.as_ptr(), args.len());
        perry_runtime::exception::js_try_end();
        exit_domain(handle);
        result
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        exit_domain(handle);
        let _ = js_domain_emit_error(handle, err, undefined(), true);
        undefined()
    }
}

#[no_mangle]
pub extern "C" fn js_domain_create() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(DomainHandle::new())
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = string_from_header(event_name_ptr) else {
        return handle;
    };
    if let Some(domain) = get_handle_mut::<DomainHandle>(handle) {
        domain
            .listeners
            .entry(event_name)
            .or_default()
            .push(f64::from_bits(listener_bits as u64));
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    args_ptr: *const ArrayHeader,
) -> f64 {
    let Some(event_name) = string_from_header(event_name_ptr) else {
        return js_bool(false);
    };
    let args = collect_array_args(args_ptr);
    js_bool(emit_domain_event(handle, &event_name, &args))
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_run(
    handle: Handle,
    callback: f64,
    args_ptr: *const ArrayHeader,
) -> f64 {
    let args = collect_array_args(args_ptr);
    call_with_domain(handle, callback, &args)
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_bind(handle: Handle, callback: f64) -> f64 {
    ensure_wrapper_closures_registered();
    let closure = perry_runtime::closure::js_closure_alloc(domain_bound_wrapper as *const u8, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, handle);
    perry_runtime::closure::js_closure_set_capture_f64(closure, 1, callback);
    js_nanbox_pointer(closure as i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_intercept(handle: Handle, callback: f64) -> f64 {
    ensure_wrapper_closures_registered();
    let closure =
        perry_runtime::closure::js_closure_alloc(domain_intercept_wrapper as *const u8, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, handle);
    perry_runtime::closure::js_closure_set_capture_f64(closure, 1, callback);
    js_nanbox_pointer(closure as i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_add(handle: Handle, member: f64) -> Handle {
    if let Some(domain) = get_handle_mut::<DomainHandle>(handle) {
        if !domain
            .members
            .iter()
            .any(|candidate| candidate.to_bits() == member.to_bits())
        {
            domain.members.push(member);
        }
    }
    let member_handle = handle_from_value(member);
    if ee_is_event_emitter_handle(member_handle) {
        ee_set_domain(member_handle, handle);
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_domain_remove(handle: Handle, member: f64) -> Handle {
    if let Some(domain) = get_handle_mut::<DomainHandle>(handle) {
        domain
            .members
            .retain(|candidate| candidate.to_bits() != member.to_bits());
    }
    let member_handle = handle_from_value(member);
    if ee_is_event_emitter_handle(member_handle) && ee_get_domain(member_handle) == handle {
        ee_set_domain(member_handle, 0);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_domain_enter(handle: Handle) -> f64 {
    enter_domain(handle);
    undefined()
}

#[no_mangle]
pub extern "C" fn js_domain_exit(handle: Handle) -> f64 {
    exit_domain(handle);
    undefined()
}

extern "C" fn domain_bound_wrapper(closure: *const ClosureHeader, rest: f64) -> f64 {
    unsafe {
        let handle = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as Handle;
        let callback = perry_runtime::closure::js_closure_get_capture_f64(closure, 1);
        let rest_ptr = js_nanbox_get_pointer(rest) as *const ArrayHeader;
        let args = collect_array_args(rest_ptr);
        call_with_domain(handle, callback, &args)
    }
}

extern "C" fn domain_intercept_wrapper(closure: *const ClosureHeader, rest: f64) -> f64 {
    unsafe {
        let handle = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as Handle;
        let callback = perry_runtime::closure::js_closure_get_capture_f64(closure, 1);
        let rest_ptr = js_nanbox_get_pointer(rest) as *const ArrayHeader;
        let args = collect_array_args(rest_ptr);
        let first = args.first().copied().unwrap_or_else(undefined);
        let first_value = JSValue::from_bits(first.to_bits());
        if !first_value.is_null() && !first_value.is_undefined() {
            annotate_error(handle, first, undefined(), callback, false);
            let _ = emit_domain_event(handle, "error", &[first]);
            return undefined();
        }
        call_with_domain(handle, callback, args.get(1..).unwrap_or(&[]))
    }
}

fn bind_method_value(handle: Handle, name: &'static [u8]) -> f64 {
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    unsafe { js_class_method_bind(nanbox_handle(handle), name.as_ptr(), name.len()) }
}

pub fn is_domain_handle(handle: Handle) -> bool {
    get_handle::<DomainHandle>(handle).is_some()
}

pub unsafe fn dispatch_domain_method(handle: Handle, method: &str, args: &[f64]) -> Option<f64> {
    if !is_domain_handle(handle) {
        return None;
    }
    match method {
        "on" | "addListener" if args.len() >= 2 => {
            let event = event_name_from_value(args[0])?;
            Some(nanbox_handle(js_domain_on(
                handle,
                event,
                args[1].to_bits() as i64,
            )))
        }
        "emit" if !args.is_empty() => {
            let event = event_name_from_value(args[0])?;
            let rest = array_from_values(&args[1..]);
            Some(js_domain_emit(handle, event, rest))
        }
        "run" if !args.is_empty() => {
            let rest = array_from_values(&args[1..]);
            Some(js_domain_run(handle, args[0], rest))
        }
        "bind" if !args.is_empty() => Some(js_domain_bind(handle, args[0])),
        "intercept" if !args.is_empty() => Some(js_domain_intercept(handle, args[0])),
        "add" if !args.is_empty() => Some(nanbox_handle(js_domain_add(handle, args[0]))),
        "remove" if !args.is_empty() => Some(nanbox_handle(js_domain_remove(handle, args[0]))),
        "enter" => Some(js_domain_enter(handle)),
        "exit" => Some(js_domain_exit(handle)),
        _ => None,
    }
}

pub fn dispatch_domain_property(handle: Handle, property: &str) -> Option<f64> {
    if ee_is_event_emitter_handle(handle) && property == "domain" {
        let domain = ee_get_domain(handle);
        return Some(if domain == 0 {
            null()
        } else {
            nanbox_handle(domain)
        });
    }
    if !is_domain_handle(handle) {
        return None;
    }
    match property {
        "members" => get_handle::<DomainHandle>(handle).map(|domain| {
            let mut arr = js_array_alloc(0);
            for member in domain.members.iter().copied() {
                arr = js_array_push_f64(arr, member);
            }
            js_nanbox_pointer(arr as i64)
        }),
        "on" => Some(bind_method_value(handle, b"on")),
        "addListener" => Some(bind_method_value(handle, b"addListener")),
        "emit" => Some(bind_method_value(handle, b"emit")),
        "run" => Some(bind_method_value(handle, b"run")),
        "bind" => Some(bind_method_value(handle, b"bind")),
        "intercept" => Some(bind_method_value(handle, b"intercept")),
        "add" => Some(bind_method_value(handle, b"add")),
        "remove" => Some(bind_method_value(handle, b"remove")),
        "enter" => Some(bind_method_value(handle, b"enter")),
        "exit" => Some(bind_method_value(handle, b"exit")),
        _ => None,
    }
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
    _args_ptr: *const f64,
    _args_len: usize,
) -> f64 {
    let method = if method_ptr.is_null() {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    match method {
        "Domain" | "createDomain" | "create" => nanbox_handle(js_domain_create()),
        "_stack" => active_stack_value(),
        "active" => active_domain_value(),
        _ => undefined(),
    }
}
