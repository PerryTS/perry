//! `js_instanceof` — the static (compile-time-known-class-id) form of
//! `instanceof`, and the per-built-in dispatch ladder behind it.
//!
//! Split out of `instanceof.rs` for the file-size cap. Pure relocation —
//! no logic changes; see `super::*` for every helper this calls.

use super::*;

#[no_mangle]
pub extern "C" fn js_instanceof(value: f64, class_id: u32) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let true_val = f64::from_bits(TAG_TRUE);
    let false_val = f64::from_bits(TAG_FALSE);

    if class_id == 0 {
        return false_val;
    }
    // `proxy instanceof C` follows the proxy's prototype chain, which forwards
    // to the target (absent a `getPrototypeOf` trap) — so unwrap to the target
    // before walking the class chain. The proxy is a small id with no chain of
    // its own. (drizzle's aliased-column proxies + `is(value, type)`.)
    let mut value = value;
    {
        let mut depth = 0;
        while depth < 16 && crate::proxy::js_proxy_is_proxy(value) != 0 {
            value = crate::proxy::js_proxy_target(value);
            depth += 1;
        }
    }
    // User-defined `Symbol.hasInstance` takes precedence over the built-in
    // prototype-chain walk — and over the ordinary class-chain fast path below.
    // `new C() instanceof C` must run a class-level `@@hasInstance` rather than
    // short-circuit on the chain (the hook can return `false` for a real
    // instance), so both hook forms are consulted here, ahead of that walk.
    //
    // Form 1: the HIR lifts `static [Symbol.hasInstance](v)` to a top-level
    // function `__perry_wk_hasinstance_<class>` and the LLVM backend registers a
    // pointer to it against the class id at module init.
    if let Some(func_ptr) = lookup_has_instance_hook(class_id) {
        let hook: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(func_ptr as *const u8) };
        let result = hook(value);
        // Normalize: any truthy NaN-boxed bool stays as the TAG_TRUE/FALSE
        // sentinel. User-written `return typeof v === "number" && ...`
        // already returns a NaN-boxed bool, so this is usually a no-op.
        let rbits = result.to_bits();
        if rbits == TAG_TRUE || rbits == TAG_FALSE {
            return result;
        }
        // Fallback: treat as truthy → TRUE, zero/undefined → FALSE.
        if result.is_nan() && rbits & 0xFFFF_0000_0000_0000 == 0x7FFC_0000_0000_0000 {
            return false_val;
        }
        if result == 0.0 || result.is_nan() {
            return false_val;
        }
        return true_val;
    }

    // Form 2: the `Object.defineProperty(C, Symbol.hasInstance, { value: fn })`
    // form (zod 4) stores the closure in the class static-symbol table. Read it
    // off the class id (OWN lookup only — never resolves Function.prototype's
    // default @@hasInstance thunk, so no recursion). A present-but-non-callable
    // value throws; only `null`/`undefined` falls through to the chain.
    //
    // The latch check is what keeps `well_known_symbol("hasInstance")` — a
    // string-keyed interning probe — off the path entirely in the (dominant)
    // case where no class in the program declares any static Symbol member.
    if crate::symbol::CLASS_STATIC_SYMBOLS_LATCH.is_armed() {
        let hi_sym = crate::symbol::well_known_symbol("hasInstance");
        if !hi_sym.is_null() {
            let hi_f64 = f64::from_bits(crate::value::JSValue::pointer(hi_sym as *const u8).bits());
            if let Some(vb) = crate::symbol::class_static_symbol_lookup(class_id, hi_f64) {
                let cb = f64::from_bits(vb);
                if let HasInstanceOutcome::Result(r) = dispatch_own_has_instance(cb, value) {
                    return r;
                }
            }
        }
    }

    // OrdinaryHasInstance step 3: once no user `@@hasInstance` answered, a
    // primitive is never an instance of anything (#11261). Without this a
    // plain number reached the native brand probes below as a handle id.
    if instanceof_lhs_is_primitive(value) {
        return false_val;
    }
    // Subclass-of-built-in: see `subclass_of_builtin_reaches`.
    if subclass_of_builtin_reaches(value, class_id) {
        return true_val;
    }
    // Temporal reference types (`d instanceof Temporal.Duration`, …). A Temporal
    // value is a NaN-boxed pointer to a brand-tagged cell, not an ObjectHeader
    // with a class chain, so probe the cell's brand kind directly. Keep the band
    // in sync with perry-runtime/src/temporal/mod.rs.
    if (crate::temporal::CLASS_ID_TEMPORAL_FIRST..=crate::temporal::CLASS_ID_TEMPORAL_LAST)
        .contains(&class_id)
    {
        return if crate::temporal::temporal_value_matches_class_id(value, class_id) {
            true_val
        } else {
            false_val
        };
    }
    // `value instanceof Function` — true for any callable value. Per
    // `OrdinaryHasInstance`, every Perry function (declaration, expression,
    // arrow, method, bound function, native handle, built-in constructor)
    // has `Function.prototype` in its prototype chain. Keep `CLASS_ID_FUNCTION`
    // in sync with perry-codegen/src/expr/instance_misc1.rs.
    if class_id == CLASS_ID_FUNCTION {
        return if value_is_callable(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_URL {
        let addr = value_addr(value);
        let branded = addr != 0 && crate::url::is_url_object_shape(addr as *mut ObjectHeader);
        return if branded || recorded_prototype_instanceof_builtin(value, "URL") == Some(true) {
            true_val
        } else {
            false_val
        };
    }
    // Keep in sync with perry-codegen/src/expr/instance_misc1.rs.
    let classic_stream_name = match class_id {
        0xFFFF0070 => Some("Stream"),
        0xFFFF0071 => Some("Readable"),
        0xFFFF0072 => Some("Writable"),
        0xFFFF0073 => Some("Duplex"),
        0xFFFF0074 => Some("Transform"),
        0xFFFF0075 => Some("PassThrough"),
        _ => None,
    };
    if let Some(name) = classic_stream_name {
        return if crate::node_stream::is_classic_stream_instance_of(value, name)
            || super::tls_constructor_prototype_is_instance_of(value, name)
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_EVENT_EMITTER {
        return if is_event_emitter_instance_value(value)
            || super::tls_constructor_prototype_is_instance_of(value, "EventEmitter")
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE {
        return if is_event_emitter_async_resource_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_ASYNC_RESOURCE {
        return if crate::async_hooks::resolve_async_resource_handle(value_addr(value) as i64)
            .is_some()
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_ASYNC_LOCAL_STORAGE {
        let candidate = small_native_handle_id(value).unwrap_or(value_addr(value) as i64);
        let matched = candidate != 0 && {
            super::class_handles::handle_property_dispatch().is_some_and(|dispatch| {
                let property = b"getStore";
                let result = unsafe { dispatch(candidate, property.as_ptr(), property.len()) };
                value_is_callable(result)
            })
        };
        return if matched { true_val } else { false_val };
    }
    if class_id == CLASS_ID_NET_SOCKET {
        return if let Some(handle) = small_native_handle_id(value) {
            let net_socket = crate::object::net_socket_handle_probe()
                .map(|probe| unsafe { probe(handle) })
                .unwrap_or(false);
            let tls_socket = crate::object::tls_handle_kind_probe()
                .map(|probe| unsafe { probe(handle) == 2 })
                .unwrap_or(false);
            if net_socket || tls_socket {
                true_val
            } else {
                false_val
            }
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_STATS_EXPORT {
        return if crate::fs::is_fs_stats_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_DIR {
        return if crate::fs::is_fs_dir_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_DIRENT {
        return if crate::fs::is_fs_dirent_instance_value(value) {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_READ_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "ReadStream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_WRITE_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "WriteStream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == crate::fs::CLASS_ID_FS_UTF8_STREAM {
        return if crate::fs::is_fs_stream_instance_value(value, "Utf8Stream") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_CRYPTO {
        return if is_native_module_namespace_value(value, "crypto.webcrypto") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_SUBTLE_CRYPTO {
        return if is_native_module_namespace_value(value, "crypto.subtle") {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_CRYPTO_KEY {
        let addr = value_addr(value);
        return if addr != 0 && crate::buffer::crypto_key_meta(addr).is_some() {
            true_val
        } else {
            false_val
        };
    }

    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    // Native/exotic subclass instances (typed arrays, ArrayBuffers, boxed
    // primitives, Dates, …) do not carry a Perry `ObjectHeader.class_id`.
    // Their constructor records the distinct newTarget prototype in the
    // prototype side table instead. Honor that chain for user class ids.
    if is_class_id_registered(class_id) {
        let addr = value_addr(value);
        if addr != 0 && super::prototype_chain::object_static_prototype(addr).is_some() {
            let constructor = super::class_constructor_ref_value(class_id);
            return if ordinary_has_instance_prototype_walk(value, constructor) {
                true_val
            } else {
                false_val
            };
        }
    }

    // Special handling for Uint8Array/Buffer (class_id 0xFFFF0004)
    // Perry buffers are raw BufferHeader pointers bitcast to f64 (not NaN-boxed),
    // so the normal POINTER_TAG check doesn't work for them.
    // We use a thread-local buffer registry to identify buffer pointers.
    if class_id == crate::buffer::BUFFER_TYPE_ID {
        // Check if NaN-boxed pointer
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::buffer::is_registered_buffer(addr) {
                return true_val;
            }
        }
        // Check if raw pointer (buffer values are bitcast, not NaN-boxed)
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 && crate::buffer::is_registered_buffer(bits as usize) {
            return true_val;
        }
        return false_val;
    }

    // ArrayBuffer — Perry models ArrayBuffer storage with BufferHeader values
    // marked in a side registry. They can arrive either NaN-boxed or as raw
    // buffer pointers, matching the Buffer/Uint8Array path above.
    const CLASS_ID_ARRAY_BUFFER: u32 = 0xFFFF0025;
    const CLASS_ID_SHARED_ARRAY_BUFFER: u32 = 0xFFFF002E;
    if class_id == CLASS_ID_ARRAY_BUFFER || class_id == CLASS_ID_SHARED_ARRAY_BUFFER {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        let matches_brand = if class_id == CLASS_ID_SHARED_ARRAY_BUFFER {
            crate::buffer::is_shared_array_buffer(addr)
        } else {
            crate::buffer::is_array_buffer(addr)
        };
        if addr != 0 && crate::buffer::is_registered_buffer(addr) && matches_brand {
            return true_val;
        }
        return false_val;
    }

    // #1545: Web Streams `instanceof ReadableStream` / `instanceof
    // WritableStream`. Stream handles are numeric `id as f64`, so consult the
    // stdlib kind-probe (1 = readable, 2 = writable) rather than the class
    // chain. Covers `ts.readable instanceof ReadableStream`,
    // `rs.pipeThrough(ts) instanceof ReadableStream`, etc.
    // kind probe values: 1 = readable, 2 = writable, 5 = transform
    // (3 = reader, 4 = writer — not user-facing instanceof targets here).
    const CLASS_ID_READABLE_STREAM: u32 = 0xFFFF0060;
    const CLASS_ID_WRITABLE_STREAM: u32 = 0xFFFF0061;
    const CLASS_ID_TRANSFORM_STREAM: u32 = 0xFFFF0062;
    if class_id == CLASS_ID_READABLE_STREAM
        || class_id == CLASS_ID_WRITABLE_STREAM
        || class_id == CLASS_ID_TRANSFORM_STREAM
    {
        if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
            if let Some(probe) = crate::object::stream_handle_kind_probe() {
                let kind = unsafe { probe(value as usize) };
                let want = match class_id {
                    CLASS_ID_READABLE_STREAM => 1,
                    CLASS_ID_WRITABLE_STREAM => 2,
                    _ => 5, // CLASS_ID_TRANSFORM_STREAM
                };
                if kind == want {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // WHATWG fetch: `instanceof Response` / `Request` / `Headers` / `Blob` /
    // `File`.
    // These are pointer-tagged small-integer handles (stdlib fetch registries),
    // not heap objects, so consult the stdlib fetch kind-probe rather than the
    // class chain. Without this, Hono's `res instanceof Response` route-fallback
    // guard sees `false` and skips the fallback, escaping a bare sentinel.
    const CLASS_ID_RESPONSE: u32 = 0xFFFF0028;
    const CLASS_ID_REQUEST: u32 = 0xFFFF0029;
    const CLASS_ID_HEADERS: u32 = 0xFFFF002A;
    const CLASS_ID_BLOB: u32 = 0xFFFF0026;
    const CLASS_ID_FILE: u32 = 0xFFFF002F;
    if class_id == CLASS_ID_RESPONSE
        || class_id == CLASS_ID_REQUEST
        || class_id == CLASS_ID_HEADERS
        || class_id == CLASS_ID_BLOB
        || class_id == CLASS_ID_FILE
    {
        let want = match class_id {
            CLASS_ID_RESPONSE => 1u8,
            CLASS_ID_REQUEST => 2,
            CLASS_ID_HEADERS => 3,
            CLASS_ID_BLOB => 4,
            _ => 5, // CLASS_ID_FILE
        };
        if let Some(handle) = small_native_handle_id(value) {
            if let Some(probe) = crate::object::fetch_handle_kind_probe() {
                let kind = unsafe { probe(handle as usize) };
                // File inherits Blob, so a File handle satisfies both brands.
                if kind == want || (class_id == CLASS_ID_BLOB && kind == 5) {
                    return true_val;
                }
            }
        }
        // `class X extends Request/Response` instance: a heap object that
        // stashes the underlying native fetch handle id under
        // `__perry_fetch_handle__`. Unwrap and probe so `sub instanceof
        // Request` is true, matching a bare handle.
        if jsval.is_pointer() {
            let raw = jsval.as_pointer::<u8>() as usize;
            if let Some(id) = unsafe { crate::object::fetch_subclass_handle_id(raw) } {
                if let Some(probe) = crate::object::fetch_handle_kind_probe() {
                    let kind = unsafe { probe(id as usize) };
                    if kind == want || (class_id == CLASS_ID_BLOB && kind == 5) {
                        return true_val;
                    }
                }
            }
        }
        // A Blob can also be a real heap object allocated with CLASS_ID_BLOB
        // (e.g. `stream/consumers`.`blob()` and `blob_value_from_bytes`), not
        // just a small fetch-registry handle. Match it by its own class id so
        // `blob instanceof Blob` is true for that representation too.
        if class_id == CLASS_ID_BLOB && jsval.is_pointer() {
            let obj = jsval.as_pointer::<ObjectHeader>();
            if crate::value::addr_class::is_above_handle_band(obj as usize)
                && unsafe { (*obj).class_id } == CLASS_ID_BLOB
            {
                return true_val;
            }
        }
        return false_val;
    }

    // Built-in JS types Map / Set / RegExp / Date — Perry doesn't define
    // user classes for these, so we use reserved class IDs and detect via
    // the per-type registries (MAP_REGISTRY / SET_REGISTRY / REGEX_POINTERS)
    // or, for Date, by checking that the value is a finite f64 timestamp.
    const CLASS_ID_DATE: u32 = 0xFFFF0020;
    const CLASS_ID_REGEXP: u32 = 0xFFFF0021;
    const CLASS_ID_MAP: u32 = 0xFFFF0022;
    const CLASS_ID_SET: u32 = 0xFFFF0023;
    if class_id == CLASS_ID_DATE {
        // A Perry Date is a NaN-boxed pointer to a `DateCell` (#2089). Its
        // identity is the cell's `GcHeader` type, so `new Date(NaN)` (an
        // Invalid Date — a cell whose time value is NaN) matches just like
        // any other Date, and a plain number never matches.
        if crate::date::is_date_value(value) {
            return true_val;
        }
        return false_val;
    }
    if class_id == CLASS_ID_MAP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::map::is_registered_map(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_SET {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::set::is_registered_set(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    // #5834: `x instanceof WeakMap`/`WeakSet` for a REAL instance. These
    // reserved ids (kept in sync with perry-codegen/src/expr/instance_misc1.rs)
    // are distinct from the runtime `CLASS_ID_WEAKMAP`/`CLASS_ID_WEAKSET`
    // stamped on actual instances (weakref.rs) — the subclass-chain walk above
    // only matches a `class S extends WeakMap {}` instance (whose chain reaches
    // this reserved id), so a genuine `new WeakMap()` still needs its own probe
    // here, same shape as Map/Set above.
    const CLASS_ID_WEAKMAP_RESERVED: u32 = 0xFFFF002C;
    const CLASS_ID_WEAKSET_RESERVED: u32 = 0xFFFF002D;
    if class_id == CLASS_ID_WEAKMAP_RESERVED {
        return if crate::object::weak_class_id_from_receiver(value)
            == Some(crate::weakref::CLASS_ID_WEAKMAP)
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_WEAKSET_RESERVED {
        return if crate::object::weak_class_id_from_receiver(value)
            == Some(crate::weakref::CLASS_ID_WEAKSET)
        {
            true_val
        } else {
            false_val
        };
    }
    if class_id == CLASS_ID_REGEXP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::regex::is_regex_pointer(addr as *const u8) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_PROMISE {
        if let Some(matches) = recorded_prototype_instanceof_builtin(value, "Promise") {
            return if matches { true_val } else { false_val };
        }
        return if crate::promise::js_value_is_promise(value) != 0 {
            true_val
        } else {
            false_val
        };
    }

    // `Object` — ECMAScript spec: `x instanceof Object` is true for any
    // non-primitive (every object/array/function/Map/Set/Buffer/RegExp/
    // Date/typed-array/Promise/etc.). The codegen maps `Object` to this
    // reserved id (#585 follow-up: pre-#585 fix this case worked by
    // accident because the codegen produced `class_id = 0` and the
    // runtime returned true via `0 == 0` on the obj_class_id check).
    const CLASS_ID_OBJECT: u32 = 0xFFFF0050;
    if class_id == CLASS_ID_OBJECT {
        if jsval.is_pointer() {
            // A Symbol is a POINTER_TAG heap allocation but a PRIMITIVE, not an
            // object, so `Symbol() instanceof Object` is false (the comment
            // above says "any non-primitive"). Every other primitive is
            // non-pointer-tagged and already falls through below. #6587 review.
            if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
                return false_val;
            }
            // Covers every heap object, including a Date (now a NaN-boxed
            // `DateCell` pointer — #2089) and an Invalid Date.
            return true_val;
        }
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            let addr = bits as usize;
            if crate::buffer::is_registered_buffer(addr)
                || crate::set::is_registered_set(addr)
                || crate::map::is_registered_map(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            {
                return true_val;
            }
        }
        return false_val;
    }

    // Array — Perry arrays are heap allocations with `GC_TYPE_ARRAY` in
    // their gc_header (one byte at obj-8). Pointer can arrive NaN-boxed
    // (POINTER_TAG) or as a raw bitcast f64; handle both. Lazy arrays
    // (Phase 5 JSON.parse result) are also arrays from the user's
    // perspective — must return true without force-materializing.
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == CLASS_ID_ARRAY {
        // A POINTER_TAG handle id (fetch/zlib/stdlib registries) is not a heap
        // address; the canonical header read rejects it instead of probing the
        // byte below it.
        let is_array = unsafe { crate::value::addr_class::try_read_gc_header(value_addr(value)) }
            .is_some_and(|header| {
                header.obj_type == crate::gc::GC_TYPE_ARRAY
                    || header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            });
        return if is_array { true_val } else { false_val };
    }

    // Typed arrays — Int8Array..Float16Array reserved IDs (0xFFFF0030..3B).
    // The pointer can arrive as either a NaN-boxed POINTER_TAG value or a
    // raw bitcast f64, so handle both forms.
    if (0xFFFF0030..=0xFFFF003B).contains(&class_id) {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 {
            if let Some(actual_kind) = crate::typedarray::lookup_typed_array_kind(addr) {
                let want_id = crate::typedarray::class_id_for_kind(actual_kind);
                if want_id == class_id {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Only objects (pointers) can be instances of classes
    if !jsval.is_pointer() {
        return false_val;
    }

    // Get the object pointer
    let obj_ptr = jsval.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_val;
    }

    // Refs #421: NaN-boxed POINTER_TAG values whose unboxed payload is a
    // small registry id (Web Fetch handles, sockets, DB connections, etc.)
    // are NOT real ObjectHeader pointers — reading the GC header at
    // `obj_ptr - 8` would SIGSEGV on unmapped memory. They aren't instances
    // of any user-defined class either, so return false unconditionally.
    if crate::value::addr_class::is_handle_band(obj_ptr as usize) {
        return false_val;
    }

    unsafe {
        // Special handling for built-in Error and its subclasses (TypeError, RangeError, etc.).
        // ErrorHeader uses GC_TYPE_ERROR; we match by error_kind against the requested CLASS_ID_*.
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let err_ptr = obj_ptr as *const crate::error::ErrorHeader;
            let kind = (*err_ptr).error_kind;
            if class_id == crate::event_target::CLASS_ID_DOM_EXCEPTION {
                return if crate::event_target::is_dom_exception_error(err_ptr) {
                    true_val
                } else {
                    false_val
                };
            }
            let builtin_name = match class_id {
                crate::error::CLASS_ID_ERROR => Some("Error"),
                crate::error::CLASS_ID_TYPE_ERROR => Some("TypeError"),
                crate::error::CLASS_ID_RANGE_ERROR => Some("RangeError"),
                crate::error::CLASS_ID_REFERENCE_ERROR => Some("ReferenceError"),
                crate::error::CLASS_ID_SYNTAX_ERROR => Some("SyntaxError"),
                crate::error::CLASS_ID_EVAL_ERROR => Some("EvalError"),
                crate::error::CLASS_ID_URI_ERROR => Some("URIError"),
                crate::error::CLASS_ID_AGGREGATE_ERROR => Some("AggregateError"),
                _ => None,
            };
            if let Some(name) = builtin_name {
                if let Some(matches) = recorded_prototype_instanceof_builtin(value, name) {
                    return if matches { true_val } else { false_val };
                }
            }
            return match class_id {
                crate::error::CLASS_ID_ERROR => true_val,
                crate::error::CLASS_ID_TYPE_ERROR => {
                    if kind == crate::error::ERROR_KIND_TYPE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_RANGE_ERROR => {
                    if kind == crate::error::ERROR_KIND_RANGE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_REFERENCE_ERROR => {
                    if kind == crate::error::ERROR_KIND_REFERENCE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_SYNTAX_ERROR => {
                    if kind == crate::error::ERROR_KIND_SYNTAX_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_EVAL_ERROR => {
                    if kind == crate::error::ERROR_KIND_EVAL_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_URI_ERROR => {
                    if kind == crate::error::ERROR_KIND_URI_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_AGGREGATE_ERROR => {
                    if kind == crate::error::ERROR_KIND_AGGREGATE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                _ => false_val,
            };
        }

        if gc_type == crate::gc::GC_TYPE_OBJECT {
            if let Some(matches) =
                crate::perf_hooks::is_perf_hooks_shape_instance_of(value, class_id)
            {
                return if matches { true_val } else { false_val };
            }
            if let Some(matches) =
                crate::perf_hooks::is_perf_entry_object_instance_of(obj_ptr, class_id)
            {
                return if matches { true_val } else { false_val };
            }
        }

        // For user-defined classes that extend Error: `myErr instanceof Error` should be true.
        if class_id == crate::error::CLASS_ID_ERROR {
            // #9940: a function-local class declaration gets a fresh class
            // object on every evaluation, but all evaluations share its
            // compile-time class id. A constructor factory can therefore
            // evaluate `class Definition extends Error {}`, then later
            // evaluate the same declaration with an Object parent. The class
            // registry is keyed by the shared id and is necessarily
            // last-wins; the instance's recorded evaluation prototype is the
            // authoritative chain. Zod's `$constructor` has exactly this
            // shape, and its later schema classes made an earlier ZodError
            // fail `instanceof Error` even though getPrototypeOf still showed
            // `ZodError -> Error -> Object`.
            if let Some(matches) = recorded_prototype_instanceof_builtin(value, "Error") {
                return if matches { true_val } else { false_val };
            }
        }

        // Everything below reads `ObjectHeader::class_id`, which only a
        // genuine `GC_TYPE_OBJECT` has. Every other GC type keeps something
        // else in that word — an array's `length`, a closure's function
        // pointer, a Map's `size` — so `[1, 2] instanceof C` was true whenever
        // the length equalled (or chained to) `C`'s class id.
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            return false_val;
        }

        if class_id == crate::error::CLASS_ID_ERROR {
            let obj_class_id = (*obj_ptr).class_id;
            if extends_builtin_error(obj_class_id) {
                return true_val;
            }
        }

        // Check if the object's class_id matches directly
        let obj_class_id = (*obj_ptr).class_id;
        if class_id == crate::event_target::CLASS_ID_EVENT
            && obj_class_id == crate::event_target::CLASS_ID_CUSTOM_EVENT
        {
            return true_val;
        }
        // Walk up the inheritance chain using the class registry. #7575: the
        // walk also follows the generic-origin edge, so a dynamic RHS holding a
        // generic class (`const C = Gen; x instanceof C`) matches an instance of
        // one of its specializations.
        if class_chain_reaches(obj_class_id, class_id) {
            return true_val;
        }

        false_val
    }
}
