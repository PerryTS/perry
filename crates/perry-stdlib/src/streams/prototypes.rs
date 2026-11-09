//! RSTREAM R5: the members of `ReadableStream.prototype`,
//! `ReadableStreamDefaultController.prototype` and
//! `ReadableStreamDefaultReader.prototype` are real functions. Each probes
//! `this` through the stream registry: the right kind runs the stream
//! operation, any other receiver gets Node's error (operations that return a
//! promise reject, the rest throw). The three families are the runtime's
//! `external` families: the runtime births each prototype on first
//! observation and calls [`install`], the provider hook this module registers.

use super::*;
use perry_runtime::object::BuiltinConstructorIdentity as Family;

const READABLE_STREAM: u8 = Family::ReadableStream as u8;
const DEFAULT_CONTROLLER: u8 = Family::ReadableStreamDefaultController as u8;
const DEFAULT_READER: u8 = Family::ReadableStreamDefaultReader as u8;

// The runtime's generic define primitives, through the stable C ABI: this
// archive carries its own copy of the runtime's Rust items, so a direct call
// would set that copy's hook and allocate in that copy (see `streams.rs`).
#[allow(improper_ctypes)]
extern "C" {
    fn js_runtime_provide_external_families(
        birth: extern "C" fn(u8, *mut ObjectHeader),
        construct: extern "C" fn(u8, f64) -> f64,
    );
    fn js_builtin_define_method(
        target: *mut ObjectHeader,
        name_ptr: *const u8,
        name_len: usize,
        info: *const JsFunctionInfo,
        arity: u32,
        attr_bits: u8,
    ) -> f64;
    fn js_builtin_define_getter(
        target: *mut ObjectHeader,
        name_ptr: *const u8,
        name_len: usize,
        info: *const JsFunctionInfo,
        enumerable: bool,
    );
    fn js_builtin_define_symbol_method(
        target: *mut ObjectHeader,
        symbol_ptr: *const u8,
        symbol_len: usize,
        value: f64,
    );
    fn js_builtin_prototype_method_value(
        builtin_ptr: *const u8,
        builtin_len: usize,
        method_ptr: *const u8,
        method_len: usize,
    ) -> f64;
}

mod protos {
    use super::*;
    /// `{ writable, enumerable, configurable }`: WebIDL operations and
    /// `ReadableStream.from` are enumerable.
    const OPERATION: u8 = 0x07;
    /// A prototype method; returns the installed function.
    pub(super) fn method(
        target: *mut ObjectHeader,
        name: &str,
        info: *const JsFunctionInfo,
        arity: u32,
    ) -> f64 {
        unsafe {
            js_builtin_define_method(target, name.as_ptr(), name.len(), info, arity, OPERATION)
        }
    }
    /// A prototype getter: `{ get, enumerable, configurable }`.
    pub(super) fn getter(proto: *mut ObjectHeader, name: &str, info: *const JsFunctionInfo) {
        unsafe { js_builtin_define_getter(proto, name.as_ptr(), name.len(), info, true) }
    }
    /// `[Symbol.asyncIterator]`: the same function object as `value`.
    pub(super) fn async_iterator(proto: *mut ObjectHeader, value: f64) {
        let symbol = "asyncIterator";
        unsafe { js_builtin_define_symbol_method(proto, symbol.as_ptr(), symbol.len(), value) }
    }
    /// The constructor of a family prototype being born.
    pub(super) fn constructor(proto: *mut ObjectHeader) -> *mut ObjectHeader {
        let key = js_string_from_bytes(b"constructor".as_ptr(), 11);
        let ctor = js_object_get_field_by_name(proto, key);
        perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(ctor.bits()))
            as *mut ObjectHeader
    }
    /// `name` read off `family`'s prototype (births it), when it holds one.
    pub(super) fn family_member(family: u8, name: &str) -> Option<f64> {
        let interface = match family {
            READABLE_STREAM => "ReadableStream",
            DEFAULT_CONTROLLER => "ReadableStreamDefaultController",
            DEFAULT_READER => "ReadableStreamDefaultReader",
            _ => return None,
        };
        let value = unsafe {
            js_builtin_prototype_method_value(
                interface.as_ptr(),
                interface.len(),
                name.as_ptr(),
                name.len(),
            )
        };
        (value.to_bits() != TAG_UNDEFINED).then_some(value)
    }
}

const KIND_READABLE: u8 = 1;
const KIND_READER: u8 = 3;
const KIND_DEFAULT_CONTROLLER: u8 = 6;

fn kind_of(this: f64) -> u8 {
    js_stream_handle_kind(stream_id(this))
}

/// `this` when it is a stream handle of `kind`, else Node's `ERR_INVALID_THIS`.
unsafe fn receiver(this: JsThis, kind: u8, interface: &str) -> f64 {
    let this = this.as_f64();
    if kind_of(this) != kind {
        throw_type_error_with_code(
            &format!("Value of \"this\" must be of type {interface}"),
            "ERR_INVALID_THIS",
        );
    }
    this
}

/// A promise operation's receiver check: a rejected promise on a wrong `this`.
unsafe fn promise_op(this: JsThis, kind: u8, interface: &str, op: impl FnOnce(f64) -> f64) -> f64 {
    let this = this.as_f64();
    if kind_of(this) == kind {
        return op(this);
    }
    let promise = js_promise_new();
    let err = make_type_error_with_code(
        &format!("Value of \"this\" must be of type {interface}"),
        "ERR_INVALID_THIS",
    );
    js_promise_reject(promise, f64::from_bits(err));
    box_promise(promise)
}

unsafe fn call(this: f64, method: &str, args: &[f64]) -> f64 {
    dispatch_stream_method(this, method, args).unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

/// The controller's receiver check: Node throws a plain TypeError there.
unsafe fn controller(this: JsThis) -> f64 {
    let this = this.as_f64();
    if kind_of(this) != KIND_DEFAULT_CONTROLLER {
        throw_type_error("Value of \"this\" must be of type ReadableStreamDefaultController");
    }
    this
}

const RS: &str = "ReadableStream";
const RSDR: &str = "ReadableStreamDefaultReader";

extern "C" fn rs_locked(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { js_readable_stream_locked(receiver(this, KIND_READABLE, RS)) }
}
extern "C" fn rs_cancel(_: *const ClosureHeader, this: JsThis, reason: f64) -> f64 {
    unsafe { promise_op(this, KIND_READABLE, RS, |s| call(s, "cancel", &[reason])) }
}
extern "C" fn rs_get_reader(_: *const ClosureHeader, this: JsThis, options: f64) -> f64 {
    unsafe { call(receiver(this, KIND_READABLE, RS), "getReader", &[options]) }
}
extern "C" fn rs_pipe_through(
    _: *const ClosureHeader,
    this: JsThis,
    pair: f64,
    options: f64,
) -> f64 {
    unsafe {
        call(
            receiver(this, KIND_READABLE, RS),
            "pipeThrough",
            &[pair, options],
        )
    }
}
extern "C" fn rs_pipe_to(_: *const ClosureHeader, this: JsThis, dest: f64, options: f64) -> f64 {
    unsafe {
        promise_op(this, KIND_READABLE, RS, |s| {
            call(s, "pipeTo", &[dest, options])
        })
    }
}
extern "C" fn rs_tee(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { call(receiver(this, KIND_READABLE, RS), "tee", &[]) }
}
extern "C" fn rs_values(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { call(receiver(this, KIND_READABLE, RS), "values", &[]) }
}

extern "C" fn ctl_desired_size(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { js_readable_stream_controller_desired_size(controller(this)) }
}
extern "C" fn ctl_close(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { call(controller(this), "close", &[]) }
}
extern "C" fn ctl_enqueue(_: *const ClosureHeader, this: JsThis, chunk: f64) -> f64 {
    unsafe { call(controller(this), "enqueue", &[chunk]) }
}
extern "C" fn ctl_error(_: *const ClosureHeader, this: JsThis, reason: f64) -> f64 {
    unsafe { call(controller(this), "error", &[reason]) }
}

extern "C" fn reader_closed(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        promise_op(this, KIND_READER, RSDR, |r| {
            box_promise(js_reader_closed(r))
        })
    }
}
extern "C" fn reader_cancel(_: *const ClosureHeader, this: JsThis, reason: f64) -> f64 {
    unsafe { promise_op(this, KIND_READER, RSDR, |r| call(r, "cancel", &[reason])) }
}
extern "C" fn reader_read(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { promise_op(this, KIND_READER, RSDR, |r| call(r, "read", &[])) }
}
extern "C" fn reader_release_lock(_: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { call(receiver(this, KIND_READER, RSDR), "releaseLock", &[]) }
}

/// The runtime's lazy birth of one family prototype, in Node's key order.
extern "C" fn install(family: u8, proto: *mut ObjectHeader) {
    match family {
        READABLE_STREAM => {
            protos::getter(proto, "locked", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.locked.<get>")] export crate::streams::prototypes::rs_locked, 0; with_declared(0)));
            protos::method(proto, "cancel", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.cancel")] export crate::streams::prototypes::rs_cancel, 1; with_declared(1)), 0);
            protos::method(proto, "getReader", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.getReader")] export crate::streams::prototypes::rs_get_reader, 1; with_declared(1)), 0);
            protos::method(proto, "pipeThrough", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.pipeThrough")] export crate::streams::prototypes::rs_pipe_through, 2; with_declared(2)), 1);
            protos::method(proto, "pipeTo", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.pipeTo")] export crate::streams::prototypes::rs_pipe_to, 2; with_declared(2)), 1);
            protos::method(proto, "tee", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.tee")] export crate::streams::prototypes::rs_tee, 0; with_declared(0)), 0);
            let values = protos::method(proto, "values", perry_runtime::fn_info!(#[realm("ReadableStream.prototype.values")] export crate::streams::prototypes::rs_values, 0; with_declared(0)), 0);
            protos::async_iterator(proto, values);
            // Node's own key order on the constructor: length, name, prototype, from.
            protos::method(protos::constructor(proto), "from", perry_runtime::fn_info!(#[realm(ReadableStream.from)] export crate::streams::statics::readable_stream_from_static, 1; with_declared(1)), 1);
        }
        DEFAULT_CONTROLLER => {
            protos::getter(proto, "desiredSize", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultController.prototype.desiredSize.<get>")] export crate::streams::prototypes::ctl_desired_size, 0; with_declared(0)));
            protos::method(proto, "close", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultController.prototype.close")] export crate::streams::prototypes::ctl_close, 0; with_declared(0)), 0);
            protos::method(proto, "enqueue", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultController.prototype.enqueue")] export crate::streams::prototypes::ctl_enqueue, 1; with_declared(1)), 0);
            protos::method(proto, "error", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultController.prototype.error")] export crate::streams::prototypes::ctl_error, 1; with_declared(1)), 0);
        }
        DEFAULT_READER => {
            protos::method(proto, "read", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultReader.prototype.read")] export crate::streams::prototypes::reader_read, 0; with_declared(0)), 0);
            protos::method(proto, "releaseLock", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultReader.prototype.releaseLock")] export crate::streams::prototypes::reader_release_lock, 0; with_declared(0)), 0);
            protos::getter(proto, "closed", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultReader.prototype.closed.<get>")] export crate::streams::prototypes::reader_closed, 0; with_declared(0)));
            protos::method(proto, "cancel", perry_runtime::fn_info!(#[realm("ReadableStreamDefaultReader.prototype.cancel")] export crate::streams::prototypes::reader_cancel, 1; with_declared(1)), 0);
        }
        _ => {}
    }
}

/// `new ReadableStreamDefaultReader(stream)` (the one constructible family).
extern "C" fn construct_reader(_family: u8, stream: f64) -> f64 {
    unsafe {
        if kind_of(stream) != KIND_READABLE {
            throw_invalid_arg_type(
                "The \"stream\" argument must be an instance of ReadableStream.",
            );
        }
        js_readable_stream_get_reader(stream)
    }
}

pub(crate) fn install_stream_prototypes() {
    unsafe { js_runtime_provide_external_families(install, construct_reader) };
}

/// The member a stream handle reads off its interface's prototype
/// (`s.getReader === ReadableStream.prototype.getReader`); `None` for kinds
/// without a prototype family here or names it does not hold.
pub(crate) fn prototype_member(kind: u8, name: &str) -> Option<f64> {
    let family = match kind {
        KIND_READABLE => READABLE_STREAM,
        KIND_READER => DEFAULT_READER,
        KIND_DEFAULT_CONTROLLER => DEFAULT_CONTROLLER,
        _ => return None,
    };
    protos::family_member(family, name)
}

#[cfg(test)]
mod tests {
    use super::protos::family_member;
    use super::{DEFAULT_CONTROLLER, DEFAULT_READER, READABLE_STREAM};

    /// After the streams install, each family prototype holds its own
    /// methods (the realm declaration's rows) and nothing of the others.
    #[test]
    fn stream_prototypes_hold_the_declared_keys() {
        crate::test_runtime::initialize();
        super::install_stream_prototypes();
        let rows = [
            (
                READABLE_STREAM,
                &[
                    "cancel",
                    "getReader",
                    "pipeThrough",
                    "pipeTo",
                    "tee",
                    "values",
                ][..],
            ),
            (DEFAULT_CONTROLLER, &["close", "enqueue", "error"][..]),
            (DEFAULT_READER, &["cancel", "read", "releaseLock"][..]),
        ];
        for (family, keys) in rows {
            for key in keys {
                assert!(family_member(family, key).is_some(), "{family:?}.{key}");
            }
            let owns_enqueue = family == DEFAULT_CONTROLLER;
            assert_eq!(
                family_member(family, "enqueue").is_some(),
                owns_enqueue,
                "{family:?}"
            );
        }
    }
}
