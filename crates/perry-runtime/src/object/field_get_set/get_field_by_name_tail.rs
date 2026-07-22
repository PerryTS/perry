//! Object-deref tail of `js_object_get_field_by_name`: pointer-strip,
//! handle dispatch, and the full ObjectHeader property walk. Extracted
//! verbatim from field_get_set.rs (issue #1103 split) so neither half
//! exceeds the file-size budget. Pure relocation — no logic change.

use super::*;

/// Tail of `js_object_get_field_by_name` (everything after the leading
/// primitive/handle/Date receiver guards). Body moved verbatim.
pub(crate) fn get_field_by_name_object_tail(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                // undefined/null tag or null pointer — return undefined
                return JSValue::undefined();
            }
            // Issue #340: small-handle receivers (raw < 0x100000) come
            // from native modules (axios, fastify, ioredis, ...) that
            // store objects in registries and expose integer ids. The
            // handle property dispatcher (registered by stdlib via
            // `js_register_handle_property_dispatch`) routes the
            // property name to the per-module accessor (e.g. axios
            // status/data, fastify req query/params/...). Without
            // this, every property access on those handles silently
            // returned undefined.
            if crate::value::addr_class::is_small_handle(raw as usize) {
                if !key.is_null() {
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if is_timer_handle_method_key(key_bytes)
                            && crate::timer::is_known_timer_id(raw as i64)
                        {
                            let this_f64 = f64::from_bits(
                                crate::value::js_nanbox_pointer(raw as i64).to_bits(),
                            );
                            let result =
                                super::super::js_class_method_bind(this_f64, key_ptr, key_len);
                            return JSValue::from_bits(result.to_bits());
                        }
                        if let Some(v) = crate::text::text_handle_property(
                            raw as usize,
                            key_bytes,
                            key_ptr,
                            key_len,
                        ) {
                            return v;
                        }
                    }
                    // Drizzle-sqlite blocker: synth `data.constructor` for
                    // small-handle native instances so drizzle's
                    // `isConfig(data)` duck-type via
                    // `data.constructor.name !== "Object"` doesn't crash on
                    // `(undefined).name` under #648's strict catch-all.
                    // Returning the existing NULL_OBJECT_BYTES stub (a real
                    // ObjectHeader-shape with no fields) makes `(stub).name`
                    // return undefined safely, and `undefined !== "Object"`
                    // makes isConfig return false at the first gate. Refs
                    // #645 deeper followup.
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if key_bytes == b"constructor" {
                            if let Some(dispatch) = handle_property_dispatch() {
                                let bits = dispatch(raw as i64, key_ptr, key_len);
                                let value = JSValue::from_bits(bits.to_bits());
                                if !value.is_undefined() {
                                    return value;
                                }
                            }
                            let null_obj_ptr =
                                &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                            return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                        }
                    }
                    if let Some(dispatch) = handle_property_dispatch() {
                        unsafe {
                            let key_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let key_len = (*key).byte_len as usize;
                            let bits = dispatch(raw as i64, key_ptr, key_len);
                            return JSValue::from_bits(bits.to_bits());
                        }
                    }
                }
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() {
        return JSValue::undefined();
    }
    // Same handle-receiver path for already-stripped pointers — happens
    // when the codegen passes a raw i64 handle through the slow path.
    if crate::value::addr_class::is_handle_band(obj as usize) {
        if !key.is_null() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if is_timer_handle_method_key(key_bytes)
                    && crate::timer::is_known_timer_id(obj as i64)
                {
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = super::super::js_class_method_bind(this_f64, key_ptr, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
                if let Some(v) =
                    crate::text::text_handle_property(obj as usize, key_bytes, key_ptr, key_len)
                {
                    return v;
                }
            }
            if let Some(dispatch) = handle_property_dispatch() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    return JSValue::from_bits(bits.to_bits());
                }
            }
        }
        return JSValue::undefined();
    }
    if (obj as usize) < 0x10000 {
        return JSValue::undefined();
    }
    unsafe {
        if crate::closure::is_closure_ptr(obj as usize) {
            if key.is_null() {
                return JSValue::undefined();
            }
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if let Ok(name_str) = std::str::from_utf8(key_bytes) {
                if crate::closure::closure_is_key_deleted(obj as usize, name_str) {
                    return JSValue::undefined();
                }
                // ECMAScript "poison pill": reading `caller` / `arguments` off a
                // strict-mode function throws a TypeError (the %ThrowTypeError%
                // accessor on `Function.prototype`). Perry has no sloppy mode —
                // all TS/JS it compiles is strict — so this applies to every
                // function (declarations, expressions, methods, classes, arrows,
                // bound and built-in closures), matching `node`'s strict-mode
                // behavior. A `delete fn.caller` (handled above) still wins, and a
                // genuine own data prop of that name takes precedence so the rare
                // `Object.defineProperty(fn, "caller", …)` round-trips.
                if matches!(name_str, "caller" | "arguments")
                    && crate::closure::closure_get_dynamic_prop(obj as usize, name_str).to_bits()
                        == crate::value::TAG_UNDEFINED
                {
                    crate::fs::validate::throw_type_error_with_code(
                        "Restricted function property access",
                        "ERR_INVALID_ARG_TYPE",
                    );
                }
                let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                if val.to_bits() != crate::value::TAG_UNDEFINED {
                    return JSValue::from_bits(val.to_bits());
                }
                if name_str == "constructor" {
                    if let Some(ctor) =
                        crate::object::generator_function_constructor_of(obj as usize)
                    {
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    // Ordinary functions inherit `constructor` from
                    // `Function.prototype` → the global `Function`. (Generator /
                    // async-generator functions are handled just above with
                    // their own intrinsic constructors.)
                    let ctor =
                        super::super::js_get_global_this_builtin_value(b"Function".as_ptr(), 8);
                    if !JSValue::from_bits(ctor.to_bits()).is_undefined() {
                        return JSValue::from_bits(ctor.to_bits());
                    }
                }
                if name_str == "prototype" {
                    if let Some(proto) =
                        crate::object::generator_function_prototype_of(obj as usize)
                    {
                        return JSValue::from_bits(proto.to_bits());
                    }
                    let func_value = crate::value::js_nanbox_pointer(obj as i64);
                    if let Some(proto) =
                        super::super::ordinary_function_prototype_value_for_read(func_value)
                    {
                        return JSValue::from_bits(proto.to_bits());
                    }
                }
                if name_str == "length" {
                    let closure_value = crate::value::js_nanbox_pointer(obj as i64);
                    if let Some(arity) =
                        super::super::native_module::bound_native_callable_value_arity(
                            closure_value,
                        )
                    {
                        return JSValue::number(arity as f64);
                    }
                    if let Some(len) =
                        super::super::native_module::builtin_closure_length(obj as usize)
                    {
                        return JSValue::number(len as f64);
                    }
                    let length =
                        crate::closure::closure_length(obj as *const crate::closure::ClosureHeader);
                    return JSValue::number(length.unwrap_or(0) as f64);
                }
                if name_str == "name" {
                    let func_ptr =
                        (*(obj as *const crate::closure::ClosureHeader)).func_ptr as usize;
                    let fname =
                        crate::builtins::function_name_for_ptr(func_ptr).unwrap_or_default();
                    let s = crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
                    return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                }
            }
            return JSValue::undefined();
        }
        if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
            return JSValue::from_bits(val.to_bits());
        }
        // Buffers: BufferHeader is allocated via raw `alloc()` (no GcHeader)
        // and tracked in BUFFER_REGISTRY. Detect first so the GC header check
        // below doesn't read garbage one word before the BufferHeader.
        // Route `.length` to `js_buffer_length` (matches the codegen path that
        // routes through PropertyGet for chained `Buffer.from(...).length`
        // expressions where the static type isn't recognized as Buffer).
        if crate::buffer::is_registered_buffer(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if let Some(value) = crypto_key_property_value(obj as usize, key_bytes) {
                    return value;
                }
                if key_bytes == b"length" || key_bytes == b"byteLength" {
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
                // An own property on the Buffer shadows the same-named prototype
                // method; both reads live in `buffer_own_prop`.
                if let Some(v) = super::buffer_own_prop::buffer_own_prop_or_method(
                    obj, key_bytes, key_ptr, key_len,
                ) {
                    return v;
                }
                // ArrayBuffer.prototype `resizable` / `maxByteLength` getters.
                // Perry has no resizable ArrayBuffers, so `resizable` is always
                // false and `maxByteLength` equals `byteLength`. These live only
                // on ArrayBuffer (not DataView/SharedArrayBuffer/typed arrays),
                // which return `undefined` for them in Node — so scope to a
                // plain registered ArrayBuffer.
                if (key_bytes == b"resizable"
                    || key_bytes == b"maxByteLength"
                    || key_bytes == b"detached")
                    && crate::buffer::is_array_buffer(obj as usize)
                    && !crate::buffer::is_data_view(obj as usize)
                    && !crate::buffer::is_shared_array_buffer(obj as usize)
                {
                    if key_bytes == b"resizable" {
                        return JSValue::bool(false);
                    }
                    // `detached` (ES2024) — true after a successful
                    // `transfer`/`transferToFixedLength`/structuredClone
                    // transfer.
                    if key_bytes == b"detached" {
                        return JSValue::bool(crate::buffer::is_detached_buffer(obj as usize));
                    }
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
                if key_bytes == b"constructor" {
                    if crate::buffer::crypto_key_meta(obj as usize).is_some() {
                        let ctor = super::super::js_get_global_this_builtin_value(
                            b"CryptoKey".as_ptr(),
                            9,
                        );
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    // #3657: a DataView's `.constructor` is the global
                    // `DataView`, not `Buffer` — checked before the
                    // Uint8Array/Buffer arms since a DataView slice is also a
                    // registered buffer.
                    if crate::buffer::is_data_view(obj as usize) {
                        let ctor =
                            super::super::js_get_global_this_builtin_value(b"DataView".as_ptr(), 8);
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    // An ArrayBuffer / SharedArrayBuffer answers with ITS
                    // constructor (`ta.buffer.constructor === ArrayBuffer`,
                    // test262 ctors/buffer-arg/typedarray-backed-by-
                    // sharedarraybuffer).
                    if crate::buffer::is_shared_array_buffer(obj as usize) {
                        let ctor = super::super::js_get_global_this_builtin_value(
                            b"SharedArrayBuffer".as_ptr(),
                            17,
                        );
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    if crate::buffer::is_any_array_buffer(obj as usize) {
                        let ctor = super::super::js_get_global_this_builtin_value(
                            b"ArrayBuffer".as_ptr(),
                            11,
                        );
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    if crate::buffer::is_uint8array_buffer(obj as usize) {
                        let ctor = super::super::js_get_global_this_builtin_value(
                            b"Uint8Array".as_ptr(),
                            10,
                        );
                        return JSValue::from_bits(ctor.to_bits());
                    }
                    let module = b"buffer.Buffer";
                    return JSValue::from_bits(
                        js_create_native_module_namespace(module.as_ptr(), module.len()).to_bits(),
                    );
                }
                if crate::buffer::is_secret_key(obj as usize) {
                    if key_bytes == b"type" {
                        let s = crate::string::js_string_from_bytes(b"secret".as_ptr(), 6);
                        return JSValue::from_bits(JSValue::string_ptr(s).bits());
                    }
                    if key_bytes == b"symmetricKeySize" {
                        let b = obj as *const crate::buffer::BufferHeader;
                        return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                    }
                    if key_bytes == b"asymmetricKeyType" || key_bytes == b"asymmetricKeyDetails" {
                        return JSValue::undefined();
                    }
                }
                if key_bytes == b"buffer" || key_bytes == b"parent" {
                    let alias = crate::buffer::buffer_backing_array_buffer(obj as usize);
                    return JSValue::from_bits(
                        crate::value::js_nanbox_pointer(alias as i64).to_bits(),
                    );
                }
                if key_bytes == b"byteOffset" || key_bytes == b"offset" {
                    let offset = crate::buffer::buffer_byte_offset(obj as usize);
                    return JSValue::number(offset as f64);
                }
                // Issue #639 followup: method-as-value reads on a Buffer
                // (e.g. duck-type tests like `typeof v.readUInt8 === "function"`
                // in @perryts/mysql's `isBufferLike`) need to return a
                // bound-method closure so `typeof` reports `"function"` and
                // a subsequent call routes through `js_native_call_method`'s
                // existing `dispatch_buffer_method` arm. Pre-fix every
                // non-length read returned undefined, so duck tests failed
                // and the encoder fell through to its `String(buf)` fallback —
                // BLOB params got encoded as VAR_STRING and the INSERT
                // silently corrupted the binary column.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if is_buffer_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        // Buffers are stored as raw f64-bitcast pointers
                        // (NOT NaN-boxed) per CLAUDE.md "Module-level
                        // variables" — but `js_native_call_method`'s
                        // buffer arm at line ~5031 strips both raw and
                        // NaN-boxed payloads via `(bits >> 48) >= 0x7FF8`,
                        // so wrapping in POINTER_TAG here is equally
                        // valid and matches `js_class_method_bind`.
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Typed arrays (Int32Array/Float64Array/...): the `TypedArrayHeader` is
        // `std::alloc`'d (small) or GC-old-allocated (large), but in both cases
        // tracked in TYPED_ARRAY_REGISTRY, so detect via the side table before
        // the GC-header read below (which would read garbage for the small
        // `std::alloc` case). `.length`, `.byteLength`, `.byteOffset`, and
        // `.BYTES_PER_ELEMENT` lower as generic PropertyGet for multi-byte
        // numeric-length views whose static type the codegen doesn't recognize;
        // pre-fix, only Uint8Array worked (it's a registered buffer) so
        // multi-byte `.byteLength` returned undefined.
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let ta = obj as *const crate::typedarray::TypedArrayHeader;
                let elem_size = crate::typedarray::elem_size_for_kind(kind);
                if let Some(value) =
                    crate::typedarray_props::typed_array_get_own_property_value(ta, key)
                {
                    return JSValue::from_bits(value.to_bits());
                }
                match key_bytes {
                    b"length" => {
                        let len = crate::typedarray::js_typed_array_length(ta);
                        return JSValue::number(len as f64);
                    }
                    b"byteLength" => {
                        let len = crate::typedarray::js_typed_array_length(ta);
                        return JSValue::number((len as usize * elem_size) as f64);
                    }
                    b"buffer" => {
                        let buf = crate::typedarray_view::js_typed_array_backing_buffer(ta);
                        if buf.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(
                            crate::value::js_nanbox_pointer(buf as i64).to_bits(),
                        );
                    }
                    b"byteOffset" => {
                        return JSValue::number(crate::typedarray_view::js_typed_array_byte_offset(
                            ta,
                        ) as f64)
                    }
                    b"BYTES_PER_ELEMENT" => return JSValue::number(elem_size as f64),
                    // `ta.constructor` (no own override) resolves through the
                    // prototype chain to the intrinsic constructor for this
                    // element kind (e.g. `Uint8Array`). Mirrors the `Array` arm;
                    // needed so a default-`SpeciesCreate`d result reports
                    // `result.constructor === TA`.
                    b"constructor" => {
                        let name = crate::typedarray::name_for_kind(kind);
                        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
                        return JSValue::from_bits(v.to_bits());
                    }
                    _ => {}
                }
            }
            return JSValue::undefined();
        }
        // Sets: SetHeader is allocated via raw `alloc()` (no GcHeader),
        // so we can't safely read the byte preceding the pointer to
        // determine its type. Detect via the SET_REGISTRY first. Route
        // `.size` to `js_set_size` and synthesize method values for
        // prototype functions such as `.has`, which Node exposes through
        // ordinary property reads.
        if crate::set::is_registered_set(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let s = obj as *const crate::set::SetHeader;
                    return JSValue::number(crate::set::js_set_size(s) as f64);
                }
                if let Some(name) = set_method_value_name(key_bytes) {
                    // Return the SAME brand-checking thunk installed on
                    // Set.prototype so `const m = s.forEach; m.call(badThis)`
                    // throws a TypeError (and `m === Set.prototype.forEach`).
                    // Falls back to the legacy instance-bound closure if the
                    // prototype thunk isn't available.
                    if let Ok(method_name) = std::str::from_utf8(name) {
                        if let Some(v) =
                            super::super::collection_proto_thunks::collection_proto_method_value(
                                "Set",
                                method_name,
                            )
                        {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                    return JSValue::from_bits(result.to_bits());
                }
                // User expando keys (`s.tag = x`) live in the exotic side
                // table (`ExoticKind::Set`); see the Map/Set arm below.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    let receiver =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    if let Some(v) = crate::object::exotic_expando::exotic_get_own_property(
                        obj as usize,
                        crate::object::exotic_expando::ExoticKind::Set,
                        name,
                        receiver,
                    ) {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Symbols: registered in SYMBOL_POINTERS by symbol.rs. Symbols
        // allocated via Symbol.for(...) are Box-leaked (no GcHeader), so
        // reading the byte before would be UB. Detect via the side table.
        if crate::symbol::is_registered_symbol(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let sym_f64 =
                    f64::from_bits(0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF));
                if key_bytes == b"description" {
                    return JSValue::from_bits(
                        crate::symbol::js_symbol_description(sym_f64).to_bits(),
                    );
                }
            }
            return JSValue::undefined();
        }
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000
            || !is_valid_obj_ptr(obj as *const u8)
        {
            return JSValue::undefined();
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_ARRAY && !is_valid_obj_ptr(obj as *const u8) {
            return JSValue::undefined();
        }
        // Issue #618: closures have their own GC type (GC_TYPE_CLOSURE=4)
        // distinct from GC_TYPE_OBJECT, but support dynamic-property storage
        // via the `CLOSURE_DYNAMIC_PROPS` side-table. `js_object_set_field_by_name`
        // routes writes there for the IIFE-namespace pattern
        // (`((sql2) => { sql2.identifier = ...; })(sql)`); mirror the read
        // path here so the companion get fires. Pre-fix the
        // `gc_type != GC_TYPE_OBJECT` arm below would early-return undefined
        // for any closure receiver, masking the dynamic-prop side-table.
        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                // #3655: a `delete`d slot (`delete fn.name`, configurable:true)
                // reads back `undefined`, even though `name`/`length` are
                // otherwise synthesized from the registries below.
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    if crate::closure::closure_is_key_deleted(obj as usize, name_str) {
                        return JSValue::undefined();
                    }
                    // ECMAScript "poison pill" — see the matching arm in
                    // `js_object_get_field_by_name`. Reading `caller`/`arguments`
                    // off any strict-mode function throws a TypeError; Perry has
                    // no sloppy mode, so this covers every function. A genuine own
                    // data prop of that name still wins.
                    if matches!(name_str, "caller" | "arguments")
                        && crate::closure::closure_get_dynamic_prop(obj as usize, name_str)
                            .to_bits()
                            == crate::value::TAG_UNDEFINED
                    {
                        crate::fs::validate::throw_type_error_with_code(
                            "Restricted function property access",
                            "ERR_INVALID_ARG_TYPE",
                        );
                    }
                }
                // `fn.length` — return the registered ECMAScript-visible
                // length for the underlying function. Ramda's
                // `converge` / `useWith` / `addIndex` chain feeds
                // `pluck('length', fns)` through
                // `reduce(max, 0, …)` → `curryN(N, …)` → `_arity(N, …)`;
                // without a real number here that pipeline produces
                // `NaN`, and `_arity` throws
                // `First argument to _arity must be a non-negative
                // integer no greater than ten` at module init.
                if name_bytes == b"length" {
                    let closure_value = crate::value::js_nanbox_pointer(obj as i64);
                    if let Some(arity) =
                        super::super::native_module::bound_native_callable_value_arity(
                            closure_value,
                        )
                    {
                        return JSValue::number(arity as f64);
                    }
                    // #3143: built-in proto methods share one func_ptr, so the
                    // func-ptr arity registry can't tell `map` (1) from `slice`
                    // (2) — read the per-closure recorded spec length first.
                    if let Some(len) =
                        super::super::native_module::builtin_closure_length(obj as usize)
                    {
                        return JSValue::number(len as f64);
                    }
                    let length =
                        crate::closure::closure_length(obj as *const crate::closure::ClosureHeader);
                    return JSValue::number(length.unwrap_or(0) as f64);
                }
                // #2145: `fn.__proto__` is the closure's [[Prototype]]
                // — `Int8Array.__proto__ === %TypedArray%` after
                // `populate_global_this_builtins` wired the static-proto
                // side-table. Spec models `__proto__` as a
                // `Object.prototype` accessor that returns
                // `[[GetPrototypeOf]](this)`; for closures Perry resolves
                // that off the same side-table `Object.setPrototypeOf`
                // writes to. Walking `closure_get_dynamic_prop` would
                // instead look for a `__proto__` own-prop on the parent,
                // which is the wrong thing — the proto IS the answer.
                // Returns undefined (not null) when no proto is recorded,
                // matching the closure-receiver `getPrototypeOf` arm
                // semantics for non-wired closures.
                if name_bytes == b"__proto__" {
                    if let Some(proto_bits) = crate::closure::closure_static_prototype(obj as usize)
                    {
                        return JSValue::from_bits(proto_bits);
                    }
                    return JSValue::undefined();
                }
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    // User-attached own property (`fn.x = 1`) takes precedence.
                    let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                    if val.to_bits() != crate::value::TAG_UNDEFINED {
                        return JSValue::from_bits(val.to_bits());
                    }
                    // #3664: `g.constructor` for a generator/async-generator
                    // function resolves through its [[Prototype]] (`%Generator%`)
                    // to `%GeneratorFunction%` / `%AsyncGeneratorFunction%`.
                    // Other functions have no `constructor` own-prop in Perry's
                    // model (they fall through to `undefined`, as before).
                    if name_str == "constructor" {
                        if let Some(ctor) =
                            crate::object::generator_function_constructor_of(obj as usize)
                        {
                            return JSValue::from_bits(ctor.to_bits());
                        }
                    }
                    // #3664: `g.prototype` for a generator/async-generator
                    // function is a lazily-created object whose [[Prototype]] is
                    // `%Generator.prototype%`. Non-generator functions fall
                    // through (unchanged). The dynamic-prop check above already
                    // returned any cached/user-assigned `prototype`.
                    if name_str == "prototype" {
                        if let Some(proto) =
                            crate::object::generator_function_prototype_of(obj as usize)
                        {
                            return JSValue::from_bits(proto.to_bits());
                        }
                        let func_value = crate::value::js_nanbox_pointer(obj as i64);
                        if let Some(proto) =
                            super::super::ordinary_function_prototype_value_for_read(func_value)
                        {
                            return JSValue::from_bits(proto.to_bits());
                        }
                    }
                    // #2059: `fn.name` — every function carries a built-in own
                    // `name` data property. Resolve the codegen-registered name
                    // (keyed by the wrapper func_ptr, the same registry the
                    // `[Function: <name>]` formatter uses); anonymous functions
                    // read back `""`, matching Node, not `undefined`.
                    if name_str == "name" {
                        let func_ptr =
                            (*(obj as *const crate::closure::ClosureHeader)).func_ptr as usize;
                        let fname =
                            crate::builtins::function_name_for_ptr(func_ptr).unwrap_or_default();
                        let s =
                            crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    // #3716: reading `f.bind` / `f.call` / `f.apply` *as a value*
                    // off any function must yield a real callable, not
                    // `undefined`. Reify it into a BOUND_METHOD closure bound to
                    // this function as receiver; invoking it routes back through
                    // `js_native_call_method(f, "<method>", …)`. This is what makes
                    // the "uncurry-this" idiom
                    // `Function.prototype.call.bind(method)` work — reading `.bind`
                    // off the reified `Function.prototype.call` previously read
                    // back `undefined`, so the bound function was never produced.
                    if let Some(method) = reified_function_method_name(name_str) {
                        let receiver =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        return JSValue::from_bits(
                            crate::closure::reify_function_method_value(receiver, method).to_bits(),
                        );
                    }
                    return JSValue::from_bits(val.to_bits());
                }
            }
            return JSValue::undefined();
        }
        // Error objects: route the common instance properties (message,
        // name, stack, cause) through the dedicated error accessors.
        // `js_object_get_field_by_name_f64` is the codegen's default
        // property dispatch for caught exceptions, so this is the only
        // sensible place to wire Error access.
        if gc_type == crate::gc::GC_TYPE_ERROR {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let err_ptr = obj as *mut crate::error::ErrorHeader;
                // User-assigned own properties (`err.code = "X"`,
                // `err.errno = -2`, custom fields) take precedence over the
                // built-in accessors below — they were recorded in the
                // per-error side table by the setter (#2014). Routed through
                // the exotic helper so `Object.defineProperty(err, k, {get})`
                // accessors fire too.
                if let Ok(key_str) = std::str::from_utf8(key_bytes) {
                    let receiver =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    if let Some(v) = super::super::exotic_expando::exotic_get_own_property(
                        err_ptr as usize,
                        super::super::exotic_expando::ExoticKind::Error,
                        key_str,
                        receiver,
                    ) {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
                match key_bytes {
                    b"message" => {
                        let s = crate::error::js_error_get_message(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"name" => {
                        let s = crate::error::js_error_get_name(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"stack" => {
                        let s = crate::error::js_error_get_stack(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"cause" => {
                        let v = crate::error::js_error_get_cause(err_ptr);
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"toString" => {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, b"toString".as_ptr(), 8);
                        return JSValue::from_bits(result.to_bits());
                    }
                    b"constructor" => {
                        let name = crate::error::error_kind_constructor_name((*err_ptr).error_kind);
                        let name = name.as_bytes();
                        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"code" => {
                        // Errors thrown by runtime validation paths (e.g.
                        // diagnostics_channel argument checks) register
                        // their `ERR_*` code in a side table keyed on the
                        // message StringHeader pointer. This avoids the
                        // earlier substring-match shim that incorrectly
                        // applied `ERR_INVALID_ARG_TYPE` to any user
                        // TypeError whose `.message` happened to equal
                        // the placeholder text.
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(code) = crate::node_submodules::error_code_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                code.as_ptr(),
                                code.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"errors" => {
                        // AggregateError.errors — return the errors array
                        // NaN-boxed with POINTER_TAG so callers can index
                        // into it. (The LLVM backend also has a direct
                        // `js_error_get_errors` fast path in expr.rs but
                        // this covers dynamic dispatch on caught errors.)
                        let errs = crate::error::js_error_get_errors(err_ptr);
                        if errs.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(crate::js_nanbox_pointer(errs as i64).to_bits());
                    }
                    b"syscall" => {
                        // Node attaches `syscall` to system-call errors
                        // (open/stat/access/…). Perry's fs helpers register
                        // the value in a side table keyed by the message
                        // StringHeader (parallel to the `.code` path).
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(syscall) =
                            crate::node_submodules::error_syscall_for_message(msg)
                        {
                            let s = crate::string::js_string_from_bytes(
                                syscall.as_ptr(),
                                syscall.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"errno" => {
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(errno) = crate::node_submodules::error_errno_for_message(msg) {
                            return JSValue::number(errno as f64);
                        }
                        return JSValue::undefined();
                    }
                    b"path" => {
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(path) = crate::node_submodules::error_path_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                path.as_ptr(),
                                path.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"hostname" => {
                        // Node attaches `hostname` to c-ares dns errors
                        // (`dns.resolve*`/`dns.reverse`). Mirrors `.path`.
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(hostname) =
                            crate::node_submodules::error_hostname_for_message(msg)
                        {
                            let s = crate::string::js_string_from_bytes(
                                hostname.as_ptr(),
                                hostname.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    b"dest" => {
                        // Node attaches `dest` to two-path fs errors
                        // (rename/copyFile/link/symlink). Mirrors `.path`.
                        let msg = crate::error::js_error_get_message(err_ptr);
                        if let Some(dest) = crate::node_submodules::error_dest_for_message(msg) {
                            let s = crate::string::js_string_from_bytes(
                                dest.as_ptr(),
                                dest.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                        return JSValue::undefined();
                    }
                    _ => {
                        // Inherited members: user-defined props/accessors on
                        // `Error.prototype` (or the kind-specific prototype)
                        // resolve through the prototype object — e.g.
                        // `Object.defineProperty(Error.prototype, "prop",
                        // {value}); new Error().prop`.
                        let kind_name =
                            crate::error::error_kind_constructor_name((*err_ptr).error_kind);
                        for proto_name in [kind_name, "Error"] {
                            let proto = crate::object::builtin_prototype_value(proto_name);
                            let pv = JSValue::from_bits(proto.to_bits());
                            if pv.is_pointer() {
                                let proto_ptr = pv.as_pointer::<ObjectHeader>();
                                if !proto_ptr.is_null() {
                                    let v = js_object_get_field_by_name(proto_ptr, key);
                                    if !v.is_undefined() {
                                        return JSValue::from_bits(v.bits());
                                    }
                                }
                            }
                            if proto_name == "Error" {
                                break;
                            }
                        }
                        return JSValue::undefined();
                    }
                }
            }
            return JSValue::undefined();
        }
        // Arrays: handle `.length` so dynamic property access on a
        // typed-Any local returned from `JSON.parse("[1,2,3]")` picks
        // up the real length instead of falling through to object
        // field lookup and returning undefined. The array-length
        // inline fast path in codegen fires only when the type is
        // statically known, so this branch catches the dynamic case.
        if gc_type == crate::gc::GC_TYPE_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let arr = obj as *const crate::array::ArrayHeader;
                if key_bytes == b"length" {
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                // date-fns / drizzle / lodash duck-typing path:
                // `arr.constructor === Array`, `new arr.constructor(...)`,
                // etc. expect a non-undefined function-typed value that
                // refers back to the global `Array` constructor. Resolve
                // through the singleton so this returns the same closure
                // pointer as the bare `Array` identifier.
                if key_bytes == b"constructor" {
                    // An own `constructor` expando (`arr.constructor = Foo`)
                    // shadows the intrinsic — observable via ArraySpeciesCreate
                    // (map/filter/slice/splice/concat) and reflection. Only fall
                    // back to the global `Array` when there is no own write.
                    if let Some(v) = own_data_field_by_name(obj, key) {
                        return v;
                    }
                    if let Some(v) = crate::array::array_named_property_get(arr, key) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(index) = super::super::canonical_array_index(name) {
                        if crate::state::state().descriptors.accessors_in_use.get() {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                                if acc.get != 0 {
                                    let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                    return invoke_accessor_getter(acc.get, receiver);
                                }
                                return JSValue::undefined();
                            }
                        }
                        if super::super::has_own_helpers::array_own_key_present(arr, key) {
                            return JSValue::from_bits(
                                crate::array::js_array_get_f64(arr, index).to_bits(),
                            );
                        }
                        if let Some(v) = array_prototype_property_value(name, obj as usize) {
                            return v;
                        }
                        return JSValue::undefined();
                    }
                    // Named (non-index) accessor installed via
                    // `Object.defineProperty(arr, "prop", {get,set})`.
                    if crate::state::state().descriptors.accessors_in_use.get() {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            return JSValue::undefined();
                        }
                    }
                    if let Some(v) = own_data_field_by_name(obj, key) {
                        return v;
                    }
                    if let Some(v) = crate::array::array_named_property_get(arr, key) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    if let Some(v) = array_prototype_property_value(name, obj as usize) {
                        return v;
                    }
                }
                if is_array_method_value_name(key_bytes) {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(v) = array_prototype_property_value(name, obj as usize) {
                            return v;
                        }
                    }
                }
            }
            return JSValue::undefined();
        }
        // Issue #179 Phase 2: lazy array dispatch. `.length` returns
        // cached_length without materializing; any other property
        // access force-materializes (via the call into the generic
        // array path, which goes through `clean_arr_ptr` and hits
        // the lazy branch there).
        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
            }
            // Any other property access force-materializes, then
            // re-enters via the materialized ArrayHeader pointer.
            let materialized = crate::json_tape::force_materialize_lazy(
                obj as *mut crate::json_tape::LazyArrayHeader,
            );
            return js_object_get_field_by_name(materialized as *const ObjectHeader, key);
        }
        // Strings: handle `.length` so `(x as string).length` on an
        // unknown-typed local (TypeScript `as` casts are erased in
        // HIR) produces the real UTF-16 code-unit length.
        if gc_type == crate::gc::GC_TYPE_STRING {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let s = obj as *const crate::StringHeader;
                    return JSValue::number((*s).utf16_len as f64);
                }
                // A primitive string inherits `.constructor` from String.prototype:
                // `"x".constructor === String` (test262 language/types/string/
                // S8.4_A9/A12). Resolve to the same global `String` value bare-
                // `String` yields so identity holds — mirrors the Array branch above.
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"String".as_ptr(), 6);
                    return JSValue::from_bits(v.to_bits());
                }
                if let Some((kind, asym_type)) = crate::buffer::asymmetric_key_meta(obj as usize) {
                    if key_bytes == b"type" {
                        let label = if kind == 1 {
                            b"public".as_slice()
                        } else {
                            b"private".as_slice()
                        };
                        let s =
                            crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
                        return JSValue::from_bits(JSValue::string_ptr(s).bits());
                    }
                    if key_bytes == b"asymmetricKeyType" {
                        let label = match asym_type {
                            1 => b"rsa".as_slice(),
                            2 => b"ec".as_slice(),
                            3 => b"ed25519".as_slice(),
                            4 => b"x25519".as_slice(),
                            _ => b"".as_slice(),
                        };
                        if !label.is_empty() {
                            let s = crate::string::js_string_from_bytes(
                                label.as_ptr(),
                                label.len() as u32,
                            );
                            return JSValue::from_bits(JSValue::string_ptr(s).bits());
                        }
                    }
                    if key_bytes == b"asymmetricKeyDetails" {
                        let details = js_object_alloc(0, if asym_type == 2 { 1 } else { 0 });
                        if asym_type == 2 {
                            let name =
                                crate::string::js_string_from_bytes(b"namedCurve".as_ptr(), 10);
                            let val =
                                crate::string::js_string_from_bytes(b"prime256v1".as_ptr(), 10);
                            js_object_set_field_by_name(
                                details,
                                name,
                                f64::from_bits(JSValue::string_ptr(val).bits()),
                            );
                        }
                        return JSValue::from_bits(JSValue::pointer(details as *mut u8).bits());
                    }
                    // `js_class_method_bind` only needs a pointer that stays
                    // valid for the closure's lifetime — the static byte
                    // literals satisfy that without per-read allocation.
                    let static_name: Option<&'static [u8]> = match key_bytes {
                        b"export" => Some(b"export"),
                        b"equals" => Some(b"equals"),
                        b"toCryptoKey" => Some(b"toCryptoKey"),
                        _ => None,
                    };
                    if let Some(name) = static_name {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Maps/Sets: `.size`, expando keys, and prototype member values —
        // see `map_set_receiver.rs` (extracted for the file-size gate).
        if gc_type == crate::gc::GC_TYPE_MAP || gc_type == crate::gc::GC_TYPE_SET {
            return super::map_set_receiver::map_set_instance_property(
                obj,
                key,
                gc_type == crate::gc::GC_TYPE_MAP,
            );
        }
        // RegExp: RegExpHeader is allocated via GC_TYPE_OBJECT but tracked
        // in REGEX_POINTERS. Detect and route `.source`, `.flags`,
        // `.lastIndex`, `.global`, `.ignoreCase`, `.multiline`, `.sticky`,
        // `.unicode`, `.dotAll` to the regex header fields. Must run
        // before the generic object-field path so the keys_array lookup
        // doesn't try to read the regex header bytes as ObjectHeader.
        if gc_type == crate::gc::GC_TYPE_OBJECT && crate::regex::is_regex_pointer(obj as *const u8)
        {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let re = obj as *const crate::regex::RegExpHeader;
                // User expando / defineProperty'd own properties shadow the
                // prototype fallthrough but NOT the spec header props above
                // (source/flags/lastIndex/... are non-configurable).
                if !matches!(
                    key_bytes,
                    b"source"
                        | b"flags"
                        | b"lastIndex"
                        | b"global"
                        | b"ignoreCase"
                        | b"multiline"
                        | b"sticky"
                        | b"unicode"
                        | b"dotAll"
                        | b"hasIndices"
                ) {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        let receiver =
                            f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
                        if let Some(v) = super::super::exotic_expando::exotic_get_own_property(
                            obj as usize,
                            super::super::exotic_expando::ExoticKind::RegExp,
                            name,
                            receiver,
                        ) {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                }
                match key_bytes {
                    b"source" => {
                        let s = crate::regex::js_regexp_get_source(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"flags" => {
                        let s = crate::regex::js_regexp_get_flags(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"lastIndex" => {
                        // lastIndex stores the raw NaN-boxed value (usually a
                        // number, but any value is assignable).
                        return JSValue::from_bits((*re).last_index);
                    }
                    b"global" => {
                        return JSValue::bool((*re).global);
                    }
                    b"ignoreCase" => {
                        return JSValue::bool((*re).case_insensitive);
                    }
                    b"multiline" => {
                        return JSValue::bool((*re).multiline);
                    }
                    // #2828: route the remaining observable flags to the
                    // header fields populated by `js_regexp_new` instead of
                    // unconditionally returning `false`.
                    b"sticky" => {
                        return JSValue::bool((*re).sticky);
                    }
                    b"unicode" => {
                        return JSValue::bool((*re).unicode);
                    }
                    b"dotAll" => {
                        return JSValue::bool((*re).dot_all);
                    }
                    b"hasIndices" => {
                        return JSValue::bool((*re).has_indices);
                    }
                    // Inherited `RegExp.prototype` members read off an instance
                    // (`re.constructor`, `re.exec`, `re.toString`, a user-added
                    // `RegExp.prototype.x`) resolve through the prototype chain.
                    // The RegExpHeader isn't a plain object, so walk to
                    // %RegExp.prototype% and return its own data field — this is
                    // what makes `re.constructor === RegExp` and reflective
                    // method reads work. `source`/`flags`/the flag accessors are
                    // handled by the arms above and never reach here, so we never
                    // return an un-invoked getter closure.
                    _ => {
                        let proto = crate::object::builtin_prototype_value("RegExp");
                        let proto_ptr =
                            crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader;
                        if !proto_ptr.is_null() {
                            if let Some(v) = own_data_field_by_name(proto_ptr, key) {
                                return v;
                            }
                        }
                        return JSValue::undefined();
                    }
                }
            }
            return JSValue::undefined();
        }
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return JSValue::undefined();
            }
        }
        if super::super::is_arguments_object(obj) {
            if let Some(value) = super::super::arguments_object_get_field(obj, key) {
                return value;
            }
        }

        // #1387: `PerformanceEntry#toJSON` is a synthesized (non-enumerable)
        // method — entry objects are plain shaped objects with no stored
        // `toJSON` field, so a `entry.toJSON` read (e.g. `typeof entry.toJSON`)
        // would otherwise miss the keys_array and return undefined. Return a
        // bound-method closure; the call lands in `js_native_call_method`'s
        // toJSON arm via `dispatch_bound_method`. Gated on the key bytes first
        // so non-toJSON reads pay only a length+compare, not the identity
        // check.
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"toJSON" && crate::perf_hooks::is_perf_entry_object(obj) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                let result = js_class_method_bind(this_f64, b"toJSON".as_ptr(), 6);
                return JSValue::from_bits(result.to_bits());
            }
        }

        // #2856: a property READ (not a call) of `next` on a Map/Set
        // iterator object must yield a callable (so `typeof it.next ===
        // "function"` and `const n = it.next; n()` work). The iterators
        // dispatch via class id and store no `next` field, so bind the
        // method to the receiver. Also bind the self-iterator methods.
        if !key.is_null()
            && ((*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                || (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID)
        {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            let bind_name: Option<&'static [u8]> = match key_bytes {
                b"next" => Some(b"next"),
                b"return" => Some(b"return"),
                b"throw" => Some(b"throw"),
                b"@@iterator" => Some(b"@@iterator"),
                _ => None,
            };
            if let Some(name) = bind_name {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
                return JSValue::from_bits(result.to_bits());
            }
            return JSValue::undefined();
        }

        // Issue #649: native-module sub-namespace property access.
        // `fs.constants.F_OK` lowers to `PropertyGet { PropertyGet { fs,
        // "constants" }, "F_OK" }` — the inner expression's runtime value
        // is a NATIVE_MODULE_CLASS_ID-tagged ObjectHeader produced by
        // `js_create_native_module_namespace`; the outer PropertyGet then
        // arrives here with the sub-namespace as receiver. Pre-fix the
        // lookup fell through to the field-bag scan (which only stores
        // `__module__`) and returned undefined. Now we route through
        // `get_native_module_constant` directly.
        // Issue #649 / #3687 / #894: native-module own-field reads
        // (sub-namespaces, process IPC props, callable exports). Body
        // relocated to native_module.rs::vt_get_own_field so the
        // (module, method) tables are reachable only through the vtable.
        // `None` (no module name / vtable uninstalled) falls through to
        // the generic scans below, matching the pre-relocation flow.
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            if let Some(vt) = super::super::native_module::native_module_vtable() {
                if let Some(v) = (vt.get_own_field)(obj, key) {
                    return v;
                }
            }
        }

        if (*obj).class_id == crate::tty::CLASS_ID_TTY_WRITE_STREAM && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let property_name =
                std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
            if let Some(value) = crate::tty::tty_write_stream_dimension(property_name) {
                return JSValue::from_bits(value.to_bits());
            }
        }

        // AbortSignal method read through a DYNAMICALLY-typed receiver
        // (`const s: any = c.signal; s.addEventListener` / `typeof
        // s.addEventListener`). The static receiver form lowers to the native
        // call, but this generic walk found no method property and returned
        // undefined (the #5964 URLSearchParams dynamic-dispatch class).
        // Returns a bound-method closure for the known signal methods.
        if (*obj).class_id == crate::url::abort::ABORT_SIGNAL_CLASS_ID && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if let Some(bound) =
                crate::url::abort::abort_signal_method_bind(obj as *mut ObjectHeader, key_bytes)
            {
                return JSValue::from_bits(bound.to_bits());
            }
        }

        // Refs #420 / #618 followup: `instance.constructor` returns the
        // class ref. Pre-fix this fell through to the keys_array lookup
        // which never finds "constructor" (the class itself isn't stored
        // as a field on the instance), and the chain returned undefined.
        // Drizzle's `is(value, type)` walks `value.constructor[entityKind]`
        // which depends on this. Spec: every instance's `__proto__.constructor`
        // points back to the class function. We materialize that lookup
        // by reading the ObjectHeader's class_id and returning the
        // INT32-tagged class ref if registered. Unregistered class_id
        // (e.g. `class C {}` with no methods) still returns undefined
        // here; pure object literals have class_id=0 and also return
        // undefined (matches Node behavior — bare object literals don't
        // get a custom constructor; their .constructor would be Object).
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            // #4949 `.prototype` / #6497 `.name` on heap class-expression
            // values — see `class_object_props`.
            if (*obj).object_type == crate::error::OBJECT_TYPE_CLASS && (*obj).class_id != 0 {
                if key_bytes == b"prototype" {
                    return super::class_object_props::class_object_prototype_value(obj);
                }
                if key_bytes == b"name" {
                    if let Some(v) = super::class_object_props::class_object_name_value(obj, key) {
                        return v;
                    }
                }
            }
            if (*obj).class_id == CLASS_ID_BOXED_STRING {
                if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits()),
                ) {
                    if let Some(value) = string_index_value(payload, key) {
                        return value;
                    }
                }
            }
            if key_bytes == b"constructor" {
                if let Some(v) = super::class_object_props::instance_constructor_value(obj, key) {
                    return v;
                }
            }
        }

        let keys = (*obj).keys_array;

        if keys.is_null() {
            // #809: an object with no own keys (e.g. an `Object.create(proto)`
            // result, or a `Function.prototype = obj` instance) still has to
            // resolve inherited props/methods. Pre-fix this returned undefined
            // here — BEFORE the `class_id` prototype-walk below — so
            // `Object.create(P).m()` threw `TypeError: m is not a function`.
            let class_id = (*obj).class_id;
            if class_id != 0 {
                let receiver =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                if let Some(v) =
                    super::super::class_registry::resolve_proto_chain_field_with_receiver(
                        class_id, key, receiver,
                    )
                {
                    return v;
                }
                let key_bytes = std::slice::from_raw_parts(
                    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                    (*key).byte_len as usize,
                );
                // Issue #838 followup (b): same keyless-receiver gap for
                // JS-classic prototype methods. An instance allocated via
                // `js_new_function_construct` (no constructor-body write
                // yet, or a constructor that runs the closures' own
                // capture writes but never `this.<own field> = …`)
                // starts with `keys_array == null`. Without this arm
                // dayjs's `(new _(cfg)).format` returned undefined
                // because the keyless branch skipped the regular
                // `CLASS_PROTOTYPE_METHODS` walk reached further down
                // — see the matching arm at line ~4083.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(v) = lookup_prototype_method(class_id, name) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // Native class vtable accessors and methods are exposed
                    // from the class, not from own fields, so keyless
                    // receivers need the same fallback as shaped receivers.
                    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                        if let Some(ref reg) = *registry {
                            let mut cid = class_id;
                            let mut depth = 0usize;
                            while depth < 32 {
                                if let Some(vtable) = reg.get(&cid) {
                                    if let Some(&getter_ptr) = vtable.getters.get(name) {
                                        let this_f64 = class_getter_this(obj);
                                        let f: extern "C" fn(f64) -> f64 =
                                            std::mem::transmute(getter_ptr);
                                        return JSValue::from_bits(f(this_f64).to_bits());
                                    }
                                }
                                match get_parent_class_id(cid) {
                                    Some(p) if p != 0 && p != cid => {
                                        cid = p;
                                        depth += 1;
                                    }
                                    _ => break,
                                }
                            }
                        }
                    }
                    if lookup_class_method_in_chain(class_id, name).is_some() {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            if class_id == crate::builtins::CONSOLE_INSTANCE_CLASS_ID {
                let key_bytes = std::slice::from_raw_parts(
                    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                    (*key).byte_len as usize,
                );
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if crate::builtins::is_console_instance_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            // #2820: a keyless object (`{}`, `Object.create(...)`) may still
            // carry an explicit `Object.setPrototypeOf` prototype — walk it so
            // inherited reads resolve.
            if !key.is_null() {
                if let Some(v) =
                    super::super::prototype_chain::resolve_inherited_field(obj as usize, key)
                {
                    return v;
                }
                if let Some(v) = ordinary_object_prototype_property_value(obj, key) {
                    return v;
                }
            }
            return JSValue::undefined();
        }

        // Validate keys_array is a real heap pointer (upper 16 bits must be 0 for ARM64/x86-64 user space).
        // If the object is actually a non-Object type (closure, array, map, etc.), keys_array at offset
        // 16 may contain garbage. An invalid upper 16-bit value catches this case defensively.
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            // #2820: an object with no own keys (`{}`) may still have an
            // explicit `Object.setPrototypeOf` prototype — walk it before
            // giving up so inherited reads resolve.
            if !key.is_null() {
                if let Some(v) =
                    super::super::prototype_chain::resolve_inherited_field(obj as usize, key)
                {
                    return v;
                }
                if let Some(v) = ordinary_object_prototype_property_value(obj, key) {
                    return v;
                }
            }
            return JSValue::undefined();
        }

        // Issue #62 phase B: the previous "ASCII-like pointer value" heuristic
        // assumed macOS mmap always returns arena pointers with `top_byte < 0x20`.
        // That stopped holding once strings started arena-allocating (more blocks,
        // mimalloc mapping into higher ranges): valid 0x000_04355_a033_* pointers
        // triggered false positives, the heuristic returned `undefined`, and tests
        // like `Object.defineProperty` flapped. The GcHeader `obj_type ==
        // GC_TYPE_ARRAY` check immediately below is a real content-level validation
        // (can't be faked by an address in any range) and fully supersedes this
        // address-sniffing heuristic.

        // Cross-platform safety: validate keys_array has a valid GcHeader.
        // If the keys_array pointer is corrupt (e.g., due to a stale reference after GC,
        // or a func_addr relocation issue on x86_64), the GcHeader check catches it
        // before we dereference the array contents.
        {
            let keys_gc =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_gc_type = (*keys_gc).obj_type;
            // keys_array must be GC_TYPE_ARRAY (arena-allocated array)
            if keys_gc_type != crate::gc::GC_TYPE_ARRAY {
                return JSValue::undefined();
            }
        }

        // Fast path: check field index cache (keys_array_ptr + key_hash → field_index)
        // Objects with the same shape share the same keys_array, so we cache per-shape lookups.
        let key_bytes = std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        );
        // #4140: builtin reflection-only accessors (e.g. the
        // `%TypedArray%.prototype` getters) don't flip `ACCESSORS_IN_USE`, so the
        // gated short-circuits below skip them on a plain value read. Handle the
        // hosting prototype object here — a cheap pointer compare for everything
        // else — before the slot scan returns the empty backing field.
        if let Some(v) = builtin_reflection_accessor_read(obj, key_bytes) {
            return v;
        }
        let key_hash = {
            let mut h: u32 = 0x811c9dc5;
            for &b in key_bytes {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h
        };
        let keys_id = keys as usize;

        // Clamp the keys length to capacity so a bogus/oversized length can't
        // drive the wide-key map build or the linear scan below into unbounded
        // work (see `keys_array_len_capped_to_capacity`). No-op for well-formed
        // arrays.
        let key_count = crate::array::keys_array_len_capped_to_capacity(keys);

        // Per-thread inline cache (`state().field_lookup.field_cache`):
        // fixed-size direct-mapped cache (no allocation, no HashMap).
        // Each entry stores (keys_ptr, key_hash, field_index). Copied-minor
        // nursery reset can reuse a keys-array address, so cache hits still
        // validate the key slot before returning a field.
        let st = crate::state::state();
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % super::FIELD_CACHE_SIZE;
        let cached = {
            let cache = &*st.field_lookup.field_cache.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash {
                Some(entry.2)
            } else {
                None
            }
        };
        if let Some(field_idx) = cached {
            let idx = field_idx as usize;
            let cache_hit_valid = if idx < key_count {
                let key_val = crate::array::js_array_get(keys, field_idx);
                // #1781: SSO-aware match — pre-fix the `is_string()` here
                // false-invalidated cache hits for ≤5-byte keys stored
                // as SHORT_STRING_TAG values.
                crate::string::js_string_key_matches(key_val, key)
            } else {
                false
            };
            if !cache_hit_valid {
                let cache = &mut *st.field_lookup.field_cache.get();
                cache[cache_idx] = (0, 0, 0);
            } else {
                // Accessor short-circuit: if this (obj, key) has a getter installed,
                // invoke it instead of reading the slot. The `ACCESSORS_IN_USE`
                // thread-local gate keeps this off the hot path in the common case;
                // the per-object flag gate avoids invoking a stale getter left by a
                // freed object whose address this fresh object reused.
                if st.descriptors.accessors_in_use.get()
                    && super::super::object_has_descriptors(obj as usize)
                {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            // Has accessor but no getter → undefined.
                            return JSValue::undefined();
                        }
                    }
                }
                return js_object_get_field(obj, field_idx);
            }
        }

        // Slow path: linear scan through keys array
        let _field_count = (*obj).field_count as usize;

        let alloc_limit =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;

        // #5054: wide objects get a validated key→index map so per-key reads
        // stay O(1) instead of O(key_count). A `None` falls through to the
        // linear scan below (the index is an accelerator, not authoritative).
        if key_count >= WIDE_KEY_INDEX_MIN_KEYS {
            if let Some(i) = wide_key_index_lookup(keys_id, key_bytes, key, keys, key_count) {
                if st.descriptors.accessors_in_use.get()
                    && super::super::object_has_descriptors(obj as usize)
                {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            return JSValue::undefined();
                        }
                    }
                }
                return if (i as usize) < alloc_limit {
                    js_object_get_field(obj, i)
                } else {
                    match overflow_get(obj as usize, i as usize) {
                        Some(bits) => JSValue::from_bits(bits),
                        None => JSValue::undefined(),
                    }
                };
            }
        }

        if key_count > 65536 {
            return JSValue::undefined();
        }

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: accept inline SSO short keys here too — the
            // slow-path lookup is what backs `obj[k]` for ≤5-byte
            // keys after a field-cache miss.
            if crate::string::js_string_key_matches(key_val, key) {
                // Cache this lookup for next time
                {
                    let cache = &mut *st.field_lookup.field_cache.get();
                    cache[cache_idx] = (keys_id, key_hash, i as u32);
                }
                if key_count >= WIDE_KEY_INDEX_MIN_KEYS {
                    wide_key_index_note_hit(keys_id, key_bytes, i as u32);
                }
                // Accessor short-circuit (see fast path above).
                if st.descriptors.accessors_in_use.get()
                    && super::super::object_has_descriptors(obj as usize)
                {
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                            if acc.get != 0 {
                                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                                return invoke_accessor_getter(acc.get, receiver);
                            }
                            return JSValue::undefined();
                        }
                    }
                }
                if i < alloc_limit {
                    return js_object_get_field(obj, i as u32);
                } else {
                    return match overflow_get(obj as usize, i) {
                        Some(bits) => JSValue::from_bits(bits),
                        None => JSValue::undefined(),
                    };
                }
            }
        }

        // Key not found in the keys_array — fall back to the class
        // vtable's getter map. Refs #486 (hono): cross-module class
        // getters (e.g. hono Context's `get req()` defined in
        // `hono/dist/context.js` and read from a user `c.req.url`
        // expression in main.ts) reach this point because the field
        // dispatcher only looks for stored fields, not getter accessors.
        // The getter is registered in `CLASS_VTABLE_REGISTRY` via
        // `js_register_class_getter` at module init by codegen — invoke
        // it with the same NaN-boxed `this` the codegen passes for
        // method dispatch.
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    // Walk the class -> parent chain so a getter declared
                    // on a base class is also found when the receiver is
                    // a subclass instance. `get_parent_class_id` reads
                    // CLASS_REGISTRY (populated by `js_register_class_parent`).
                    let mut cid = class_id;
                    let mut depth = 0usize;
                    while depth < 32 {
                        if let Some(vtable) = reg.get(&cid) {
                            if let Ok(name) = std::str::from_utf8(key_bytes) {
                                if let Some(&getter_ptr) = vtable.getters.get(name) {
                                    // Getters take `this` as f64 (NaN-boxed
                                    // POINTER_TAG), matching the codegen
                                    // calling convention for class methods.
                                    let this_f64: f64 = class_getter_this(obj);
                                    let f: extern "C" fn(f64) -> f64 =
                                        std::mem::transmute(getter_ptr);
                                    return JSValue::from_bits(f(this_f64).to_bits());
                                }
                            }
                        }
                        match get_parent_class_id(cid) {
                            Some(p) if p != 0 && p != cid => {
                                cid = p;
                                depth += 1;
                            }
                            _ => break,
                        }
                    }
                }
            }

            // Issue #711 part 2: walk the class chain for a registered
            // prototype object (from `Function.prototype = X`). When
            // found, the method is an own-property of the proto
            // object — return its value directly. `pipe`, `[Equal.symbol]`,
            // etc. on Effect's EffectPrototype reach here.
            {
                let receiver =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                if let Some(v) = resolve_proto_chain_field_with_receiver(class_id, key, receiver) {
                    return v;
                }
            }

            // Issue #838: JS-classic `Class.prototype.method = fn`
            // assignment registered via `js_register_prototype_method`.
            // Read returns the stored closure value directly, mirroring
            // Node's `Object.getPrototypeOf(inst).method` lookup. The
            // bound-method-closure fallback below handles vtable methods;
            // this arm covers methods that only exist as prototype
            // assignments (never declared inside the `class` block).
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if let Some(v) = lookup_prototype_method(class_id, name) {
                    return JSValue::from_bits(v.to_bits());
                }
                if class_id == crate::builtins::CONSOLE_INSTANCE_CLASS_ID
                    && crate::builtins::is_console_instance_method_name(name)
                {
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }

            // v0.5.756: method-as-value fallback. If `obj.method` reads via
            // the runtime path (Any-typed receiver, so the codegen #446
            // arm at expr.rs:3596 didn't fire), look up the method in the
            // class vtable chain and return a bound-method closure
            // (BOUND_METHOD_FUNC_PTR sentinel + (this, name_ptr, name_len)
            // captures). This makes both `typeof obj.method === "function"`
            // and `obj.method(args)` work for class methods on Any-typed
            // receivers — the closure-call dispatch routes through
            // `js_native_call_method` which walks the same vtable chain.
            // Refs #446 / drizzle's `(ins as any)._prepare()` chain.
            //
            // Method IDENTITY (test262 class/elements): `js_class_method_bind`
            // routes user-class method-as-value reads through a single cached
            // canonical per `(owner_class, name)`, so `c.m === C.prototype.m`
            // and `c1.m === c2.m` hold (and an own data property of the same
            // name still shadows it). Actual `obj.method(args)` calls don't flow
            // through here — they lower directly to `js_native_call_method`.
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if lookup_class_method_in_chain(class_id, name).is_some() {
                    // Allocate a fresh i8 buffer for the method name owned
                    // by the closure. The keys_array's StringHeader bytes
                    // could in theory be GC'd if the keys_array is not
                    // pinned for the closure's lifetime.
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }
        }

        // #2820: before giving up, walk an explicit `Object.setPrototypeOf`
        // prototype chain recorded for this object so inherited property reads
        // (`obj.x` where `x` is an own property of the set prototype) resolve.
        if !key.is_null() {
            if let Some(v) =
                super::super::prototype_chain::resolve_inherited_field(obj as usize, key)
            {
                return v;
            }
            if let Some(v) = ordinary_object_prototype_property_value(obj, key) {
                return v;
            }
        }

        // `class X extends Request/Response`: inherited native members
        // (`url`/`method`/`headers`/`body`/`bodyUsed`/… and body methods read
        // as values) live on the underlying fetch handle, not the JS prototype
        // chain. Forward the read to the handle when this object stashes one
        // and the key isn't the marker field itself. Refs Hono `c.req` body.
        if !key.is_null() && key_bytes != FETCH_SUBCLASS_HANDLE_FIELD {
            if let Some(id) = fetch_subclass_handle_id(obj as usize) {
                // Body methods (`text`/`json`/`arrayBuffer`/`blob`/`bytes`/
                // `formData`/`clone`) live on the native fetch handle. They must
                // be READABLE as callable values, not just invocable as a fused
                // `inst.text()` (handled by the `js_native_call_method`
                // body-method arm, #4756): codegen lowers `inst.text()` to a
                // property read + call, and @hono/node-server forwards the body
                // through `this[getRequestCache]()[k]()` -- a *computed* read of
                // the native handle method off a `class extends Request`
                // instance. Forwarding that read to the handle as an object
                // pointer yields `undefined` -> "text is not a function". Return
                // a bound method that re-dispatches through
                // `js_native_call_method`, whose body-method arm forwards to the
                // handle. Refs Hono `c.req.text()` / `.json()` / `.formData()`.
                if is_fetch_subclass_body_method(key_bytes) {
                    let this_f64 = crate::value::js_nanbox_pointer(obj as i64);
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let bound = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(bound.to_bits());
                }
                let v = js_object_get_field_by_name(id as usize as *const ObjectHeader, key);
                if !v.is_undefined() {
                    return v;
                }
            }
        }

        // `class X extends Temporal.<Type>`: inherited accessor getters
        // (`days`/`years`/`epochNanoseconds`/…) resolve via the Temporal brand on
        // the underlying cell, not the JS prototype chain. Forward the read to
        // the stashed cell when this object has one. Skip BOTH the temporal and
        // fetch marker keys: reading a marker here would re-enter this tail and
        // (cross-) trigger the other marker's reader, an infinite recursion that
        // stack-overflows. Methods read as fused `inst.m(...)` calls are handled
        // in `native_call_method.rs`. (#5587)
        #[cfg(feature = "temporal")]
        if !key.is_null()
            && key_bytes != crate::object::TEMPORAL_SUBCLASS_CELL_FIELD
            && key_bytes != FETCH_SUBCLASS_HANDLE_FIELD
        {
            if let Some(cell) = crate::object::temporal_subclass_cell(obj as usize) {
                let name = String::from_utf8_lossy(key_bytes);
                if let Some(v) = crate::temporal::dispatch::get_property(cell, &name) {
                    return JSValue::from_bits(v.to_bits());
                }
                // A prototype METHOD read as a value (`sub.abs`, not `sub.abs()`):
                // return a bound method that re-dispatches through
                // `js_native_call_method` (whose Temporal-subclass arm forwards to
                // the cell). Only bind genuine method names so an unknown property
                // still reads as `undefined`. Mirrors the fetch body-method bind.
                if crate::temporal::dispatch::has_method(cell, &name) {
                    let this_f64 = crate::value::js_nanbox_pointer(obj as i64);
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let bound = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(bound.to_bits());
                }
            }
        }

        // #5961: native URLSearchParams is an ordinary object (class_id == 0,
        // leading `_entries` slot) whose method surface normally exists only
        // via static type-directed lowering. A type-erased receiver lands
        // here instead — resolve the methods dynamically so `sp.append(...)`
        // stays callable, and `size` reads as a number.
        if !key.is_null() && crate::url::search_params::shape_is_url_search_params(obj) {
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if name == "size" {
                    let n = crate::url::search_params::js_url_search_params_size(
                        obj as *mut ObjectHeader,
                    );
                    return JSValue::from_bits((n as f64).to_bits());
                }
                if let Some(v) =
                    crate::url::search_params::url_search_params_method_value(obj, name)
                {
                    return JSValue::from_bits(v.to_bits());
                }
            }
        }

        // #6301: `EventTarget`'s method surface, read as a VALUE
        // (`typeof b.dispatchEvent`, `const add = t.addEventListener`).
        // Deliberately LAST in the tail so an own property and a real
        // class-vtable method (a subclass that *overrides* `dispatchEvent`) are
        // resolved earlier and keep winning. See
        // `event_target::event_target_value_read` for why placing it after the
        // `keys_array.is_null()` early return above is correct.
        if !key.is_null() {
            if let Some(bound) =
                crate::event_target::event_target_value_read(obj as *mut ObjectHeader, key_bytes)
            {
                return JSValue::from_bits(bound.to_bits());
            }
        }

        // Key not found
        JSValue::undefined()
    }
}
