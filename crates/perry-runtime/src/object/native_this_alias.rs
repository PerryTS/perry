//! #4973: util.inherits-era construction over native-module classes.
//!
//! The classic pre-class Node subclass pattern constructs through an
//! explicit-`this` parent call:
//!
//! ```js
//! function testServer() {
//!   http.Server.call(this, () => {});
//!   this.on('connection', ...);
//! }
//! Object.setPrototypeOf(testServer.prototype, http.Server.prototype);
//! const server = new testServer();
//! server.listen(0, cb);
//! ```
//!
//! Perry's `http.Server` is a bound native-module export whose invocation
//! creates a *handle* (a small integer id dispatched through
//! `HANDLE_METHOD_DISPATCH`), not an initialization of `this`. The `.call`
//! return value is discarded by the pattern, so `this` stayed a plain object
//! and every subsequent `server.on(...)` / `server.listen(...)` failed.
//!
//! Fix: when a bound native *class* export is invoked through
//! `Function.prototype.call` / `.apply` with an explicit plain-object `this`,
//! record an alias `this → handle`. `js_native_call_method` consults the
//! alias for object receivers with no own method of that name and forwards
//! the call to the handle, so the instance behaves as the native object.
//!
//! Storage: `ObjectMeta.native_state` names a traced two-slot alias record
//! (forwarding value, composite flag). A runtime stream keeps that same record
//! in its NativeAlias slot. The forwarding value may be a small server handle
//! or an ordinary payload-backed response. These are child edges, never roots;
//! the alias, target, listeners and captured receiver can die as one cycle.

use crate::value::JSValue;

/// Extract a plausible ObjectHeader address from a value that may be
/// NaN-boxed (POINTER_TAG) or a raw i64 pointer bit-cast to f64 (top 16
/// bits zero — the codegen's I64 object convention). 0 = not an object.
fn object_addr_of(value: f64) -> usize {
    let bits = value.to_bits();
    let top = bits >> 48;
    let addr = if top == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top == 0 {
        bits as usize
    } else {
        return 0;
    };
    if crate::value::addr_class::is_above_handle_band(addr) {
        addr
    } else {
        0
    }
}

/// `object_addr_of`, narrowed to a real heap object that is not a closure —
/// the only receivers an explicit-`this` construction can alias.
fn plain_object_addr_of(value: f64) -> Option<usize> {
    let addr = object_addr_of(value);
    (addr != 0
        && super::is_valid_obj_ptr(addr as *const u8)
        && !crate::closure::is_closure_ptr(addr))
    .then_some(addr)
}

/// A native-this alias is one traced two-slot record: forwarding value and
/// composite-dispatch boolean. Both small handles and ordinary payload owners
/// use this encoding. Unlike an id word, it can retain a payload owner without
/// making the owner a global root. Stream records have their own fixed layout.
const ALIAS_SLOTS: u32 = 2;

#[inline]
fn decode_alias_word(word: u64) -> Option<(f64, bool)> {
    if !crate::native_payload::is_payload_state_word(word) {
        return None;
    }
    let addr = (word & crate::value::POINTER_MASK) as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let record = addr as *const crate::array::ArrayHeader;
    unsafe {
        if (*record).length != ALIAS_SLOTS {
            return None;
        }
        let slots = crate::array::array_elements_ptr(record);
        Some((
            f64::from_bits(*slots),
            *slots.add(1) == crate::value::TAG_TRUE,
        ))
    }
}

#[inline]
pub(crate) fn is_alias_word(word: u64) -> bool {
    decode_alias_word(word).is_some()
}

/// The alias an object carries, read from the object itself: its
/// `native_state` word, or, when the runtime also keeps stream state for the
/// object, the alias slot of that state record (`node_stream::state_record`;
/// one word, one owner).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` header (the caller has classified
/// it).
#[inline]
pub(crate) unsafe fn object_alias(obj: *const super::ObjectHeader) -> Option<(f64, bool)> {
    let meta = (*obj).meta;
    if meta.is_null() {
        return None;
    }
    let word = (*meta).native_state;
    decode_alias_word(word)
        .or_else(|| decode_alias_word(crate::node_stream::record_alias_word(word)))
}

/// Look up the forwarding handle for an arbitrary receiver (NaN-boxed or raw
/// pointer value), and whether to dispatch it through the composite
/// dispatcher. None for anything that is not a live ordinary object carrying
/// an alias.
pub(crate) fn alias_handle_for_object(receiver: f64) -> Option<(f64, bool)> {
    let addr = object_addr_of(receiver);
    if addr == 0 {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    unsafe { object_alias(addr as *const super::ObjectHeader) }
}

/// True when `(module, method)` names a native-module class export whose
/// explicit-`this` invocation should alias the receiver to the constructed
/// handle. Kept narrow: the inherits pattern in the wild targets the server
/// classes; widen deliberately, with tests, if more show up.
fn is_aliasable_native_class(module: &str, method: &str) -> bool {
    matches!(module, "http" | "https") && matches!(method, "Server" | "createServer")
}

/// #10454: native classes whose explicit-`this` construction
/// (`Base.call(this, …)`, `super(…)`) must run the constructor through the
/// http dispatcher up front instead of the ordinary call of the bound
/// export. `https` has no distinct `ServerResponse` in Node.
fn is_construct_before_call_native_class(module: &str, method: &str) -> bool {
    module == "http" && method == "ServerResponse"
}

/// Alias `this_arg` to `result` using one traced record on the receiver, when
/// `result` is a NaN-boxed native forwarding value and
/// `this_arg` is a real heap object (not a closure, not another handle).
pub(crate) fn register_this_to_handle_alias(this_arg: f64, result: f64, composite: bool) {
    let result_jv = JSValue::from_bits(result.to_bits());
    if !result_jv.is_pointer() {
        return;
    }
    let Some(obj_addr) = plain_object_addr_of(this_arg) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(obj_addr as *mut super::ObjectHeader);
    let result = scope.root_nanbox_f64(result);
    object.with_mut_ptr(|obj: *mut super::ObjectHeader| unsafe { super::object_meta_ensure(obj) });
    let record = crate::array::js_array_alloc_with_length_exact(ALIAS_SLOTS);
    let record = scope.root_raw_mut_ptr(record);
    record.with_mut_ptr(|r| {
        crate::array::js_array_set_f64(r, 0, result.get_nanbox_f64());
        crate::array::js_array_set_f64(
            r,
            1,
            f64::from_bits(if composite {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            }),
        );
    });
    let installed = object.with_mut_ptr(|obj: *mut super::ObjectHeader| unsafe {
        let meta = (*obj).meta;
        let word = (*meta).native_state;
        let alias = record
            .with_mut_ptr(|r: *mut crate::array::ArrayHeader| {
                crate::value::js_nanbox_pointer(r as i64)
            })
            .to_bits();
        if crate::node_stream::store_record_alias_word(word, alias) {
            return true;
        }
        if word != 0 && !is_alias_word(word) {
            return false;
        }
        (*meta).native_state = alias;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            alias,
        );
        true
    });
    if !installed {
        return;
    }
    // Native state is per receiver, not a fact of its ordinary shape. Move
    // onto an exotic-read lineage so pre-alias cached misses cannot survive
    // construction or be shared with a sibling receiver (#11725). The mark
    // roots its receiver before it allocates.
    object.with_mut_ptr(|obj: *mut super::ObjectHeader| unsafe {
        super::proto_validity::mark_exotic_read_receiver(obj as usize)
    });
}

/// Called from the `Function.prototype.call` / `.apply` arms after the callee
/// returned. Registers `this_arg → result` when the callee is a bound native
/// class export, `this_arg` is a plain heap object, and `result` is a native
/// handle.
pub(crate) fn maybe_alias_explicit_this_construction(callee: f64, this_arg: f64, result: f64) {
    // Callee must be a bound native-module class export.
    let Some((module, method)) =
        (unsafe { super::native_module::bound_native_callable_module_and_method(callee) })
    else {
        return;
    };
    if !is_aliasable_native_class(&module, &method) {
        return;
    }
    register_this_to_handle_alias(this_arg, result, false);
}

/// Construct the aliasable native class `(module, method)` through the http
/// dispatcher and alias `this_val` to the resulting handle. `None` when the
/// pair is not aliasable or `this_val` is not a plain heap object — the
/// caller then proceeds with its ordinary dispatch.
///
/// Shared by the explicit-`this` call path (`Base.call(this, req)`, below)
/// and `super()`'s dynamic-parent path (`js_fetch_or_value_super`, for
/// `class X extends http.ServerResponse` in any heritage shape — http
/// classes are never recognized at HIR-lowering time). Both must construct
/// here rather than invoke the bound export: that ordinary call builds a
/// handle and drops it (#10454).
pub(crate) fn construct_aliased_native_class(
    module: &str,
    method: &str,
    this_val: f64,
    args: &[f64],
) -> Option<f64> {
    let module = super::native_module::normalize_native_module_alias(module);
    if !is_construct_before_call_native_class(module, method) {
        return None;
    }
    plain_object_addr_of(this_val)?;
    Some(unsafe { construct_native_http_class_with_this(module, method, this_val, args) })
}

/// #10454: `http.ServerResponse.call(this, req)` / `.apply(this, [req])` —
/// light-my-request's `lib/response.js` shape. Called from the
/// `Function.prototype.call` / `.apply` arms BEFORE the ordinary call.
///
/// # Safety
/// `rest_ptr`/`rest_len` must describe a valid NaN-boxed argument slice (the
/// arguments after the explicit `this`).
pub(crate) unsafe fn maybe_construct_http_class_with_this(
    callee: f64,
    this_arg: f64,
    rest_ptr: *const f64,
    rest_len: usize,
) -> Option<f64> {
    let (module, method) = super::native_module::bound_native_callable_module_and_method(callee)?;
    let args: &[f64] = if rest_ptr.is_null() || rest_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(rest_ptr, rest_len)
    };
    construct_aliased_native_class(&module, &method, this_arg, args)
}

/// Run the node:stream subclass-init shim for the base named `method`
/// (`Readable`/`Writable`/`Duplex`/`Transform`/`PassThrough`/`Stream`) on
/// `this`, mutating it in place. Shared by `super()`'s dynamic-parent path
/// (`js_fetch_or_value_super`, #10448/#10798) and the explicit-`this`
/// `Base.call(this, opts)` path below (#10454), so every construction shape
/// installs the identical surface. False when `method` is no stream base.
///
/// The node_stream shims are reached through the stream bucket's install
/// (`js_nm_install_stream`), never directly: this runs on the always-linked
/// `Function.prototype.call`/`.apply` path, and a direct call would link all of
/// node_stream into every program. A stream base callee only exists in a
/// program that imports `stream`, which emits that install.
pub(crate) fn run_node_stream_subclass_init(method: &str, this: f64, opts: f64) -> bool {
    super::native_module_registry::nm_stream_subclass_init(method, this, opts)
}

/// The stream bucket's subclass-init entry, installed by `js_nm_install_stream`.
pub(crate) fn node_stream_subclass_init(method: &str, this: f64, opts: f64) -> bool {
    use crate::node_stream as ns;
    match method {
        "Readable" => ns::js_node_stream_readable_subclass_init(this, opts),
        "Writable" => ns::js_node_stream_writable_subclass_init(this, opts),
        "Duplex" => ns::js_node_stream_duplex_subclass_init(this, opts),
        "Transform" => ns::js_node_stream_transform_subclass_init(this, opts),
        "PassThrough" => ns::js_node_stream_passthrough_subclass_init(this, opts),
        "Stream" => ns::js_node_stream_legacy_subclass_init(this),
        _ => return false,
    };
    true
}

/// #10454: `util.inherits(Fn, Readable)` + `Readable.call(this, opts)` — the
/// pre-class stream subclass shape (light-my-request's `lib/request.js`).
/// Stream bases are not handle factories: their surface is installed as own
/// properties on the instance, so there is no handle to alias `this` to. The
/// ordinary call built a fresh, unrelated stream and discarded it, leaving
/// `this` with no `push`/`pipe`. Run the same subclass-init shim `super()`
/// uses directly on the explicit `this` instead.
///
/// Called from the `Function.prototype.call` / `.apply` arms BEFORE the
/// ordinary call; returns `None` (caller proceeds as before) for every other
/// callee or a `this` that is not a plain heap object.
///
/// # Safety
/// `rest_ptr`/`rest_len` must describe a valid NaN-boxed argument slice (the
/// arguments after the explicit `this`, i.e. `opts`).
pub(crate) unsafe fn maybe_run_stream_subclass_init_via_this(
    callee: f64,
    this_arg: f64,
    rest_ptr: *const f64,
    rest_len: usize,
) -> Option<f64> {
    let (module, method) = super::native_module::bound_native_callable_module_and_method(callee)?;
    if super::native_module::normalize_native_module_alias(&module) != "stream" {
        return None;
    }
    plain_object_addr_of(this_arg)?;
    let opts = if rest_len >= 1 && !rest_ptr.is_null() {
        *rest_ptr
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    run_node_stream_subclass_init(&method, this_arg, opts)
        .then(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Property-read forwarding companion to `alias_handle_for_object`: when a
/// by-name read on an aliased object missed every layer (returned
/// undefined), re-dispatch the read against the aliased native handle so
/// `server.address` / `server.listen` read as bound callables — codegen's
/// static `Named("<fn>")` paths read the method as a property value first,
/// then call it. Returns None when the receiver has no alias or the handle
/// dispatcher yields undefined.
pub(crate) fn alias_forward_property_read(receiver: f64, key: &str) -> Option<f64> {
    let (handle_val, composite) = alias_handle_for_object(receiver)?;
    let handle = (handle_val.to_bits() & crate::value::POINTER_MASK) as i64;
    // Primary dispatcher only for the server aliases — see
    // handle_method_dispatch_primary (an id-colliding ext-net socket must not
    // answer for the server).
    let dispatch = if composite {
        super::class_handles::handle_property_dispatch()?
    } else {
        super::class_handles::handle_property_dispatch_primary()?
    };
    let value = unsafe { dispatch(handle, key.as_ptr(), key.len()) };
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

/// Shared implementation for the `js_http(s)_server_construct_with_this` /
/// `js_http_server_response_construct_with_this` externs: dispatch
/// `(module, method)` through the registered native http dispatcher with the
/// given constructor args, then alias `this_val` to the resulting handle.
/// This is the ONLY reliable way to construct one of these native http
/// classes from Rust: `js_native_call_value` on the bound export closure
/// does NOT reach this dispatcher (confirmed empirically, #10454) — it takes
/// a completely different, non-constructing path, so a `super()`/`.call()`
/// hook that tried calling the closure value directly silently produced no
/// handle at all. Route through `JS_NATIVE_HTTP_DISPATCH` directly instead,
/// exactly like the codegen-recognized `http.Server.call(this, …)` path
/// already did (#4973) before this function existed as a shared helper.
unsafe fn construct_native_http_class_with_this(
    module: &str,
    method: &str,
    this_val: f64,
    args: &[f64],
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let ptr = crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return undefined;
    }
    let dispatch: unsafe extern "C" fn(
        *const u8,
        usize,
        *const u8,
        usize,
        *const f64,
        usize,
    ) -> f64 = std::mem::transmute(ptr);
    // Trim trailing undefined padding so the dispatcher's arg
    // classification sees the same arity the source call had.
    let mut len = args.len();
    while len > 0 && args[len - 1].to_bits() == crate::value::TAG_UNDEFINED {
        len -= 1;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_root = scope.root_nanbox_f64(this_val);
    let result = dispatch(
        module.as_ptr(),
        module.len(),
        method.as_ptr(),
        method.len(),
        args.as_ptr(),
        len,
    );
    let result = scope.root_nanbox_f64(result);
    register_this_to_handle_alias(
        this_root.get_nanbox_f64(),
        result.get_nanbox_f64(),
        method == "ServerResponse",
    );
    result.get_nanbox_f64()
}

/// Shared implementation for the `js_http(s)_server_construct_with_this`
/// externs: dispatch `(module, "Server")` through the registered native
/// http dispatcher with the (up to 2) constructor args, then alias
/// `this_val` to the resulting handle.
unsafe fn construct_native_server_with_this(module: &str, this_val: f64, a0: f64, a1: f64) -> f64 {
    construct_native_http_class_with_this(module, "Server", this_val, &[a0, a1])
}

/// #4973: `http.Server.call(this, handler)` — HIR-lowered entry. Constructs
/// the server through the stdlib dispatcher and aliases `this` to the
/// handle so subsequent `this.on(...)` / `server.listen(...)` calls on the
/// plain-object instance forward to the server.
///
/// # Safety
/// FFI entry from generated code; args are NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_construct_with_this(
    this_val: f64,
    a0: f64,
    a1: f64,
) -> f64 {
    construct_native_server_with_this("http", this_val, a0, a1)
}

/// #4973: `https.Server.call(this, ...)` twin of the above.
///
/// # Safety
/// FFI entry from generated code; args are NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_https_server_construct_with_this(
    this_val: f64,
    a0: f64,
    a1: f64,
) -> f64 {
    construct_native_server_with_this("https", this_val, a0, a1)
}

/// Keepalive anchors: the auto-optimize whole-program LLVM rebuild
/// dead-strips `#[no_mangle]` fns referenced only from generated `.o`
/// files. See the `KEEP_JS_FUNCTION_BIND` precedent in closure/dispatch.rs.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_HTTP_SERVER_CONSTRUCT_WITH_THIS: unsafe extern "C" fn(f64, f64, f64) -> f64 =
    js_http_server_construct_with_this;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_HTTPS_SERVER_CONSTRUCT_WITH_THIS: unsafe extern "C" fn(f64, f64, f64) -> f64 =
    js_https_server_construct_with_this;

/// #10454: `http.ServerResponse.prototype`'s own methods (Node's
/// `ServerResponse` + `OutgoingMessage` surface, plus the listener methods the
/// native handle serves). Every name must be one the http handle dispatchers
/// answer (`server_response_method_bytes` in perry-ext-http).
const SERVER_RESPONSE_PROTOTYPE_METHODS: &[&str] = &[
    "setHeader",
    "getHeader",
    "removeHeader",
    "hasHeader",
    "getHeaders",
    "getHeaderNames",
    "appendHeader",
    "setHeaders",
    "writeHead",
    "write",
    "addTrailers",
    "end",
    "flushHeaders",
    "cork",
    "uncork",
    "destroy",
    "setTimeout",
    "writeEarlyHints",
    "writeContinue",
    "writeProcessing",
    "assignSocket",
    "detachSocket",
    "pipe",
    "on",
    "addListener",
    "once",
    "prependOnceListener",
];

/// Body of every `ServerResponse.prototype.<method>`: resolve `this` to its
/// native handle — the receiver itself (`new ServerResponse(req)`), or the
/// handle a `ServerResponse.call(this, req)` / `super(req)` aliased it to —
/// and dispatch the method on it. This is what makes `util.inherits` /
/// `setPrototypeOf` / `extends` subclasses see the methods through the
/// prototype chain, and what `ServerResponse.prototype.writeHead.apply(this,
/// args)` (light-my-request's override shape) calls.
extern "C" fn server_response_prototype_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8;
    let name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
    let response_scope = crate::gc::RuntimeHandleScope::new();
    let receiver_root = response_scope.root_nanbox_f64(this.as_f64());
    let receiver = receiver_root.get_nanbox_f64();
    let receiver_jv = JSValue::from_bits(receiver.to_bits());
    let receiver_is_handle = receiver_jv.is_pointer()
        && crate::value::addr_class::is_small_handle(
            (receiver.to_bits() & crate::value::POINTER_MASK) as usize,
        );
    let receiver_is_response = plain_object_addr_of(receiver).is_some_and(|a| unsafe {
        (*(a as *const super::ObjectHeader)).class_id
            == crate::native_class_ids::HTTP_SERVER_RESPONSE
    });
    let (handle_val, composite) = if receiver_is_handle || receiver_is_response {
        (receiver, true)
    } else if let Some(alias) = alias_handle_for_object(receiver) {
        alias
    } else {
        let name = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len))
        };
        let msg = format!(
            "ServerResponse.prototype.{name} called on an object that is not a ServerResponse"
        );
        super::object_ops::throw_object_type_error(msg.as_bytes());
    };
    let target_root = response_scope.root_nanbox_f64(handle_val);
    let dispatch = if composite {
        super::class_handles::handle_method_dispatch()
    } else {
        super::class_handles::handle_method_dispatch_primary()
    };
    let args = [arg0, arg1, arg2];
    let mut len = args.len();
    while len > 0 && args[len - 1].to_bits() == crate::value::TAG_UNDEFINED {
        len -= 1;
    }
    let result = match dispatch {
        Some(dispatch) => unsafe {
            dispatch(
                (handle_val.to_bits() & crate::value::POINTER_MASK) as i64,
                name_ptr,
                name_len,
                args.as_ptr(),
                len,
            )
        },
        None => undefined,
    };
    // Chainable methods (`setHeader`, `writeHead`, …) return the handle;
    // hand the aliasing object back instead, as Node returns `this`.
    if result.to_bits() == target_root.get_nanbox_f64().to_bits() {
        receiver_root.get_nanbox_f64()
    } else {
        result
    }
}

extern "C" fn response_property_get(
    c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    unsafe { response_property_dispatch(c, this.as_f64(), None) }
}
extern "C" fn response_property_set(
    c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    unsafe { response_property_dispatch(c, this.as_f64(), Some(value)) }
}
unsafe fn response_property_dispatch(
    c: *const crate::closure::ClosureHeader,
    receiver: f64,
    value: Option<f64>,
) -> f64 {
    let target = alias_handle_for_object(receiver).map_or(receiver, |a| a.0);
    let name = crate::closure::js_closure_get_capture_ptr(c, 0) as *const u8;
    let len = crate::closure::js_closure_get_capture_ptr(c, 1) as usize;
    let h = (target.to_bits() & crate::value::POINTER_MASK) as i64;
    if let Some(v) = value {
        if let Some(f) = super::class_handles::handle_property_set_dispatch() {
            f(h, name, len, v);
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        super::class_handles::handle_property_dispatch()
            .map_or(f64::from_bits(crate::value::TAG_UNDEFINED), |f| {
                f(h, name, len)
            })
    }
}

/// #10454: give `http.ServerResponse` a `.prototype` carrying its methods, so
/// the classic subclass shapes inherit them. Called from the http module's
/// callable-export attach hook when the bound constructor is minted.
pub(crate) fn attach_http_server_response_prototype(constructor_value: f64) -> f64 {
    use super::{
        define_builtin_data_property, set_bound_native_closure_name, set_builtin_closure_length,
        set_builtin_property_attrs, PropertyAttrs,
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer()
        || !crate::closure::is_closure_ptr(
            (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        )
    {
        return constructor_value;
    }
    let proto = super::js_object_alloc(0, 0);
    if proto.is_null() {
        return constructor.get_nanbox_f64();
    }
    let proto = scope.root_raw_mut_ptr(proto);
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    proto.with_mut_ptr(|proto_ptr| {
        define_builtin_data_property(
            proto_ptr,
            key,
            constructor.get_nanbox_f64(),
            "constructor".to_string(),
            PropertyAttrs::new(true, false, true),
        );
    });
    let info = crate::fn_info!(server_response_prototype_method_thunk, 3; with_declared(3));
    for method in SERVER_RESPONSE_PROTOTYPE_METHODS {
        let method_closure = crate::closure::js_closure_alloc(info, 2);
        if method_closure.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method_closure, 0, method.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method_closure, 1, method.len() as i64);
        let method_closure = scope.root_raw_mut_ptr(method_closure);
        method_closure
            .with_mut_ptr(|closure_ptr| set_bound_native_closure_name(closure_ptr, method));
        method_closure.with_mut_ptr(|closure_ptr: *mut u8| {
            set_builtin_closure_length(closure_ptr as usize, 0)
        });
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        proto.with_mut_ptr(|proto_ptr| {
            method_closure.with_mut_ptr(|closure_ptr: *mut u8| {
                define_builtin_data_property(
                    proto_ptr,
                    key,
                    crate::value::js_nanbox_pointer(closure_ptr as i64),
                    (*method).to_string(),
                    PropertyAttrs::new(true, false, true),
                );
            });
        });
    }
    for name in [
        "statusCode",
        "statusMessage",
        "headersSent",
        "writableEnded",
        "writableFinished",
        "finished",
        "destroyed",
        "sendDate",
        "strictContentLength",
        "socket",
        "connection",
        "req",
        "writableLength",
        "writableHighWaterMark",
        "writableCorked",
        "writableObjectMode",
        "writableNeedDrain",
    ] {
        let get = crate::closure::js_closure_alloc(crate::fn_info!(response_property_get, 0), 2);
        let get = scope.root_raw_mut_ptr(get);
        let set = crate::closure::js_closure_alloc(crate::fn_info!(response_property_set, 1), 2);
        let set = scope.root_raw_mut_ptr(set);
        for c in [&get, &set] {
            c.with_mut_ptr(|c| {
                crate::closure::js_closure_set_capture_ptr(c, 0, name.as_ptr() as i64);
                crate::closure::js_closure_set_capture_ptr(c, 1, name.len() as i64);
            });
        }
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        proto.with_mut_ptr(|p| unsafe {
            super::install_own_builtin_accessor(
                p,
                key,
                name,
                get.with_mut_ptr(|c: *mut u8| crate::value::js_nanbox_pointer(c as i64))
                    .to_bits(),
                set.with_mut_ptr(|c: *mut u8| crate::value::js_nanbox_pointer(c as i64))
                    .to_bits(),
                PropertyAttrs::new(true, false, true),
            );
        });
    }
    let closure_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    proto.with_mut_ptr(|proto_ptr: *mut u8| {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "prototype",
            crate::value::js_nanbox_pointer(proto_ptr as i64),
        );
    });
    let closure_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        PropertyAttrs::new(true, false, false),
    );
    constructor.get_nanbox_f64()
}

#[cfg(test)]
mod tests;
