use super::*;

/// Apply the builtin's declared identity and its members' declared permanent
/// images to the finished holder. No public name participates in either fact.
fn finish_builtin_prototype(
    declaration: &BuiltinConstructorDeclaration,
    proto_obj: *mut ObjectHeader,
) {
    // These ordinary native receivers carry CLASS identities, just
    // like declared-class instances. Publish the intrinsic holder in
    // the identity's GC-traced word when it becomes available.
    if let Some(class) = declaration.prototype_class {
        crate::object::shapes::write_identity_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(class),
            crate::value::js_nanbox_pointer(proto_obj as i64).to_bits(),
        );
    }
    // Every builtin prototype, after the fit (its live-bound
    // transition keeps no ConstFn lane): the shape names the body of
    // each member whose function info declares a permanent image
    // (`FN_PERMANENT_IMAGE`), as an ordinary key-add of that function
    // object would. The declaration on the member decides, never the
    // prototype's name; a store or delete of the member revokes its
    // lane (a new ShapeId).
    // A prototype born on its declared layout keeps that layout id: its
    // members are its layout's slots, never lanes of a new id.
    // SAFETY: the live, just-populated prototype.
    unsafe {
        let learn = |obj: *mut ObjectHeader| {
            if !crate::object::runtime_layouts::on_declared_prototype_layout(obj) {
                crate::object::shapes::learn_object_constfn_lanes(obj, |_, _| true);
            }
        };
        if crate::value::addr_class::try_read_gc_header(proto_obj as usize)
            .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
        {
            learn(proto_obj);
        } else {
            crate::object::shapes::learn_object_constfn_lanes(proto_obj, |_, _| true);
        }
        // Some native prototypes own symbol slots through an ordinary
        // descriptor holder. Its members carry the same declarations.
        if let Some(holder) = crate::object::shaped_symbols::owner(proto_obj as usize) {
            if holder != proto_obj {
                learn(holder);
            }
        }
    }
}

#[cfg(test)]
#[path = "declaration_tests.rs"]
mod declaration_tests;

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
crate::realm_decl! { Infinity, value = Infinity; NaN, value = NaN; crypto, attrs = 6, get = "crypto.<get>", births = [ArrayBuffer, Crypto, CryptoKey, Promise, SubtleCrypto]; global, attrs = 7, alias = "%Global%"; globalThis, alias = "%Global%"; localStorage, attrs = 6, get = "localStorage.<get>", set = "localStorage.<set>"; sessionStorage, attrs = 6, get = "sessionStorage.<get>", set = "sessionStorage.<set>"; undefined, value = undefined; }
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
    let bootstrap_started = crate::gc::diagnostic_hooks::enabled().then(std::time::Instant::now);
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
    for declaration in GLOBAL_THIS_BUILTIN_CONSTRUCTORS {
        let name = declaration.name;
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
        let closure_ptr =
            super::super::global_this_tables::alloc_builtin_constructor(declaration.identity);
        if closure_ptr.is_null() {
            continue;
        }
        // #2889: install static methods (`Object.keys`, `Array.isArray`, ...)
        // on the constructor closure so rebound usage like
        // `const O = Object; O.keys(x)` dispatches through the real helpers.
        // The prototype is an own static data property too. Include it in
        // the initial attributed layout so adding name/length cannot spill it.
        let proto_obj = if name == "Array" {
            // A runtime with no compiled entry (the Rust tests) registers the
            // static layouts at realm bootstrap, as `js_shape_run_static_seed`
            // does before any user code (§6a); `%Array.prototype%` is born on
            // one of them.
            #[cfg(test)]
            crate::object::layout_desc::register_static_layouts();
            let arr = crate::array::js_array_alloc(0);
            // SAFETY: the fresh intrinsic, under this bootstrap's no-move scope.
            unsafe {
                crate::array::array_prototype_property_bag_ensure(
                    arr,
                    super::proto_room::BUILTIN_PROTOTYPE_ROOM,
                );
            }
            arr as *mut ObjectHeader
        } else if name == "Function" {
            // Born on its static layout (`runtime_layouts::FUNCTION_PROTOTYPE`,
            // a callable ordinary kind); the population fills its slots.
            // SAFETY: a registered runtime descriptor with the default prototype.
            unsafe {
                crate::realm_decl! { birth crate::object::layout_desc::alloc_layout_prototype(
                    std::ptr::addr_of!(crate::object::runtime_layouts::FUNCTION_PROTOTYPE),
                    0,
                ); realm { Function.prototype, members = { length = 0.0, attrs = configurable; name = "", attrs = configurable; arguments = accessor(get = "Function.prototype.arguments.<get>", set = "Function.prototype.arguments.<get>"); caller = accessor(get = "Function.prototype.arguments.<get>", set = "Function.prototype.arguments.<get>"); constructor; apply; bind; call; toString; [Symbol.hasInstance], attrs = writable | enumerable | configurable; }; } }
            }
        } else {
            crate::realm_decl! { birth super::proto_room::alloc_builtin_prototype(); realm { AbortController.prototype, members = { constructor; signal = accessor(get = "AbortController.prototype.signal.<get>"), attrs = enumerable | configurable; abort, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "AbortController"; }; AbortSignal.prototype, extends = EventTarget.prototype, members = { constructor; aborted = accessor(get = "AbortSignal.prototype.aborted.<get>"), attrs = enumerable | configurable; reason = accessor(get = "AbortSignal.prototype.reason.<get>"); throwIfAborted; onabort = accessor(get = "AbortSignal.prototype.onabort.<get>", set = "AbortSignal.prototype.onabort.<set>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "AbortSignal"; }; AggregateError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "AggregateError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; ArrayBuffer.prototype, members = { constructor; slice; resize; transfer; transferToFixedLength; byteLength = accessor(get = "ArrayBuffer.prototype.byteLength.<get>"); resizable = accessor(get = "ArrayBuffer.prototype.resizable.<get>"); maxByteLength = accessor(get = "ArrayBuffer.prototype.maxByteLength.<get>"); detached = accessor(get = "ArrayBuffer.prototype.detached.<get>"); toLocaleString; valueOf; [Symbol.toStringTag] = "ArrayBuffer"; }; AsyncDisposableStack.prototype, members = { constructor; disposed = accessor(get = "AsyncDisposableStack.prototype.disposed.<get>"); use; adopt; defer; disposeAsync; move; toLocaleString; valueOf; [Symbol.asyncDispose] = ref(AsyncDisposableStack.prototype.disposeAsync); [Symbol.toStringTag] = "AsyncDisposableStack"; }; BigInt.prototype, members = { constructor; toString; valueOf; toLocaleString; }; Blob.prototype, members = { constructor; arrayBuffer; bytes; slice; stream; text; toLocaleString; valueOf; [Symbol.toStringTag] = "Blob"; }; Boolean.prototype, members = { constructor; toString; valueOf; toLocaleString; }; BroadcastChannel.prototype, members = { constructor; name = undefined, attrs = writable | enumerable | configurable; close, attrs = writable | enumerable | configurable; postMessage, attrs = writable | enumerable | configurable; ref, attrs = writable | enumerable | configurable; unref, attrs = writable | enumerable | configurable; onmessage = null, attrs = writable | enumerable | configurable; onmessageerror = null, attrs = writable | enumerable | configurable; }; CompressionStream.prototype, members = { constructor; }; Crypto.prototype, members = { constructor; subtle = accessor(get = "Crypto.prototype.subtle.<get>"), attrs = enumerable | configurable; getRandomValues, attrs = writable | enumerable | configurable; randomUUID, attrs = writable | enumerable | configurable; toLocaleString; valueOf; }; CryptoKey.prototype, members = { constructor; algorithm = accessor(get = "CryptoKey.prototype.algorithm.<get>"), attrs = enumerable | configurable; extractable = accessor(get = "CryptoKey.prototype.extractable.<get>"), attrs = enumerable | configurable; type = accessor(get = "CryptoKey.prototype.type.<get>"), attrs = enumerable | configurable; usages = accessor(get = "CryptoKey.prototype.usages.<get>"), attrs = enumerable | configurable; toLocaleString; valueOf; }; CustomEvent.prototype, extends = Event.prototype, members = { constructor; detail = accessor(get = "CustomEvent.prototype.detail.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "CustomEvent"; }; DOMException.prototype, extends = Error.prototype, members = { constructor; name = accessor(get = "DOMException.prototype.name.<get>"), attrs = enumerable | configurable; message = accessor(get = "DOMException.prototype.message.<get>"), attrs = enumerable | configurable; code = accessor(get = "DOMException.prototype.code.<get>"), attrs = enumerable | configurable; INDEX_SIZE_ERR = 1.0, attrs = enumerable; DOMSTRING_SIZE_ERR = 2.0, attrs = enumerable; HIERARCHY_REQUEST_ERR = 3.0, attrs = enumerable; WRONG_DOCUMENT_ERR = 4.0, attrs = enumerable; INVALID_CHARACTER_ERR = 5.0, attrs = enumerable; NO_DATA_ALLOWED_ERR = 6.0, attrs = enumerable; NO_MODIFICATION_ALLOWED_ERR = 7.0, attrs = enumerable; NOT_FOUND_ERR = 8.0, attrs = enumerable; NOT_SUPPORTED_ERR = 9.0, attrs = enumerable; INUSE_ATTRIBUTE_ERR = 10.0, attrs = enumerable; INVALID_STATE_ERR = 11.0, attrs = enumerable; SYNTAX_ERR = 12.0, attrs = enumerable; INVALID_MODIFICATION_ERR = 13.0, attrs = enumerable; NAMESPACE_ERR = 14.0, attrs = enumerable; INVALID_ACCESS_ERR = 15.0, attrs = enumerable; VALIDATION_ERR = 16.0, attrs = enumerable; TYPE_MISMATCH_ERR = 17.0, attrs = enumerable; SECURITY_ERR = 18.0, attrs = enumerable; NETWORK_ERR = 19.0, attrs = enumerable; ABORT_ERR = 20.0, attrs = enumerable; URL_MISMATCH_ERR = 21.0, attrs = enumerable; QUOTA_EXCEEDED_ERR = 22.0, attrs = enumerable; TIMEOUT_ERR = 23.0, attrs = enumerable; INVALID_NODE_TYPE_ERR = 24.0, attrs = enumerable; DATA_CLONE_ERR = 25.0, attrs = enumerable; [Symbol.toStringTag] = "DOMException"; }; DataView.prototype, members = { constructor; byteLength = accessor(get = "DataView.prototype.byteLength.<get>"); byteOffset = accessor(get = "DataView.prototype.byteOffset.<get>"); buffer = accessor(get = "DataView.prototype.buffer.<get>"); getInt8; getUint8; getInt16; getUint16; getInt32; getUint32; getFloat32; getFloat64; getBigInt64; getBigUint64; setInt8; setUint8; setInt16; setUint16; setInt32; setUint32; setFloat32; setFloat64; setBigInt64; setBigUint64; toLocaleString; valueOf; [Symbol.toStringTag] = "DataView"; }; Date.prototype, members = { constructor; getDate; getDay; getFullYear; getHours; getMilliseconds; getMinutes; getMonth; getSeconds; getTime; getTimezoneOffset; getUTCDate; getUTCDay; getUTCFullYear; getUTCHours; getUTCMilliseconds; getUTCMinutes; getUTCMonth; getUTCSeconds; getYear; setDate; setFullYear; setHours; setMilliseconds; setMinutes; setMonth; setSeconds; setTime; setUTCDate; setUTCFullYear; setUTCHours; setUTCMilliseconds; setUTCMinutes; setUTCMonth; setUTCSeconds; setYear; toDateString; toISOString; toJSON; toLocaleDateString; toLocaleString; toLocaleTimeString; toTimeString; toUTCString = ref(Date.prototype.toGMTString); valueOf; toTemporalInstant; toGMTString; isPrototypeOf; toString; [Symbol.toPrimitive], attrs = configurable; }; DecompressionStream.prototype, members = { constructor; }; DisposableStack.prototype, members = { constructor; disposed = accessor(get = "DisposableStack.prototype.disposed.<get>"); use; adopt; defer; dispose; move; toLocaleString; valueOf; [Symbol.dispose] = ref(DisposableStack.prototype.dispose); [Symbol.toStringTag] = "DisposableStack"; }; Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "Error", attrs = writable | configurable; message = "", attrs = writable | configurable; }; EvalError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "EvalError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; Event.prototype, members = { constructor; initEvent, attrs = writable | enumerable | configurable; stopImmediatePropagation, attrs = writable | enumerable | configurable; preventDefault, attrs = writable | enumerable | configurable; target = accessor(get = "Event.prototype.target.<get>"), attrs = enumerable | configurable; currentTarget = accessor(get = "Event.prototype.currentTarget.<get>"), attrs = enumerable | configurable; srcElement = accessor(get = "Event.prototype.srcElement.<get>"), attrs = enumerable | configurable; type = accessor(get = "Event.prototype.type.<get>"), attrs = enumerable | configurable; cancelable = accessor(get = "Event.prototype.cancelable.<get>"), attrs = enumerable | configurable; defaultPrevented = accessor(get = "Event.prototype.defaultPrevented.<get>"), attrs = enumerable | configurable; timeStamp = accessor(get = "Event.prototype.timeStamp.<get>"), attrs = enumerable | configurable; composedPath, attrs = writable | enumerable | configurable; returnValue = accessor(get = "Event.prototype.returnValue.<get>", set = "Event.prototype.returnValue.<set>"), attrs = enumerable | configurable; bubbles = accessor(get = "Event.prototype.bubbles.<get>"), attrs = enumerable | configurable; composed = accessor(get = "Event.prototype.composed.<get>"), attrs = enumerable | configurable; eventPhase = accessor(get = "Event.prototype.eventPhase.<get>"), attrs = enumerable | configurable; cancelBubble = accessor(get = "Event.prototype.cancelBubble.<get>", set = "Event.prototype.cancelBubble.<set>"), attrs = enumerable | configurable; stopPropagation, attrs = writable | enumerable | configurable; isTrusted = accessor(get = "Event.prototype.isTrusted.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "Event"; }; EventTarget.prototype, members = { constructor; addEventListener, attrs = writable | enumerable | configurable; removeEventListener, attrs = writable | enumerable | configurable; dispatchEvent, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "EventTarget"; }; File.prototype, members = { constructor; arrayBuffer; bytes; slice; stream; text; toLocaleString; valueOf; [Symbol.toStringTag] = "File"; }; FinalizationRegistry.prototype, members = { constructor; register; unregister; toLocaleString; valueOf; }; FormData.prototype, members = { constructor; append; delete; entries; forEach; get; getAll; has; keys; set; values; toLocaleString; valueOf; [Symbol.toStringTag] = "FormData"; }; Headers.prototype, members = { constructor; entries; append; delete; forEach; get; getSetCookie; has; keys; set; values; toLocaleString; valueOf; [Symbol.iterator] = ref(Headers.prototype.entries); [Symbol.toStringTag] = "Headers"; }; Map.prototype, members = { constructor; size = accessor(get = "Map.prototype.size.<get>"); clear; delete; entries; forEach; get; has; keys; set; values; toLocaleString; valueOf; [Symbol.iterator] = ref(Map.prototype.entries); [Symbol.toStringTag] = "Map"; }; MessageChannel.prototype, members = { constructor; }; MessagePort.prototype, members = { constructor; postMessage, attrs = writable | enumerable | configurable; start, attrs = writable | enumerable | configurable; ref, attrs = writable | enumerable | configurable; unref, attrs = writable | enumerable | configurable; hasRef, attrs = writable | enumerable | configurable; onmessage = null, attrs = writable | enumerable | configurable; onmessageerror = null, attrs = writable | enumerable | configurable; close, attrs = writable | enumerable | configurable; }; Navigator.prototype, members = { constructor; }; Number.prototype, members = { constructor; toExponential; toFixed; toLocaleString; toPrecision; toString; valueOf; }; Promise.prototype, members = { constructor; catch; finally; then; toLocaleString; valueOf; [Symbol.toStringTag] = "Promise"; }; Proxy.prototype, members = { constructor; }; RangeError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "RangeError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; ReadableStream.prototype, members = { constructor; }; ReferenceError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "ReferenceError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; RegExp.prototype, members = { constructor; flags = accessor(get = "RegExp.prototype.flags.<get>"); source = accessor(get = "RegExp.prototype.source.<get>"); global = accessor(get = "RegExp.prototype.global.<get>"); ignoreCase = accessor(get = "RegExp.prototype.ignoreCase.<get>"); multiline = accessor(get = "RegExp.prototype.multiline.<get>"); dotAll = accessor(get = "RegExp.prototype.dotAll.<get>"); sticky = accessor(get = "RegExp.prototype.sticky.<get>"); unicode = accessor(get = "RegExp.prototype.unicode.<get>"); unicodeSets = accessor(get = "RegExp.prototype.unicodeSets.<get>"); hasIndices = accessor(get = "RegExp.prototype.hasIndices.<get>"); exec; test; compile; toString; toLocaleString; valueOf; [Symbol.match]; [Symbol.search]; [Symbol.matchAll]; [Symbol.split]; [Symbol.replace]; }; Request.prototype, members = { constructor; arrayBuffer; blob; bytes; clone; formData; json; text; body = accessor(get = "Request.prototype.body.<get>"); bodyUsed = accessor(get = "Request.prototype.bodyUsed.<get>"); cache = accessor(get = "Request.prototype.cache.<get>"); credentials = accessor(get = "Request.prototype.credentials.<get>"); destination = accessor(get = "Request.prototype.destination.<get>"); duplex = accessor(get = "Request.prototype.duplex.<get>"); headers = accessor(get = "Request.prototype.headers.<get>"); integrity = accessor(get = "Request.prototype.integrity.<get>"); keepalive = accessor(get = "Request.prototype.keepalive.<get>"); method = accessor(get = "Request.prototype.method.<get>"); mode = accessor(get = "Request.prototype.mode.<get>"); redirect = accessor(get = "Request.prototype.redirect.<get>"); referrer = accessor(get = "Request.prototype.referrer.<get>"); referrerPolicy = accessor(get = "Request.prototype.referrerPolicy.<get>"); signal = accessor(get = "Request.prototype.signal.<get>"); url = accessor(get = "Request.prototype.url.<get>"); toLocaleString; valueOf; [Symbol.toStringTag] = "Request"; }; Response.prototype, members = { constructor; arrayBuffer; blob; bytes; clone; formData; json; text; body = accessor(get = "Response.prototype.body.<get>"); bodyUsed = accessor(get = "Response.prototype.bodyUsed.<get>"); headers = accessor(get = "Response.prototype.headers.<get>"); ok = accessor(get = "Response.prototype.ok.<get>"); redirected = accessor(get = "Response.prototype.redirected.<get>"); status = accessor(get = "Response.prototype.status.<get>"); statusText = accessor(get = "Response.prototype.statusText.<get>"); type = accessor(get = "Response.prototype.type.<get>"); url = accessor(get = "Response.prototype.url.<get>"); toLocaleString; valueOf; [Symbol.toStringTag] = "Response"; }; Set.prototype, members = { constructor; size = accessor(get = "Set.prototype.size.<get>"); add; clear; delete; entries; forEach; has; keys; values = ref(Set.prototype.keys); toLocaleString; valueOf; [Symbol.iterator] = ref(Set.prototype.keys); [Symbol.toStringTag] = "Set"; }; SharedArrayBuffer.prototype, members = { constructor; slice; byteLength = accessor(get = "SharedArrayBuffer.prototype.byteLength.<get>"); toLocaleString; valueOf; [Symbol.toStringTag] = "SharedArrayBuffer"; }; Storage.prototype, members = { constructor; clear, attrs = writable | enumerable | configurable; getItem, attrs = writable | enumerable | configurable; key, attrs = writable | enumerable | configurable; removeItem, attrs = writable | enumerable | configurable; setItem, attrs = writable | enumerable | configurable; length = accessor(get = "Storage.prototype.length.<get>"), attrs = enumerable; }; String.prototype, members = { constructor; at; charAt; charCodeAt; codePointAt; anchor; big; blink; bold; fixed; fontcolor; fontsize; italics; link; small; strike; sub; sup; concat; endsWith; includes; indexOf; isWellFormed; lastIndexOf; localeCompare; match; matchAll; normalize; padEnd; padStart; repeat; replace; replaceAll; search; slice; split; startsWith; substr; substring; toLocaleLowerCase; toLocaleUpperCase; toLowerCase; toUpperCase; toWellFormed; trim; trimEnd; trimStart = ref(String.prototype.trimLeft); trimLeft; trimRight = ref(String.prototype.trimEnd); toLocaleString; valueOf; toString; [Symbol.iterator]; }; SubtleCrypto.prototype, members = { constructor; encapsulateBits, attrs = writable | enumerable | configurable; decapsulateBits, attrs = writable | enumerable | configurable; encapsulateKey, attrs = writable | enumerable | configurable; decapsulateKey, attrs = writable | enumerable | configurable; toLocaleString; valueOf; }; SuppressedError.prototype, extends = Error.prototype, members = { constructor; name = "SuppressedError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; Symbol.prototype, members = { constructor; toString; valueOf; toLocaleString; }; SyntaxError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "SyntaxError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; TextDecoder.prototype, members = { constructor; decode, attrs = writable | enumerable | configurable; encoding = accessor(get = "TextDecoder.prototype.encoding.<get>"), attrs = enumerable | configurable; fatal = accessor(get = "TextDecoder.prototype.fatal.<get>"), attrs = enumerable | configurable; ignoreBOM = accessor(get = "TextDecoder.prototype.ignoreBOM.<get>"), attrs = enumerable | configurable; toLocaleString; valueOf; [Symbol.toStringTag] = "TextDecoder"; }; TextDecoderStream.prototype, members = { constructor; }; TextEncoder.prototype, members = { constructor; encode, attrs = writable | enumerable | configurable; encodeInto, attrs = writable | enumerable | configurable; encoding = accessor(get = "TextEncoder.prototype.encoding.<get>"), attrs = enumerable | configurable; toLocaleString; valueOf; [Symbol.toStringTag] = "TextEncoder"; }; TextEncoderStream.prototype, members = { constructor; }; TransformStream.prototype, members = { constructor; }; TypeError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "TypeError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; URIError.prototype, extends = Error.prototype, members = { constructor; toLocaleString; valueOf; toString; isPrototypeOf; hasOwnProperty; name = "URIError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; URL.prototype, members = { constructor; toString, attrs = writable | enumerable | configurable; href = accessor(get = "URL.prototype.href.<get>", set = "URL.prototype.href.<set>"), attrs = enumerable | configurable; origin = accessor(get = "URL.prototype.origin.<get>"), attrs = enumerable | configurable; protocol = accessor(get = "URL.prototype.protocol.<get>", set = "URL.prototype.protocol.<set>"), attrs = enumerable | configurable; username = accessor(get = "URL.prototype.username.<get>", set = "URL.prototype.username.<set>"), attrs = enumerable | configurable; password = accessor(get = "URL.prototype.password.<get>", set = "URL.prototype.password.<set>"), attrs = enumerable | configurable; host = accessor(get = "URL.prototype.host.<get>", set = "URL.prototype.host.<set>"), attrs = enumerable | configurable; hostname = accessor(get = "URL.prototype.hostname.<get>", set = "URL.prototype.hostname.<set>"), attrs = enumerable | configurable; port = accessor(get = "URL.prototype.port.<get>", set = "URL.prototype.port.<set>"), attrs = enumerable | configurable; pathname = accessor(get = "URL.prototype.pathname.<get>", set = "URL.prototype.pathname.<set>"), attrs = enumerable | configurable; search = accessor(get = "URL.prototype.search.<get>", set = "URL.prototype.search.<set>"), attrs = enumerable | configurable; searchParams = accessor(get = "URL.prototype.searchParams.<get>"), attrs = enumerable | configurable; hash = accessor(get = "URL.prototype.hash.<get>", set = "URL.prototype.hash.<set>"), attrs = enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "URL"; }; URLPattern.prototype, members = { constructor; exec; test; hasRegExpGroups = undefined, attrs = configurable; hash = undefined, attrs = configurable; hostname = undefined, attrs = configurable; password = undefined, attrs = configurable; pathname = undefined, attrs = configurable; port = undefined, attrs = configurable; protocol = undefined, attrs = configurable; search = undefined, attrs = configurable; username = undefined, attrs = configurable; }; URLSearchParams.prototype, members = { constructor; append; delete; entries; forEach; get; getAll; has; keys; set; sort; toString; values; size = accessor(get = "URLSearchParams.prototype.size.<get>"), attrs = enumerable | configurable; toLocaleString; valueOf; [Symbol.toStringTag] = "URLSearchParams"; }; WeakMap.prototype, members = { constructor; delete; get; has; set; toLocaleString; valueOf; [Symbol.toStringTag] = "WeakMap"; }; WeakRef.prototype, members = { constructor; deref; toLocaleString; valueOf; }; WeakSet.prototype, members = { constructor; add; delete; has; toLocaleString; valueOf; [Symbol.toStringTag] = "WeakSet"; }; WebSocket.prototype, members = { constructor; close, attrs = writable | enumerable | configurable; send, attrs = writable | enumerable | configurable; toLocaleString; valueOf; CONNECTING = 0.0, attrs = enumerable; OPEN = 1.0, attrs = enumerable; CLOSING = 2.0, attrs = enumerable; CLOSED = 3.0, attrs = enumerable; }; WritableStream.prototype, members = { constructor; }; BigInt64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; BigUint64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; Float16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Float32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Float64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; Int16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Int32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Int8Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 1.0; }; Uint16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Uint32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Uint8Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; toBase64; toHex; setFromBase64; setFromHex; BYTES_PER_ELEMENT = 1.0; }; Uint8ClampedArray.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 1.0; }; } }
        };
        if declaration.prototype_callable {
            // The declaration is immutable birth metadata, independent of the
            // public name/property. Publish the callable shape and header fact.
            unsafe {
                crate::object::shapes::publish_callable_object_kind(
                    proto_obj,
                    crate::object::shapes::ShapeObjectKind::CallableOrdinary,
                );
            }
        }
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
            #[cfg(feature = "regex-engine")]
            if name == "RegExp" {
                // An intrinsic is a realm-private value, independent of the
                // writable public constructor. Ordinary private storage keeps
                // it alive and relocates it through the global object's trace.
                crate::object::intrinsic_private_add(
                    crate::value::js_nanbox_pointer(singleton() as i64),
                    "[[RegExpPrototype]]",
                    crate::value::js_nanbox_pointer(proto_obj as i64),
                );
            }
            crate::event_target_hook_slots::prototype_install_constructor_constants(
                name,
                closure_ptr.cast(),
            );
            if let Some(serial) = declaration.prototype_serial {
                // SAFETY: `proto_obj` is the live, just-populated prototype.
                unsafe {
                    crate::object::proto_validity::assign_intrinsic_prototype_serial(
                        proto_obj as usize,
                        serial,
                    );
                    name_first_realm_prototype(proto_obj, serial);
                }
            }
            if let Some(serial) = declaration.named_serial {
                // SAFETY: `proto_obj` is the live intrinsic array; its holder
                // of named properties is the object the serial names.
                unsafe {
                    let holder = crate::array::array_property_bag(
                        proto_obj as *const crate::array::ArrayHeader,
                    );
                    if !holder.is_null() {
                        name_first_realm_prototype(holder, serial);
                    }
                }
            }
            if let Some(serial) = declaration.sealed_serial {
                // SAFETY: as above; runtime layouts name it by this serial.
                unsafe { crate::object::builtin_proto::seal_runtime_prototype(proto_obj, serial) };
            }
            if name == "Function" {
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
                crate::messaging_hook_slots::populate_messaging_prototype(
                    name, proto_obj, ctor_value,
                );
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
            finish_builtin_prototype(declaration, proto_obj);
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
                crate::fn_info!(#[realm(eval)] global_this_eval_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "fetch" => (
                crate::fn_info!(#[realm(fetch)] super::super::global_fetch::global_this_fetch_thunk, 2; with_rest(1)),
                1,
                true,
            ),
            "structuredClone" => (
                crate::fn_info!(#[realm(structuredClone, births = [ArrayBuffer, DOMException, Uint8Array])] global_this_structured_clone_thunk, 2; with_declared(2)),
                2,
                true,
            ),
            "atob" => (
                crate::fn_info!(#[realm(atob, births = [DOMException])] global_this_atob_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "btoa" => (
                crate::fn_info!(#[realm(btoa, births = [DOMException])] global_this_btoa_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setTimeout" => (
                crate::fn_info!(#[realm(setTimeout)] global_this_set_timeout_thunk, 3; with_rest(2)),
                2,
                true,
            ),
            "clearTimeout" => (
                crate::fn_info!(#[realm(clearTimeout)] global_this_clear_timeout_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setInterval" => (
                crate::fn_info!(#[realm(setInterval)] global_this_set_interval_thunk, 3; with_rest(2)),
                2,
                true,
            ),
            "clearInterval" => (
                crate::fn_info!(#[realm(clearInterval)] global_this_clear_interval_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "setImmediate" => (
                crate::fn_info!(#[realm(setImmediate)] global_this_set_immediate_thunk, 2; with_rest(1)),
                1,
                true,
            ),
            "clearImmediate" => (
                crate::fn_info!(#[realm(clearImmediate)] global_this_clear_immediate_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            "queueMicrotask" => (
                crate::fn_info!(#[realm(queueMicrotask)] global_this_queue_microtask_thunk, 1; with_declared(1)),
                1,
                true,
            ),
            // `gc([force])` — value form of the bare `gc()` call-intrinsic.
            // Non-enumerable (a debug/diagnostic global). The optional `force`
            // arg is accepted (arity 1) but ignored — Perry's gc is full.
            "gc" => (
                crate::fn_info!(#[realm(gc)] global_this_gc_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            // #2905: standard global helper functions.
            "parseInt" => (
                crate::fn_info!(#[realm(parseInt)] global_this_parse_int_thunk, 2; with_declared(2)),
                2,
                false,
            ),
            "parseFloat" => (
                crate::fn_info!(#[realm(parseFloat)] global_this_parse_float_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "isNaN" => (
                crate::fn_info!(#[realm(isNaN)] global_this_is_nan_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "isFinite" => (
                crate::fn_info!(#[realm(isFinite)] global_this_is_finite_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "encodeURI" => (
                crate::fn_info!(#[realm(encodeURI)] global_this_encode_uri_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "decodeURI" => (
                crate::fn_info!(#[realm(decodeURI)] global_this_decode_uri_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "encodeURIComponent" => (
                crate::fn_info!(#[realm(encodeURIComponent)] global_this_encode_uri_component_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "decodeURIComponent" => (
                crate::fn_info!(#[realm(decodeURIComponent)] global_this_decode_uri_component_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            // #4511: legacy escape/unescape (ES Annex B).
            // #4511: legacy escape/unescape (ES Annex B).
            "escape" => (
                crate::fn_info!(#[realm(escape)] global_this_escape_thunk, 1; with_declared(1)),
                1,
                false,
            ),
            "unescape" => (
                crate::fn_info!(#[realm(unescape)] global_this_unescape_thunk, 1; with_declared(1)),
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
            crate::builtins::register_function_name(&*info, name.as_bytes());
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
        } else if name == "Math" {
            crate::value::js_nanbox_pointer(super::web_hooks::math_namespace() as i64)
        } else if name == "JSON" {
            crate::value::js_nanbox_pointer(super::web_hooks::json_namespace() as i64)
        } else if name == "Reflect" {
            crate::value::js_nanbox_pointer(super::web_hooks::reflect_namespace() as i64)
        } else if name == "WebAssembly" {
            super::global_this_webassembly::create_webassembly_namespace()
        } else {
            let ns_obj = crate::realm_decl! { birth js_object_alloc(0, 0); realm { Temporal, kind = namespace, members = { Duration; Instant; PlainDate; PlainTime; PlainDateTime; PlainYearMonth; PlainMonthDay; ZonedDateTime; Now; [Symbol.toStringTag] = "Temporal"; }, births = [Array, BigInt, Error, Number, RangeError, String, TypeError]; Intl, kind = namespace, members = { getCanonicalLocales; supportedValuesOf; Locale; NumberFormat; DateTimeFormat; Collator; Segmenter; ListFormat; RelativeTimeFormat; PluralRules; DurationFormat; DisplayNames; }, births = [Array, String]; Atomics, kind = namespace, members = { load; isLockFree; store; add; sub; and; or; xor; exchange; compareExchange; notify; wait; waitAsync; [Symbol.toStringTag] = "Atomics"; }, births = [ArrayBuffer, Int32Array, Promise]; } };
            if ns_obj.is_null() {
                continue;
            }
            // #4139 + #4149: reify each namespace's own members as real
            // properties so the reflection APIs (`getOwnPropertyDescriptor`,
            // `getOwnPropertyNames`) observe them. Call sites (`Math.max(...)`,
            // `JSON.stringify(...)`, `Reflect.get(...)`) are codegen intrinsics
            // gated on the AST shape and never read these fields. Complete
            // Math/JSON/Reflect namespaces are constructed above.
            match name {
                "Atomics" => {
                    super::web_hooks::atomics_members(ns_obj);
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
        let pval = crate::perf_hook_slots::performance_namespace();
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
        crate::fn_info!(#[realm("crypto.<get>", provider = closure_alloc)] global_this_crypto_getter_thunk, 0; with_declared(0)),
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
    // #11915: every runtime-owned install and link above is done; from here a
    // mutation of an intrinsic prototype is the program's, and withdraws its
    // family's `Pristine` fact.
    crate::object::builtin_proto::seal_realm(singleton());
    if let Some(started) = bootstrap_started {
        eprintln!(
            "[gc-globalthis-bootstrap] elapsed_us={}",
            started.elapsed().as_micros()
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
        crate::fn_info!(#[realm(Error.captureStackTrace, constructible = true)] global_this_error_capture_stack_trace_thunk, 2; with_declared(2)),
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
        crate::fn_info!(#[realm(Error.isError, constructible = true)] global_this_error_is_error_thunk, 1; with_declared(1)),
    );

    // #2904: `Error.prepareStackTrace` — default stack-formatting hook.
    install_error_static_fn(
        ctor,
        "prepareStackTrace",
        crate::fn_info!(#[realm(Error.prepareStackTrace, constructible = true)] global_this_error_prepare_stack_trace_thunk, 2; with_declared(2)),
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

/// Give the agent's first realm's builtin prototype `proto` its reserved
/// intrinsic `serial` and write the serial's identity word, which names it
/// for the agent's life (`builtin_proto::name_runtime_prototype`, the
/// encoding of runtime prototype families): the unknown-receiver probe reaches
/// `%Function.prototype%` and `%Array.prototype%`'s holder through it, as the
/// slow reads reach the first realm's intrinsics (`array::prototype_addr`).
///
/// # Safety
/// `proto` is a live ordinary object whose serial is 0 or `serial`.
unsafe fn name_first_realm_prototype(proto: *mut ObjectHeader, serial: u64) {
    if crate::object::shapes::identity_word_slot(serial).is_some_and(|slot| *slot != 0) {
        return;
    }
    crate::object::proto_validity::assign_intrinsic_prototype_serial(proto as usize, serial);
    let _ = crate::object::builtin_proto::name_runtime_prototype(proto, serial);
}

crate::realm_decl!("populate.realm.json#*");
