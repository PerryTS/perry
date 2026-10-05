use super::*;

/// Populate the freshly-allocated globalThis singleton with built-in
/// constructor / namespace properties. Called exactly once from the CAS
/// winner in `js_get_global_this`. Constructors get a ClosureHeader-
/// backed value so `typeof globalThis.Array === "function"`; namespaces
/// (`Math`, `JSON`, `Reflect`) get a plain ObjectHeader (`typeof ===
/// "object"`). Both shapes carry a `prototype` dynamic property pointing
/// at an empty object so `<Builtin>.prototype` reads return a real
/// pointer instead of undefined, which is what unblocks lodash's
/// `var arrayProto = Array.prototype` chained read inside
/// `runInContext`.
pub(crate) fn populate_global_this_builtins(singleton_at_entry: *mut ObjectHeader) {
    // Every install below is a builtin definition, which arms the own-override
    // guard only on a Map, Set or Date owner (#10697).
    super::super::own_override::as_builtin_definition(|| {
        populate_global_this_builtins_inner(singleton_at_entry)
    })
}

fn populate_global_this_builtins_inner(singleton_at_entry: *mut ObjectHeader) {
    if singleton_at_entry.is_null() {
        return;
    }
    // #6982: this function installs several hundred builtins, and nearly every
    // step allocates (name strings, constructor closures, prototype objects).
    // Any of those allocations can trigger a collection, and an *evacuating*
    // minor relocates the globalThis singleton itself — it is an ordinary
    // nursery object at this point, not pinned.
    //
    // `js_get_global_this` registers the two slots that cache the singleton
    // (`THREAD_GLOBAL_THIS` and `GLOBAL_THIS_PTR`) as mutable roots precisely so
    // the collector rewrites them on a move, but the raw pointer handed to this
    // function is a plain by-value argument that nothing rewrites. After a move
    // every later `js_object_set_field_by_name(singleton, ..)` /
    // `set_builtin_property_attrs(singleton as usize, ..)` addressed the dead
    // from-space copy, whose bytes had already been recycled for freshly
    // relocated objects — the observed crashes read string payload where an
    // ArrayHeader/descriptor was expected (faulting addresses whose high half
    // was ASCII: 0x434c4f53_00000010 = "CLOS", 0x004e614e_00000008 = "NaN").
    //
    // Root the singleton in a `RuntimeHandleScope` and re-read it through the
    // handle at every use, so each use observes the post-move address. Binding
    // `singleton` as a closure rather than a value makes this exhaustive by
    // construction: any use that was not converted fails to compile.
    //
    // Only reachable when the conservative native-stack scan is off, which is
    // production's `Auto -> SkipDisabled` resolution; the scan was masking this
    // by pinning the argument register.
    //
    // #7217: rooting the singleton is necessary but NOT sufficient, and the
    // difference is the whole point of the allocation-point route.
    //
    // The singleton is one pointer. The bootstrap it drives is a *graph*:
    // `install_intl_namespace` -> `install_constructor` -> `install_function`
    // holds `ctor`, `proto` and `ns_obj` as raw `*mut ObjectHeader` locals
    // across dozens of allocating installs, and so do the error, typed-array,
    // generator, Reflect, Atomics, WebAssembly, … installers, in a dozen files
    // and several hundred call sites. Every one of those is a slot the
    // collector does not rewrite.
    //
    // On the SAFEPOINT route none of them can be exposed: a collection reached
    // from a loop back-edge poll only happens while user JS is running, and the
    // bootstrap runs no user JS. On the ALLOCATION-POINT route every one of the
    // bootstrap's own ~1.15 MB of allocations is a collection point, so the
    // whole graph is exposed at once. That is why three separate rooting fixes
    // verified green on `PERRY_GC_MOVING_LOOP_POLLS=1` were still red under
    // `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0
    // PERRY_CONSERVATIVE_STACK_SCAN=off`: the collection they were failing on
    // was not in the code they had fixed, it was minor #0 landing inside this
    // bootstrap. `PERRY_GC_PROTECT_FROMSPACE=1` names it exactly —
    // `set_builtin_property_attrs` <- `intl::install_function` <-
    // `install_constructor` <- `install_intl_namespace` <- here, on an address
    // `retired_by_minor=#0`.
    //
    // THE INVARIANT: a bootstrap that builds an IMMORTAL object graph through
    // raw pointers held across its own allocations must run in a NO-MOVE
    // WINDOW. Rooting each holder individually is unbounded (hundreds of sites
    // across a dozen installer modules) and ungateable (no checker can prove
    // the set complete), while the window is one line and provably enough. It
    // costs nothing a collection would have recovered: every object born here
    // is reachable from `globalThis` for the life of the process, so a
    // collection inside the window frees nothing. Measured footprint of the
    // whole window: ~1.15 MB allocated, ~410 KB of it live afterwards, once per
    // thread.
    let _no_move = crate::gc::GcSuppressScope::new();
    // Every object this bootstrap builds is reachable from `globalThis` for the
    // life of the process (the `_no_move` comment above measures the graph:
    // ~1.15 MB allocated, ~410 KB live afterwards). That makes each of them a
    // permanent tenant of the per-object GC slot-layout side tables — and ONE
    // permanent tenant is enough to disable `PER_OBJECT_LAYOUTS_NONEMPTY`,
    // the global emptiness proof that keeps `layout_forget_object` off the
    // allocation / death / relocation paths, for the rest of the run.
    //
    // Since a single plain-object property miss forces this bootstrap, that
    // made the armed regime universal: measured on the quiet mini, adding one
    // `for (const _x of [1]) {}` to `main()` cost `churn` +28% and `tree` +29%,
    // with `layout_forget_object` self time going 112 → 916 ms and 194 → 740 ms
    // respectively. The scope makes these objects declare
    // `GC_LAYOUT_UNKNOWN` (the tag-checked scan — the universally safe state)
    // instead of minting a mask nothing will ever remove. See
    // `gc::ImmortalLayoutScope` for the soundness argument and for why it is
    // deliberately NOT applied to typed-shape layouts.
    let _immortal = crate::gc::ImmortalLayoutScope::new();
    let bootstrap_started = crate::gc::gc_diag_enabled().then(std::time::Instant::now);
    let scope = crate::gc::RuntimeHandleScope::new();
    let singleton_handle = scope.root_raw_mut_ptr(singleton_at_entry);
    let singleton = || singleton_handle.get_raw_mut_ptr::<ObjectHeader>();
    {
        let name = b"globalThis";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton() as i64);
        super::super::define_builtin_data_property(
            singleton(),
            key,
            value,
            "globalThis".to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    {
        // #4511: Node exposes the global object as `global` too
        // (`global === globalThis`). Install the same self-reference so bare
        // `global` / `(global as any).x` reads resolve to the real singleton
        // instead of the unknown-identifier sentinel.
        let name = b"global";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton() as i64);
        super::super::define_builtin_data_property(
            singleton(),
            key,
            value,
            "global".to_string(),
            super::super::PropertyAttrs::new(true, true, true),
        );
    }
    // #2145: pre-allocate the shared `%TypedArray%` intrinsic so per-kind
    // typed-array constructors can link their `__proto__` to it as they're
    // built below, and the per-kind `.prototype` objects can be flagged with
    // `OBJ_FLAG_TYPED_ARRAY_PROTO` for `Object.getPrototypeOf` resolution.
    let (typed_array_intrinsic_ctor, _) = ensure_typed_array_intrinsic();
    // #3664: build the generator / async-generator intrinsic prototype towers
    // so `Object.getPrototypeOf(function*(){})`, `g.constructor`, and the
    // `%Generator(.prototype)%` chains resolve to real objects.
    ensure_generator_intrinsics();
    // Constructors: ClosureHeader-backed so typeof is "function".
    // #4533: native error subclasses must link to `Error` / `Error.prototype`.
    // `Error` is listed before its subclasses in GLOBAL_THIS_BUILTIN_CONSTRUCTORS,
    // so these are populated before the subclass iterations consume them.
    let mut error_ctor_bits: Option<u64> = None;
    let mut error_proto_bits: Option<u64> = None;
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        if name == "Buffer" {
            let name_bytes = name.as_bytes();
            let name_key =
                crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            let ctor_value = super::super::native_module::buffer_constructor_value();

            // #9173: Buffer instances resolve to Buffer.prototype, whose own
            // [[Prototype]] is Uint8Array.prototype. Both objects already
            // exist here (Buffer is deliberately last in the constructor
            // table), so link the real prototype objects without recursively
            // initializing globalThis from buffer_constructor_value().
            let ctor = JSValue::from_bits(ctor_value.to_bits());
            if ctor.is_pointer() {
                let buffer_proto = crate::closure::closure_get_dynamic_prop(
                    ctor.as_pointer::<crate::closure::ClosureHeader>() as usize,
                    "prototype",
                );
                let buffer_proto_value = JSValue::from_bits(buffer_proto.to_bits());
                let uint8_proto = builtin_prototype_value("Uint8Array");
                if buffer_proto_value.is_pointer()
                    && JSValue::from_bits(uint8_proto.to_bits()).is_pointer()
                {
                    super::super::prototype_chain::object_set_static_prototype(
                        buffer_proto_value.as_pointer::<ObjectHeader>() as usize,
                        uint8_proto.to_bits(),
                    );
                }
                // #11239: and the constructor side of the same edge —
                // `Object.getPrototypeOf(Buffer) === Uint8Array`, as for any
                // `class Buffer extends Uint8Array`.
                let uint8_ctor =
                    js_get_global_this_builtin_value("Uint8Array".as_ptr(), "Uint8Array".len());
                if JSValue::from_bits(uint8_ctor.to_bits()).is_pointer() {
                    crate::closure::closure_set_static_prototype(
                        ctor.as_pointer::<crate::closure::ClosureHeader>() as usize,
                        uint8_ctor.to_bits(),
                    );
                }
            }
            super::super::define_builtin_data_property(
                singleton(),
                name_key,
                ctor_value,
                name.to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
            continue;
        }
        if name == "Object" {
            // %Object% / %Object.prototype% are built on their own (a class
            // prototype needs them without the realm global); the thread's
            // realm global adopts them, a `vm`/eval realm gets its own pair.
            let (object_ctor, _) = object_intrinsics_for_realm(singleton());
            if !object_ctor.is_null() {
                let name_key = crate::string::js_string_from_bytes(b"Object".as_ptr(), 6);
                super::super::define_builtin_data_property(
                    singleton(),
                    name_key,
                    crate::value::js_nanbox_pointer(object_ctor as i64),
                    name.to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            continue;
        }
        let info = match name {
            "Array" => crate::fn_info!(global_this_array_thunk, 1; with_rest(0)),
            // #10423: `F(p, body)` through a `Function` value creates a
            // function, exactly like `new F(p, body)`.
            "Function" => {
                crate::fn_info!(unwind_in_tests global_this_function_call_thunk, 1; with_rest(0))
            }
            "String" => crate::fn_info!(global_this_string_thunk, 1; with_declared(1)),
            // #2889: call-form `Number(x)` / `Boolean(x)` through a rebound
            // global value coerce like the bare-call lowering does.
            "Number" => crate::fn_info!(global_this_number_thunk, 1; with_declared(1)),
            "Boolean" => crate::fn_info!(global_this_boolean_thunk, 1; with_declared(1)),
            "BigInt" => crate::fn_info!(global_this_bigint_thunk, 1; with_declared(1)),
            "Symbol" => crate::fn_info!(global_this_symbol_thunk, 1; with_declared(1)),
            "Error" => crate::fn_info!(error_constructor_call_thunk, 1; with_declared(1)),
            "TypeError" => crate::fn_info!(type_error_constructor_call_thunk, 1; with_declared(1)),
            "RangeError" => {
                crate::fn_info!(range_error_constructor_call_thunk, 1; with_declared(1))
            }
            "ReferenceError" => {
                crate::fn_info!(reference_error_constructor_call_thunk, 1; with_declared(1))
            }
            "SyntaxError" => {
                crate::fn_info!(syntax_error_constructor_call_thunk, 1; with_declared(1))
            }
            "EvalError" => crate::fn_info!(eval_error_constructor_call_thunk, 1; with_declared(1)),
            "URIError" => crate::fn_info!(uri_error_constructor_call_thunk, 1; with_declared(1)),
            "MessageChannel" => {
                crate::fn_info!(crate::messaging::js_message_channel_constructor_call_error, 0; with_declared(0))
            }
            "MessagePort" => {
                crate::fn_info!(crate::messaging::js_message_port_constructor_call_error, 0; with_declared(0))
            }
            "BroadcastChannel" => {
                crate::fn_info!(crate::messaging::js_broadcast_channel_constructor_call_error, 1; with_declared(1))
            }
            "Date" => crate::fn_info!(global_this_date_thunk, 1; with_declared(1)),
            "Blob" => crate::fn_info!(global_this_blob_thunk, 2; with_declared(2)),
            "File" => crate::fn_info!(global_this_file_thunk, 3; with_declared(3)),
            "Headers" => crate::fn_info!(global_this_headers_thunk, 1; with_declared(1)),
            "Request" => crate::fn_info!(global_this_request_thunk, 2; with_declared(2)),
            "Response" => crate::fn_info!(global_this_response_thunk, 2; with_declared(2)),
            "URLPattern" => {
                crate::fn_info!(global_this_url_pattern_call_thunk, 2; with_declared(2))
            }
            "Storage" => {
                crate::fn_info!(crate::web_storage::storage_constructor_illegal, 0; with_declared(0))
            }
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                crate::fn_info!(webcrypto_illegal_constructor_thunk, 0)
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => {
                crate::fn_info!(typed_array_constructor_call_thunk, 1; with_declared(0))
            }
            // #4569: collection constructors throw when called without `new`.
            "RegExp" => crate::fn_info!(regexp_constructor_call_thunk, 2; with_declared(2)),
            "Map" => crate::fn_info!(map_constructor_call_thunk, 1),
            "Set" => crate::fn_info!(set_constructor_call_thunk, 1),
            "WeakMap" => crate::fn_info!(weak_map_constructor_call_thunk, 1),
            "WeakSet" => crate::fn_info!(weak_set_constructor_call_thunk, 1),
            "WeakRef" => crate::fn_info!(weak_ref_constructor_call_thunk, 1),
            "Promise" => crate::fn_info!(promise_constructor_call_thunk, 1),
            "ArrayBuffer" | "SharedArrayBuffer" | "DataView" => {
                crate::fn_info!(construct_only_builtin_call_thunk, 0)
            }
            _ => crate::fn_info!(global_this_builtin_noop_thunk, 1),
        };
        let closure_ptr = crate::closure::js_closure_alloc(info, 0);
        if closure_ptr.is_null() {
            continue;
        }
        // #2889: install static methods (`Object.keys`, `Array.isArray`, ...)
        // on the constructor closure so rebound usage like
        // `const O = Object; O.keys(x)` dispatches through the real helpers.
        // The prototype is an own static data property too. Include it in
        // the initial attributed layout so adding name/length cannot spill it.
        let proto_obj = if name == "Array" {
            crate::array::js_array_alloc(0) as *mut ObjectHeader
        } else {
            super::proto_room::alloc_builtin_prototype()
        };
        install_builtin_constructor_statics(
            name,
            closure_ptr,
            (!proto_obj.is_null()).then(|| crate::value::js_nanbox_pointer(proto_obj as i64)),
        );
        // #11193: `C[Symbol.species]` getter returning `this`.
        if matches!(
            name,
            "Array" | "Map" | "Set" | "Promise" | "RegExp" | "ArrayBuffer" | "SharedArrayBuffer"
        ) {
            install_builtin_species_accessor(closure_ptr);
        }
        if name == "Error" {
            install_error_static_methods(closure_ptr);
        }
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        // #4533: `Object.getPrototypeOf(TypeError) === Error`. The constructor's
        // `[[Prototype]]` is `Error` itself (not `Function.prototype`).
        if name == "Error" {
            error_ctor_bits = Some(ctor_value.to_bits());
        } else if is_native_error_subclass_constructor(name) {
            if let Some(proto_bits) = error_ctor_bits {
                crate::closure::closure_set_static_prototype(closure_ptr as usize, proto_bits);
            }
        }
        if !proto_obj.is_null() {
            let ctor_key = crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                "constructor".len() as u32,
            );
            super::super::define_builtin_data_property(
                proto_obj,
                ctor_key,
                ctor_value,
                "constructor".to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
            if is_web_fetch_constructor(name) {
                super::super::define_builtin_data_property(
                    proto_obj,
                    ctor_key,
                    ctor_value,
                    "constructor".to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            if name == "Array" {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                super::super::define_builtin_data_property(
                    proto_obj,
                    constructor_key,
                    ctor_value,
                    "constructor".to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            if matches!(
                name,
                "Navigator"
                    | "TextEncoderStream"
                    | "TextDecoderStream"
                    | "CompressionStream"
                    | "DecompressionStream"
            ) {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
            }
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that take their receiver as
            // `this` and dispatch to the corresponding native entry
            // point — works in tandem with `.call`/`.apply` since those
            // arms (#970) pass the explicit receiver when forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
            crate::event_target::prototype::install_constructor_constants(name, closure_ptr.cast());
            if name == "Function" {
                // SAFETY: `proto_obj` is the live, just-populated prototype.
                unsafe {
                    crate::object::proto_validity::assign_intrinsic_prototype_serial(
                        proto_obj as usize,
                        crate::closure::shape::INTRINSIC_SERIAL_FUNCTION,
                    );
                }
                crate::closure::shape::FUNCTION_PROTOTYPE_PTR
                    .store(proto_obj as i64, std::sync::atomic::Ordering::Release);
            }
            install_error_prototype_data_properties(name, proto_obj);
            // ECMA-262 20.5.6.3: the [[Prototype]] of each NativeError prototype
            // object is %Error.prototype% (not %Object.prototype%). `Error` is
            // listed before its subclasses, so its prototype object is stashed
            // here and linked into each subclass prototype's chain. Without this
            // `Object.getPrototypeOf(TypeError.prototype) !== Error.prototype`
            // (test262 NativeErrors/*/prototype/proto.js).
            if name == "Error" {
                error_proto_bits =
                    Some(crate::value::js_nanbox_pointer(proto_obj as i64).to_bits());
            } else if is_native_error_subclass_constructor(name) {
                if let Some(proto_bits) = error_proto_bits {
                    super::super::prototype_chain::object_set_static_prototype(
                        proto_obj as usize,
                        proto_bits,
                    );
                }
            }
            if matches!(name, "MessageChannel" | "MessagePort" | "BroadcastChannel") {
                crate::messaging::populate_messaging_prototype(name, proto_obj, ctor_value);
            }
            if name == "Storage" {
                crate::web_storage::install_storage_globals(
                    singleton(),
                    closure_ptr,
                    proto_obj,
                    ctor_value,
                );
            }
            if matches!(name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
                super::super::native_module::install_webcrypto_constructor_proto(
                    proto_obj, ctor_value,
                );
            }
            if name == "WebSocket" {
                websocket_global::install_constructor_shape(closure_ptr, proto_obj);
            }
            // #2145: link per-kind typed-array constructors into the
            // `%TypedArray%` chain. `Int8Array.__proto__ === %TypedArray%`
            // and `Object.getPrototypeOf(Int8Array.prototype) ===
            // %TypedArray%.prototype`. Both reads are resolved off this
            // wiring (closure static-prototype side-table for the ctor;
            // `OBJ_FLAG_TYPED_ARRAY_PROTO` + the cached
            // `TYPED_ARRAY_INTRINSIC_PROTO_PTR` for the per-kind proto).
            if !typed_array_intrinsic_ctor.is_null()
                && matches!(
                    name,
                    "Int8Array"
                        | "Uint8Array"
                        | "Uint8ClampedArray"
                        | "Int16Array"
                        | "Uint16Array"
                        | "Int32Array"
                        | "Uint32Array"
                        | "Float16Array"
                        | "Float32Array"
                        | "Float64Array"
                        | "BigInt64Array"
                        | "BigUint64Array"
                )
            {
                if name == "Uint8Array" {
                    crate::closure::js_closure_set_capture_f64(
                        typed_array_intrinsic_ctor,
                        0,
                        crate::value::js_nanbox_pointer(proto_obj as i64),
                    );
                }
                let intrinsic_bits =
                    crate::value::js_nanbox_pointer(typed_array_intrinsic_ctor as i64).to_bits();
                crate::closure::closure_set_static_prototype(closure_ptr as usize, intrinsic_bits);
                unsafe {
                    let gc = (proto_obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *mut crate::gc::GcHeader;
                    (*gc)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                    // Charter step 3: a store-kind input.
                    crate::object::shapes::store_kind::restamp_object_store_kind(
                        proto_obj as *mut crate::object::ObjectHeader,
                    );
                }
                // Record the per-kind proto's `[[Prototype]]` as the shared
                // `%TypedArray%.prototype` so the ordinary property-get chain
                // walk (`resolve_inherited_field`) finds the inherited methods
                // (`map`, `filter`, `toString`, …) that no longer live on the
                // per-kind proto as own properties. `Object.getPrototypeOf`
                // already resolves via the flag above; this link drives value
                // reads like `Int8Array.prototype.map`.
                let intrinsic_proto =
                    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
                if intrinsic_proto != 0 {
                    let proto_bits = crate::value::js_nanbox_pointer(intrinsic_proto).to_bits();
                    super::super::prototype_chain::object_set_static_prototype(
                        proto_obj as usize,
                        proto_bits,
                    );
                }
            }
            // #6674: the TC39 base64/hex conversion methods
            // (`toBase64`/`toHex`/`setFromBase64`/`setFromHex`) are Uint8Array-ONLY
            // (they don't exist on other typed arrays), so install them as own
            // methods on the `Uint8Array` per-kind prototype — not the shared
            // `%TypedArray%.prototype` intrinsic. This materializes the value read
            // `typeof Uint8Array.prototype.toBase64 === "function"`; the static
            // factories `fromBase64`/`fromHex` are installed on the constructor by
            // `install_builtin_constructor_statics`.
            if name == "Uint8Array" {
                super::super::typed_array_proto_thunks::install_uint8array_base64_hex_proto_methods(
                    proto_obj,
                );
            }
            // #4140: per-kind `BYTES_PER_ELEMENT` own data property on BOTH the
            // constructor and its prototype, matching Node's descriptor
            // `{ value, writable:false, enumerable:false, configurable:false }`.
            // The bare `Uint8Array.BYTES_PER_ELEMENT` read folds at compile time
            // (#2902), but the reflective forms — `getOwnPropertyDescriptor`,
            // `hasOwnProperty`, and the chained `Float64Array.prototype
            // .BYTES_PER_ELEMENT` — resolve off these installed own properties.
            let ta_bytes_per_element = match name {
                "Int8Array" | "Uint8Array" | "Uint8ClampedArray" => Some(1.0),
                "Int16Array" | "Uint16Array" | "Float16Array" => Some(2.0),
                "Int32Array" | "Uint32Array" | "Float32Array" => Some(4.0),
                "Float64Array" | "BigInt64Array" | "BigUint64Array" => Some(8.0),
                _ => None,
            };
            if let Some(bytes) = ta_bytes_per_element {
                let bpe_attrs = super::super::PropertyAttrs::new(false, false, false);
                for target in [closure_ptr as *mut ObjectHeader, proto_obj] {
                    let bpe_key = crate::string::js_string_from_bytes(
                        b"BYTES_PER_ELEMENT".as_ptr(),
                        b"BYTES_PER_ELEMENT".len() as u32,
                    );
                    super::super::define_builtin_data_property(
                        target,
                        bpe_key,
                        bytes,
                        "BYTES_PER_ELEMENT".to_string(),
                        bpe_attrs,
                    );
                }
            }
            if name != "Array" {
                // Its own keys are in: return the room it did not use.
                // SAFETY: fresh, unexposed, under this bootstrap's no-move scope.
                unsafe { super::proto_room::fit_builtin_prototype(proto_obj) };
            }
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        super::super::define_builtin_data_property(
            singleton(),
            name_key,
            ctor_value,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    // Interface inheritance carries prototype methods. DOMException has
    // Error.prototype as its instance parent, but no Error constructor parent.
    for (child, parent, constructor_inherits) in [
        ("AbortSignal", "EventTarget", true),
        ("CustomEvent", "Event", true),
        ("DOMException", "Error", false),
    ] {
        let child_ctor = js_get_global_this_builtin_value(child.as_ptr(), child.len());
        let parent_ctor = js_get_global_this_builtin_value(parent.as_ptr(), parent.len());
        let child_proto = builtin_prototype_value(child);
        let parent_proto = builtin_prototype_value(parent);
        if constructor_inherits
            && JSValue::from_bits(child_ctor.to_bits()).is_pointer()
            && JSValue::from_bits(parent_ctor.to_bits()).is_pointer()
        {
            crate::closure::closure_set_static_prototype(
                crate::value::js_nanbox_get_pointer(child_ctor) as usize,
                parent_ctor.to_bits(),
            );
        }
        if JSValue::from_bits(child_proto.to_bits()).is_pointer()
            && JSValue::from_bits(parent_proto.to_bits()).is_pointer()
        {
            super::super::prototype_chain::object_set_static_prototype(
                crate::value::js_nanbox_get_pointer(child_proto) as usize,
                parent_proto.to_bits(),
            );
        }
    }
    // The hidden async/generator towers are allocated before the constructor
    // loop, but their parents are the `Function` values installed by that
    // loop. Complete those links now that both are available.
    wire_function_intrinsic_parents();
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    for name in GLOBAL_THIS_BUILTIN_FUNCTIONS.iter().copied() {
        let (info, arity, enumerable) = match name {
            "eval" => (
                crate::fn_info!(global_this_eval_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "fetch" => (
                crate::fn_info!(super::super::global_fetch::global_this_fetch_thunk, 2; with_rest(1)),
                1,
                true,
            ),
            "structuredClone" => (
                crate::fn_info!(global_this_structured_clone_thunk, 2; with_declared(2)),
                2,
                true,
            ),
            "atob" => (
                crate::fn_info!(global_this_atob_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "btoa" => (
                crate::fn_info!(global_this_btoa_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setTimeout" => (
                crate::fn_info!(global_this_set_timeout_thunk, 3; with_rest(2)),
                2,
                true,
            ),
            "clearTimeout" => (
                crate::fn_info!(global_this_clear_timeout_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setInterval" => (
                crate::fn_info!(global_this_set_interval_thunk, 3; with_rest(2)),
                2,
                true,
            ),
            "clearInterval" => (
                crate::fn_info!(global_this_clear_interval_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setImmediate" => (
                crate::fn_info!(global_this_set_immediate_thunk, 2; with_rest(1)),
                1,
                true,
            ),
            "clearImmediate" => (
                crate::fn_info!(global_this_clear_immediate_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "queueMicrotask" => (
                crate::fn_info!(global_this_queue_microtask_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            // `gc([force])` — value form of the bare `gc()` call-intrinsic.
            // Non-enumerable (a debug/diagnostic global). The optional `force`
            // arg is accepted (arity 1) but ignored — Perry's gc is full.
            "gc" => (
                crate::fn_info!(global_this_gc_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            // #2905: standard global helper functions.
            "parseInt" => (
                crate::fn_info!(global_this_parse_int_thunk, 2; with_declared(2)),
                2,
                false,
            ),
            "parseFloat" => (
                crate::fn_info!(global_this_parse_float_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "isNaN" => (
                crate::fn_info!(global_this_is_nan_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "isFinite" => (
                crate::fn_info!(global_this_is_finite_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "encodeURI" => (
                crate::fn_info!(global_this_encode_uri_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "decodeURI" => (
                crate::fn_info!(global_this_decode_uri_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "encodeURIComponent" => (
                crate::fn_info!(global_this_encode_uri_component_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "decodeURIComponent" => (
                crate::fn_info!(global_this_decode_uri_component_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            // #4511: legacy escape/unescape (ES Annex B).
            // #4511: legacy escape/unescape (ES Annex B).
            "escape" => (
                crate::fn_info!(global_this_escape_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "unescape" => (
                crate::fn_info!(global_this_unescape_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            _ => continue,
        };
        let closure_ptr = crate::closure::js_closure_alloc(info, 0);
        if closure_ptr.is_null() {
            continue;
        }
        unsafe {
            crate::builtins::js_register_function_name(
                (*info).code,
                name.as_ptr(),
                name.len() as u32,
            );
        }
        super::super::native_module::set_builtin_closure_length(closure_ptr as usize, arity);
        // Every global helper installed here (parseInt/parseFloat/isNaN/
        // isFinite/{en,de}codeURI{,Component}/escape/unescape/setTimeout/…) is a
        // built-in *non-constructor* function: per spec it has no `.prototype`
        // (reads back `undefined`) and `new fn()` throws a TypeError. Mark the
        // closure so the `new`/`.prototype` paths honor that — otherwise these
        // functions defaulted to an ordinary `.prototype` object and silently
        // accepted `new` (test262 built-ins/{decodeURI,isNaN,parseFloat,…}
        // */A5.6/A5.7/A2.6/A2.7/A7.6/A7.7).
        super::super::native_module::set_builtin_closure_non_constructable(closure_ptr as usize);
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let fn_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        super::super::define_builtin_data_property(
            singleton(),
            name_key,
            fn_value,
            name.to_string(),
            super::super::PropertyAttrs::new(true, enumerable, true),
        );
    }
    // ECMA-262 21.1.2.12 / 21.1.2.13: `Number.parseFloat` and `Number.parseInt`
    // are the SAME function objects as the global `parseFloat` / `parseInt`
    // (`Number.parseFloat === parseFloat`). The Number constructor statics were
    // installed above with fresh thunks — before the global helpers existed —
    // so re-point them now at the global closures we just created on the
    // singleton. A value-read of `Number.parseFloat` resolves to the Number
    // constructor's own `parseFloat` field (see expr_member.rs reroute-undo),
    // which now holds the identical closure the bare `parseFloat` resolves to.
    alias_number_static_to_global_function(singleton(), "parseFloat");
    alias_number_static_to_global_function(singleton(), "parseInt");
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ns_value = if matches!(name, "console" | "process") {
            // #6230: dynamic method calls on the namespace *value* (`const p =
            // process; p.exit(1)`, `process["exit"](1)`, dynamic `console.log`)
            // need the module's runtime dispatch bucket. It is NOT installed
            // here: this function is statically reachable from core property
            // paths, so installing here linked both buckets (and `process`'s
            // stdio stream objects) into every binary. Codegen instead emits
            // `js_nm_install_process` / `js_nm_install_console` /
            // `js_install_global_value_surfaces` at every site that can yield
            // one of these values (see perry-codegen `global_value_installs`).
            js_create_native_module_namespace(name_bytes.as_ptr(), name_bytes.len())
        } else if name == "WebAssembly" {
            super::global_this_webassembly::create_webassembly_namespace()
        } else {
            let ns_obj = js_object_alloc(0, 0);
            if ns_obj.is_null() {
                continue;
            }
            // #4139 + #4149: reify each namespace's own members as real
            // properties so the reflection APIs (`getOwnPropertyDescriptor`,
            // `getOwnPropertyNames`) observe them. Call sites (`Math.max(...)`,
            // `JSON.stringify(...)`, `Reflect.get(...)`) are codegen intrinsics
            // gated on the AST shape and never read these fields. Math uses the
            // richer install that also exposes per-method name/length descriptors.
            match name {
                "Math" => {
                    #[cfg(feature = "global-math")]
                    install_math_namespace(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Math");
                }
                "JSON" => {
                    #[cfg(feature = "global-json")]
                    install_json_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "JSON");
                }
                "Reflect" => {
                    #[cfg(feature = "global-reflect")]
                    install_reflect_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Reflect");
                }
                "Atomics" => {
                    #[cfg(feature = "global-atomics")]
                    install_atomics_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Atomics");
                }
                "Intl" => crate::intl::install_intl_namespace(ns_obj),
                // Members come from the `temporal` install (see
                // `crate::temporal::hooked`); without it the namespace stays a
                // plain empty object, as in a build without the feature.
                "Temporal" => {
                    if crate::temporal::hooked::install_namespace(ns_obj) {
                        set_intrinsic_to_string_tag(ns_obj, "Temporal");
                    }
                }
                _ => {}
            }
            crate::value::js_nanbox_pointer(ns_obj as i64)
        };
        super::super::define_builtin_data_property(
            singleton(),
            name_key,
            ns_value,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    // node:perf_hooks `performance` global — bind it to the same singleton the
    // named import resolves to, so `globalThis.performance ===
    // require("perf_hooks").performance` (#1327). typeof stays "object".
    {
        let pname = b"performance";
        let pkey = crate::string::js_string_from_bytes(pname.as_ptr(), pname.len() as u32);
        let pval = crate::perf_hooks::performance_namespace();
        js_object_set_field_by_name(singleton(), pkey, pval);
    }
    // Perf_hooks constructors are globals identical to the module exports.
    for name in [
        "Performance",
        "PerformanceEntry",
        "PerformanceMark",
        "PerformanceMeasure",
        "PerformanceObserver",
        "PerformanceObserverEntryList",
        "PerformanceResourceTiming",
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value =
            super::super::native_module::bound_native_callable_export_value("perf_hooks", name);
        js_object_set_field_by_name(singleton(), key, value);
    }
    super::super::native_module::install_global_webcrypto(singleton());
    let getter = crate::closure::js_closure_alloc(
        crate::fn_info!(global_this_crypto_getter_thunk, 0; with_declared(0)),
        0,
    );
    let getter_bits = if getter.is_null() {
        0
    } else {
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    };
    super::super::set_builtin_accessor_descriptor(
        singleton() as usize,
        "crypto".to_string(),
        super::super::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        super::super::PropertyAttrs::new(true, true, true),
    );
    // #2923: `globalThis.navigator` — Node's browser-compatible runtime
    // metadata object. typeof is "object". Built once per process.
    {
        let nname = b"navigator";
        let nkey = crate::string::js_string_from_bytes(nname.as_ptr(), nname.len() as u32);
        // Read the `Navigator` constructor we installed on the singleton above
        // and hand it to the navigator builder directly. We must NOT call
        // `js_navigator_object()` here: it re-fetches the constructor via
        // `js_get_global_this_builtin_value` → `js_get_global_this`, which would
        // re-enter this very lazy-init (GLOBAL_THIS_READY is still false until we
        // return) and recurse/spin forever.
        let nav_ctor_key = crate::string::js_string_from_bytes(b"Navigator".as_ptr(), 9);
        let nav_ctor = js_object_get_field_by_name(singleton(), nav_ctor_key);
        let nval =
            crate::navigator::navigator_object_with_constructor(f64::from_bits(nav_ctor.bits()));
        js_object_set_field_by_name(singleton(), nkey, nval);
    }
    // ECMA-262 19.1/19.2/19.3: NaN, Infinity, and undefined are own data
    // properties of the global object with {writable:false, enumerable:false,
    // configurable:false}.  Install them so that
    // `Object.getOwnPropertyDescriptor(globalThis, "NaN")` returns a real
    // descriptor (test262 15.2.3.3-4-178/179/180) and
    // `Object.getOwnPropertyNames(globalThis)` includes them (15.2.3.4-4-1).
    {
        let non_writable = super::super::PropertyAttrs::new(false, false, false);
        for (name, value) in [("NaN", f64::NAN), ("Infinity", f64::INFINITY)] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            super::super::define_builtin_data_property(
                singleton(),
                key,
                value,
                name.to_string(),
                non_writable,
            );
        }
        let undef_key = crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9);
        let undef_val = f64::from_bits(crate::value::TAG_UNDEFINED);
        super::super::define_builtin_data_property(
            singleton(),
            undef_key,
            undef_val,
            "undefined".to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );
    }
    // ECMA-262 §23.2.3.33: `%TypedArray%.prototype.toString` must be the
    // same function object as `Array.prototype.toString`. Alias it now that
    // both the Array constructor and the TypedArray intrinsic are set up.
    alias_typed_array_proto_to_string(singleton());
    // #10497: memoize the intrinsic prototype identities while every
    // `globalThis.<Builtin>` still names its intrinsic — before any user code
    // can reassign the writable `globalThis.Function`.
    crate::array::prime_prototype_addr_cache();
    // The bootstrap's own cost, and the evidence that the `ImmortalLayoutScope`
    // above actually did its job. `slot_masks`/`typed` are the live entry
    // counts of the two per-object layout side tables: they must still read
    // `0 0` here, because a non-zero count is exactly what disables
    // `PER_OBJECT_LAYOUTS_NONEMPTY` for the rest of the process.
    if let Some(started) = bootstrap_started {
        let slot_masks = crate::gc::per_object_layout_table_sizes();
        eprintln!(
            "[gc-globalthis-bootstrap] elapsed_us={} per_object_slot_masks={}",
            started.elapsed().as_micros(),
            slot_masks
        );
    }
}

/// Install `%TypedArray%.prototype.toString` as the same closure object as
/// `Array.prototype.toString` (ECMA-262 §23.2.3.33).
fn alias_typed_array_proto_to_string(singleton_at_entry: *mut ObjectHeader) {
    let ta_proto_addr = crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if ta_proto_addr == 0 {
        return;
    }
    // #6982: `js_string_from_bytes` allocates, so every pointer held across the
    // lookups below can be relocated by an evacuating minor. Root them and read
    // back through the handles. `TYPED_ARRAY_INTRINSIC_PROTO_PTR` is itself a
    // scanned root, but the local copy taken above is not.
    let scope = crate::gc::RuntimeHandleScope::new();
    let singleton_handle = scope.root_raw_mut_ptr(singleton_at_entry);
    let singleton = || singleton_handle.get_raw_mut_ptr::<ObjectHeader>();
    let ta_proto_handle = scope.root_raw_mut_ptr(ta_proto_addr as *mut ObjectHeader);

    // Read Array constructor from globalThis, then Array.prototype.toString.
    let arr_key = crate::string::js_string_from_bytes(b"Array".as_ptr(), 5);
    let arr_ctor = js_object_get_field_by_name(singleton(), arr_key);
    if (arr_ctor.bits() >> 48) != 0x7FFD {
        return;
    }
    let arr_ctor_ptr = (arr_ctor.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if arr_ctor_ptr.is_null() {
        return;
    }
    let arr_ctor_handle = scope.root_raw_mut_ptr(arr_ctor_ptr);

    let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let arr_proto =
        js_object_get_field_by_name(arr_ctor_handle.get_raw_mut_ptr::<ObjectHeader>(), proto_key);
    if (arr_proto.bits() >> 48) != 0x7FFD {
        return;
    }
    let arr_proto_ptr = (arr_proto.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if arr_proto_ptr.is_null() {
        return;
    }
    let arr_proto_handle = scope.root_raw_mut_ptr(arr_proto_ptr);

    let ts_key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let to_string_fn =
        js_object_get_field_by_name(arr_proto_handle.get_raw_mut_ptr::<ObjectHeader>(), ts_key);
    if to_string_fn.bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let to_string_handle = scope.root_nanbox_u64(to_string_fn.bits());

    let ts_key2 = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    super::super::define_builtin_data_property(
        ta_proto_handle.get_raw_mut_ptr::<ObjectHeader>(),
        ts_key2,
        f64::from_bits(to_string_handle.get_nanbox_u64()),
        "toString".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

/// Re-point a `Number.<name>` static at the global function of the same name so
/// the two are the identical object (`Number.parseFloat === parseFloat`). Both
/// the global helper and the `Number` constructor are already installed on the
/// `singleton` by the time this runs. No-op if either lookup fails.
fn alias_number_static_to_global_function(singleton_at_entry: *mut ObjectHeader, name: &str) {
    // #6982: same window as `populate_global_this_builtins` — every
    // `js_string_from_bytes` here can trigger an evacuating minor that relocates
    // `singleton`, the resolved global function and the `Number` constructor.
    // Root each across the allocations and re-read through the handles.
    let scope = crate::gc::RuntimeHandleScope::new();
    let singleton_handle = scope.root_raw_mut_ptr(singleton_at_entry);
    let singleton = || singleton_handle.get_raw_mut_ptr::<ObjectHeader>();

    let global_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let global_fn = js_object_get_field_by_name(singleton(), global_key);
    if (global_fn.bits() >> 48) != 0x7FFD {
        return;
    }
    let global_fn_handle = scope.root_nanbox_u64(global_fn.bits());

    let number_key = crate::string::js_string_from_bytes(b"Number".as_ptr(), 6);
    let number_ctor = js_object_get_field_by_name(singleton(), number_key);
    if (number_ctor.bits() >> 48) != 0x7FFD {
        return;
    }
    let ctor_ptr = (number_ctor.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if ctor_ptr.is_null() {
        return;
    }
    let ctor_handle = scope.root_raw_mut_ptr(ctor_ptr);

    let static_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let ctor_ptr = ctor_handle.get_raw_mut_ptr::<ObjectHeader>();
    js_object_set_field_by_name(
        ctor_ptr,
        static_key,
        f64::from_bits(global_fn_handle.get_nanbox_u64()),
    );
    super::super::set_builtin_property_attrs(
        ctor_handle.get_raw_mut_ptr::<ObjectHeader>() as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

crate::perry_thread_local! {
    /// Raw address of THIS thread's `Error` constructor closure, captured at
    /// install. Read by `error::error_prepare_stack_trace_override` so
    /// `captureStackTrace` / `error.stack` can honor a user-set
    /// `Error.prepareStackTrace`. Thread-local, not a process-global: each
    /// `perry/thread` agent has its own arena + realm, and an `Error`
    /// constructor / `prepareStackTrace` from another thread's arena can be a
    /// foreign or freed pointer — the same reason `globalThis` is per-thread.
    ///
    /// **This is a GC root, and must stay one (#7231).** The address is a RAW
    /// `*mut ClosureHeader` from `js_closure_alloc` — a nursery allocation.
    /// The canonical `Error` closure is also a field of `globalThis`, so the
    /// structural trace keeps it alive and rewrites THAT reference; this
    /// duplicate lives outside the object graph, so an evacuating collection
    /// leaves it naming from-space and `error_prepare_stack_trace_override`
    /// then reads `prepareStackTrace` off an abandoned closure. Rooted by
    /// `scan_error_constructor_root_mut`.
    pub(crate) static ERROR_CONSTRUCTOR_PTR: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Root + rewrite the raw `Error` constructor address.
pub(crate) fn scan_error_constructor_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ERROR_CONSTRUCTOR_PTR.with(|cell| {
        let mut addr = cell.get();
        if addr != 0 && visitor.visit_usize_slot(&mut addr) {
            cell.set(addr);
        }
    });
}

/// The default `Error.prepareStackTrace` thunk's address — used to tell a
/// user override apart from Perry's built-in default.
pub(crate) fn default_prepare_stack_trace_func_ptr() -> usize {
    global_this_error_prepare_stack_trace_thunk as *const u8 as usize
}

fn install_error_static_methods(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    ERROR_CONSTRUCTOR_PTR.with(|c| c.set(ctor as usize));
    let closure = crate::closure::js_closure_alloc(
        crate::fn_info!(global_this_error_capture_stack_trace_thunk, 2; with_declared(2)),
        0,
    );
    if closure.is_null() {
        return;
    }
    super::super::native_module::set_bound_native_closure_name(closure, "captureStackTrace");

    let key = crate::string::js_string_from_bytes(b"captureStackTrace".as_ptr(), 17);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    super::super::define_builtin_data_property(
        ctor as *mut ObjectHeader,
        key,
        value,
        "captureStackTrace".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    // #2904: `Error.isError` — V8/Node Error duck-check.
    install_error_static_fn(
        ctor,
        "isError",
        crate::fn_info!(global_this_error_is_error_thunk, 1; with_declared(1)),
    );

    // #2904: `Error.prepareStackTrace` — default stack-formatting hook.
    install_error_static_fn(
        ctor,
        "prepareStackTrace",
        crate::fn_info!(global_this_error_prepare_stack_trace_thunk, 2; with_declared(2)),
    );

    // #2904: `Error.stackTraceLimit` — writable number controlling captured
    // frame count. Node's default is 10; Perry's stacks are coarse but the
    // property must read as a number and be writable.
    let limit_key = crate::string::js_string_from_bytes(b"stackTraceLimit".as_ptr(), 15);
    super::super::define_builtin_data_property(
        ctor as *mut ObjectHeader,
        limit_key,
        10.0,
        "stackTraceLimit".to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

/// #2904: install a callable static method on the `Error` constructor closure
/// as a non-enumerable, writable, configurable data property (matching Node's
/// property descriptors for the V8 static helpers).
fn install_error_static_fn(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    info: *const crate::closure::JsFunctionInfo,
) {
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return;
    }
    super::super::native_module::set_bound_native_closure_name(closure, name);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    super::super::define_builtin_data_property(
        ctor as *mut ObjectHeader,
        key,
        value,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

// =====================================================================
// #2889: static methods on rebound global built-in constructor values.
//
// `const O = Object; O.keys(x)` reads `keys` off the `Object` constructor
// closure's dynamic-prop side table, then calls it. Pre-fix nothing was
// installed there, so the read returned `undefined`. These thunks delegate
// to the same runtime helpers the direct `Object.keys(x)` lowering uses.
// =====================================================================
