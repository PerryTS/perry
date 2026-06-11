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

thread_local! {
    static ALIAS_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ALIASES: RefCell<Vec<AliasEntry>> = const { RefCell::new(Vec::new()) };
}

static SCANNER_REGISTERED: std::sync::Once = std::sync::Once::new();

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
/// handle. Kept narrow: the inherits pattern in the wild targets the server
/// classes; widen deliberately, with tests, if more show up.
fn is_aliasable_native_class(module: &str, method: &str) -> bool {
    matches!(module, "http" | "https") && matches!(method, "Server" | "createServer")
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
    // Result must be a NaN-boxed small handle.
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

    SCANNER_REGISTERED.call_once(|| {
        crate::gc::gc_register_mutable_root_scanner_named(
            "runtime:native-this-alias",
            scan_alias_roots,
        );
    });
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

/// Shared implementation for the `js_http(s)_server_construct_with_this`
/// externs: dispatch `(module, "Server")` through the registered native
/// http dispatcher with the (up to 2) constructor args, then alias
/// `this_val` to the resulting handle.
unsafe fn construct_native_server_with_this(module: &str, this_val: f64, a0: f64, a1: f64) -> f64 {
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
    let args = [a0, a1];
    let mut len = args.len();
    while len > 0 && args[len - 1].to_bits() == crate::value::TAG_UNDEFINED {
        len -= 1;
    }
    let method = "Server";
    let result = dispatch(
        module.as_ptr(),
        module.len(),
        method.as_ptr(),
        method.len(),
        args.as_ptr(),
        len,
    );

    // Alias `this` → handle (same gates as the .call/.apply arm path).
    let result_jv = JSValue::from_bits(result.to_bits());
    let handle_addr = (result.to_bits() & crate::value::POINTER_MASK) as usize;
    let obj_addr = object_addr_of(this_val);
    if result_jv.is_pointer()
        && crate::value::addr_class::is_small_handle(handle_addr)
        && obj_addr != 0
        && super::is_valid_obj_ptr(obj_addr as *const u8)
        && !crate::closure::is_closure_ptr(obj_addr)
    {
        SCANNER_REGISTERED.call_once(|| {
            crate::gc::gc_register_mutable_root_scanner_named(
                "runtime:native-this-alias",
                scan_alias_roots,
            );
        });
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
    result
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
#[used]
static KEEP_HTTP_SERVER_CONSTRUCT_WITH_THIS: unsafe extern "C" fn(f64, f64, f64) -> f64 =
    js_http_server_construct_with_this;
#[used]
static KEEP_HTTPS_SERVER_CONSTRUCT_WITH_THIS: unsafe extern "C" fn(f64, f64, f64) -> f64 =
    js_https_server_construct_with_this;
