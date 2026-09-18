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
//! Storage is a small Vec (alias count is tiny — one per inherits-style
//! server) with a GC root scanner that keeps both the object and the handle
//! value alive and rewrites the object pointer if the GC moves it.

use crate::value::JSValue;
use std::cell::{Cell, RefCell};

struct AliasEntry {
    /// Raw heap address of the user object (`this`). Rewritten by the GC
    /// scanner when the object is evacuated. Keyed by address (not NaN-box
    /// bits) because `this` reaches the runtime both NaN-boxed
    /// (POINTER_TAG) and as a raw i64 pointer bit-cast to f64, depending on
    /// the codegen path.
    obj_addr: usize,
    /// NaN-boxed handle value the object forwards to.
    handle_bits: u64,
}

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

crate::perry_thread_local! {
    static ALIAS_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ALIASES: RefCell<Vec<AliasEntry>> = const { RefCell::new(Vec::new()) };
    // The mutable-root scanner registry is thread-local, so this latch must be too.
    static SCANNER_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

fn ensure_scanner_registered() {
    SCANNER_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        crate::gc::gc_register_mutable_root_scanner_named(
            "runtime:native-this-alias",
            scan_alias_roots,
        );
        registered.set(true);
    });
}

fn scan_alias_roots(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ALIASES.with(|a| {
        for entry in a.borrow_mut().iter_mut() {
            visitor.visit_usize_slot(&mut entry.obj_addr);
            visitor.visit_nanbox_u64_slot(&mut entry.handle_bits);
        }
    });
}

/// Cheap per-call gate for `js_native_call_method`: true only after at least
/// one alias has been registered on this thread.
#[inline]
pub(crate) fn alias_active() -> bool {
    ALIAS_ACTIVE.with(|c| c.get())
}

/// Look up the forwarding handle for an object receiver (NaN-boxed or raw
/// pointer value).
pub(crate) fn alias_handle_for_object(receiver: f64) -> Option<f64> {
    let addr = object_addr_of(receiver);
    if addr == 0 {
        return None;
    }
    ALIASES.with(|a| {
        a.borrow()
            .iter()
            .find(|e| e.obj_addr == addr)
            .map(|e| f64::from_bits(e.handle_bits))
    })
}

/// True when `(module, method)` names a native-module class export whose
/// explicit-`this` invocation should alias the receiver to the constructed
/// handle. Started narrow (the inherits pattern in the wild targeted the
/// server classes); widened for #10454's `http.ServerResponse.call(this,
/// req)` (`util.inherits`/light-my-request's exact shape) — `ServerResponse`
/// is modeled as the same kind of handle factory `Server` is, so the same
/// alias mechanism applies unchanged. Widen further, with tests, if more
/// show up.
fn is_aliasable_native_class(module: &str, method: &str) -> bool {
    matches!(module, "http" | "https")
        && matches!(method, "Server" | "createServer" | "ServerResponse")
}

/// Register `this_arg → result` in the alias table when `result` is a
/// NaN-boxed small native handle and `this_arg` is a real heap object (not a
/// closure, not another handle). Shared by two callers that both reach a
/// bound native class export's construction result with an explicit `this`
/// that is not itself the handle: the `.call`/`.apply` explicit-this path
/// below, and `super()`'s dynamic-parent path
/// (`js_fetch_or_value_super` → `maybe_alias_super_construction`, #10454).
fn register_this_to_handle_alias(this_arg: f64, result: f64) {
    let result_jv = JSValue::from_bits(result.to_bits());
    if !result_jv.is_pointer() {
        return;
    }
    let handle_addr = (result.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_small_handle(handle_addr) {
        return;
    }
    // `this` must be a real heap object (not a closure, not another handle).
    // Accept both the NaN-boxed and the raw-i64-pointer object shapes.
    let obj_addr = object_addr_of(this_arg);
    if obj_addr == 0
        || !super::is_valid_obj_ptr(obj_addr as *const u8)
        || crate::closure::is_closure_ptr(obj_addr)
    {
        return;
    }

    ensure_scanner_registered();
    ALIASES.with(|a| {
        let mut aliases = a.borrow_mut();
        if let Some(existing) = aliases.iter_mut().find(|e| e.obj_addr == obj_addr) {
            existing.handle_bits = result.to_bits();
        } else {
            aliases.push(AliasEntry {
                obj_addr,
                handle_bits: result.to_bits(),
            });
        }
    });
    ALIAS_ACTIVE.with(|c| c.set(true));
}

/// Called from the `Function.prototype.call` / `.apply` arms after the callee
/// returned. Registers `this_arg → result` when the callee is a bound native
/// class export, `this_arg` is a plain heap object, and `result` is a native
/// handle. Kept as a fallback for whatever a caller's ordinary `result` turns
/// out to be; `maybe_construct_http_class_with_this` below is the primary,
/// reliable path and runs BEFORE the ordinary call.
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
    register_this_to_handle_alias(this_arg, result);
}

/// Called from the `Function.prototype.call` / `.apply` arms BEFORE the
/// ordinary call, when the callee is a bound native-module class export and
/// `this_arg` is a plain heap object. When the callee names one of the
/// aliasable http classes (`Server`/`createServer`/`ServerResponse`), builds
/// the real handle via `construct_native_http_class_with_this` directly and
/// returns it, so the ordinary call — which does not reach
/// `JS_NATIVE_HTTP_DISPATCH` for these bound exports and would silently
/// construct nothing — never runs. Mirrors
/// `maybe_run_stream_subclass_init_via_this` below for the same reason: an
/// aliased heritage (`const R = http.ServerResponse; R.call(this, req)`)
/// reaches this generic path even though the primary shape
/// (`http.ServerResponse.call(this, req)`, a literal member expression) is
/// caught earlier, statically, by codegen
/// (`crates/perry-hir/src/lower/expr_call/module_class_static.rs`).
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
    if !is_aliasable_native_class(&module, &method) {
        return None;
    }
    let obj_addr = object_addr_of(this_arg);
    if obj_addr == 0
        || !super::is_valid_obj_ptr(obj_addr as *const u8)
        || crate::closure::is_closure_ptr(obj_addr)
    {
        return None;
    }
    let args: &[f64] = if rest_ptr.is_null() || rest_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(rest_ptr, rest_len)
    };
    Some(construct_native_http_class_with_this(
        &module, &method, this_arg, args,
    ))
}

/// #10454: `class X extends http.ServerResponse` (any heritage shape — HTTP
/// classes are never recognized at HIR-lowering time at all, see
/// `crates/perry-hir/src/lower_decl/class_decl.rs`'s
/// `canonical_native_parent_name`, so `super()` always takes this dynamic
/// `js_fetch_or_value_super` path) left the subclass instance with no
/// `setHeader`/`writeHead`/`end`: the ordinary value-super dispatch calls
/// the bound export as a plain call and drops the result, because
/// `ServerResponse` is modeled as a handle factory rather than an
/// initializer of `this` — the exact problem #4973 already solved for
/// `http.Server.call(this, …)`. Reuses that alias table via
/// `construct_native_http_class_with_this` — NOT `js_native_call_value` on
/// the parent value directly, which (confirmed empirically) does not reach
/// `JS_NATIVE_HTTP_DISPATCH` at all for these bound native class exports and
/// silently constructs nothing — so subsequent `this.setHeader(...)` etc.
/// forward through the existing alias-consulting dispatch
/// (`js_native_call_method`/`alias_forward_property_read`) the same way a
/// `.call(this, …)`-built instance already does.
///
/// `resolved_parent` must already be the value to invoke (the caller
/// resolves staleness — see the Temporal/Intl arms in `fetch_globals.rs` for
/// why `parent_val` itself can be stale for an aliased heritage).
///
/// # Safety
/// `args_ptr`/`args_len` must describe a valid NaN-boxed argument slice.
pub(crate) unsafe fn maybe_alias_super_construction(
    resolved_parent: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let (module, method) =
        super::native_module::bound_native_callable_module_and_method(resolved_parent)?;
    if !is_aliasable_native_class(&module, &method) {
        return None;
    }
    let args: &[f64] = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    construct_native_http_class_with_this(&module, &method, this_box, args);
    Some(f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// #10454: node:stream base classes (`Readable`/`Writable`/`Duplex`/
/// `Transform`/`PassThrough`) reached the same `util.inherits` +
/// `Base.call(this, opts)` gap `maybe_alias_super_construction` fixes for
/// `http.ServerResponse` above, but stream bases aren't handle factories —
/// they're modeled as plain objects whose methods are real own properties
/// installed directly onto the instance (see
/// `crates/perry-runtime/src/node_stream_constructors/builders.rs`'s
/// `install_methods_on_existing_object`). There is no handle to alias `this`
/// TO; the subclass-init shim mutates the object `this` already IS — the
/// same shim `class X extends Base` uses at `super()`
/// (`js_node_stream_*_subclass_init`, mirrored in
/// `js_fetch_or_value_super`'s dynamic-parent arm for #10448). Returns the
/// function to call, or `None` when `(module, method)` isn't one of these
/// five stream bases.
type StreamSubclassInitFn = unsafe extern "C" fn(f64, f64) -> f64;
fn node_stream_subclass_init_fn(module: &str, method: &str) -> Option<StreamSubclassInitFn> {
    if super::native_module::normalize_native_module_alias(module) != "stream" {
        return None;
    }
    // `PassThrough` has no subclass-init shim on `main` yet (added
    // alongside the `super()`-heritage fix, #10448) — omitted here rather
    // than duplicated, to keep this PR's diff scoped to what #10454 tests.
    let f: StreamSubclassInitFn = match method {
        "Readable" => crate::node_stream::js_node_stream_readable_subclass_init,
        "Writable" => crate::node_stream::js_node_stream_writable_subclass_init,
        "Duplex" => crate::node_stream::js_node_stream_duplex_subclass_init,
        "Transform" => crate::node_stream::js_node_stream_transform_subclass_init,
        _ => return None,
    };
    Some(f)
}

/// Called from the `Function.prototype.call` / `.apply` arms BEFORE the
/// ordinary call, when the callee is a bound native-module class export and
/// `this_arg` is a plain heap object. When the callee names a node:stream
/// base class, runs its subclass-init shim on `this_arg` directly — mutating
/// it in place — and returns the result, so the caller skips the ordinary
/// call entirely (which would otherwise construct a fresh, unrelated stream
/// object bound to no subclass override, and discard it: `this_arg` stayed
/// a plain object with none of `Readable`'s methods and `_read`/`_write`/
/// `_transform` were never captured). Returns `None` for every other
/// callee, leaving the caller's existing dispatch unchanged.
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
    let init = node_stream_subclass_init_fn(&module, &method)?;
    let obj_addr = object_addr_of(this_arg);
    if obj_addr == 0
        || !super::is_valid_obj_ptr(obj_addr as *const u8)
        || crate::closure::is_closure_ptr(obj_addr)
    {
        return None;
    }
    let opts = if rest_len >= 1 && !rest_ptr.is_null() {
        *rest_ptr
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    Some(init(this_arg, opts))
}

/// Property-read forwarding companion to `alias_handle_for_object`: when a
/// by-name read on an aliased object missed every layer (returned
/// undefined), re-dispatch the read against the aliased native handle so
/// `server.address` / `server.listen` read as bound callables — codegen's
/// static `Named("<fn>")` paths read the method as a property value first,
/// then call it. Returns None when the receiver has no alias or the handle
/// dispatcher yields undefined.
pub(crate) fn alias_forward_property_read(obj_addr: usize, key: &str) -> Option<f64> {
    if !alias_active() || obj_addr == 0 {
        return None;
    }
    let handle_bits = ALIASES.with(|a| {
        a.borrow()
            .iter()
            .find(|e| e.obj_addr == obj_addr)
            .map(|e| e.handle_bits)
    })?;
    let handle = (handle_bits & crate::value::POINTER_MASK) as i64;
    // Primary dispatcher only — see handle_method_dispatch_primary (an
    // id-colliding ext-net socket must not answer for the server).
    let dispatch = super::class_handles::handle_property_dispatch_primary()?;
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
    let result = dispatch(
        module.as_ptr(),
        module.len(),
        method.as_ptr(),
        method.len(),
        args.as_ptr(),
        len,
    );
    register_this_to_handle_alias(this_val, result);
    result
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

/// #10454: `http.ServerResponse.call(this, req)` — light-my-request's exact
/// shape (`lib/response.js`). HIR-lowered entry, same construct-and-alias
/// pattern as the `Server` pair above, generalized through
/// `construct_native_http_class_with_this`.
///
/// # Safety
/// FFI entry from generated code; args are NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_response_construct_with_this(
    this_val: f64,
    req: f64,
) -> f64 {
    construct_native_http_class_with_this("http", "ServerResponse", this_val, &[req])
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
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_HTTP_SERVER_RESPONSE_CONSTRUCT_WITH_THIS: unsafe extern "C" fn(f64, f64) -> f64 =
    js_http_server_response_construct_with_this;
