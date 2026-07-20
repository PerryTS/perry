use super::super::*;
use super::*;
// Array.prototype thunks live in the sibling `array_error` module (split out of
// `global_this`); pull them in directly so the prototype-install tables resolve
// `array_proto_*_thunk` without routing through the trunk re-exports.
use super::array_error::*;

/// Universal `Object.prototype` methods inherited by every receiver in
/// JS. Installed on every built-in constructor's prototype since Perry's
/// prototype chain on these built-ins doesn't walk back up to a shared
/// `Object.prototype` — so `Number.prototype.hasOwnProperty` would
/// otherwise be missing.
const OBJECT_PROTO_METHODS: &[(&str, u32)] = &[
    ("hasOwnProperty", 1),
    ("isPrototypeOf", 1),
    ("propertyIsEnumerable", 1),
    ("toLocaleString", 0),
    ("valueOf", 0),
    // Annex B §B.2.2 legacy accessor helpers.
    ("__defineGetter__", 2),
    ("__defineSetter__", 2),
    ("__lookupGetter__", 1),
    ("__lookupSetter__", 1),
    // `toString` is installed separately on Object/typed arrays etc. with
    // dedicated thunks; do not include it here to avoid clobbering those.
];

/// Populate well-known method properties on a built-in constructor's
/// prototype object. Each registered method is a closure carrying a
/// proper `name` property so feature-detection idioms like
/// `typeof Array.prototype.map === "function"` and `.name === "map"`
/// agree with Node when the value is read through indirection.
///
/// Two of these methods retain dedicated thunks for spec-accurate call
/// behavior — `Array.prototype.slice` (ramda's curry/variadic helpers
/// reach through `Array.prototype.slice.call(args, …)` and depend on it
/// returning a real sliced array, even via indirection) and
/// `Object.prototype.toString` (ramda's `_isArguments.js` IIFE calls
/// `Object.prototype.toString.call(arguments)` at module-init time).
/// All other methods are noop-backed: typeof + `.name` introspection
/// works, but a stored-and-called-indirect reference returns undefined.
/// The common forms — `arr.map(fn)` (codegen's NativeMethodCall) and
/// `Array.prototype.map.call(arr, fn)` (HIR rewrite, see
/// `try_builtin_prototype_method_apply_call`) — are unaffected.
pub(crate) fn populate_builtin_prototype_methods(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // #3662: Map/Set/WeakMap/WeakSet prototypes get brand-checking thunks
    // (own module, to keep this file under the 2000-line gate).
    if collection_proto_thunks::install_collection_proto_methods(builtin_name, proto_obj) {
        install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        return;
    }
    // #4795: TC39 explicit-resource-management stacks.
    if super::super::disposable_proto_thunks::install_disposable_proto_methods(
        builtin_name,
        proto_obj,
    ) {
        install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        return;
    }
    // #4100: primitive wrapper prototypes need real thunks for their own
    // methods so reflective calls brand-check `this` instead of hitting the
    // generic Object no-op/valueOf fallbacks.
    if primitive_proto_thunks::install_primitive_proto_methods(builtin_name, proto_obj) {
        install_noop_proto_methods(
            proto_obj,
            &[
                ("hasOwnProperty", 1),
                ("isPrototypeOf", 1),
                ("propertyIsEnumerable", 1),
            ],
        );
        if !matches!(builtin_name, "Number" | "BigInt") {
            install_noop_proto_methods(proto_obj, &[("toLocaleString", 0)]);
        }
        return;
    }
    match builtin_name {
        "Array" => {
            install_proto_method(
                proto_obj,
                "slice",
                array_prototype_slice_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("copyWithin", 2),
                    ("entries", 0),
                    ("fill", 1),
                    ("flat", 0),
                    ("flatMap", 1),
                    ("keys", 0),
                    ("toLocaleString", 0),
                    ("toReversed", 0),
                    ("toSorted", 1),
                    ("toSpliced", 2),
                    ("toString", 0),
                    ("values", 0),
                    ("with", 2),
                ],
            );
            // Generic mutators get REAL thunks (vs the noop above) so a borrowed
            // reference works: `obj.pop = Array.prototype.pop; obj.pop()` and
            // `Array.prototype.splice.call(obj, …)`. Each reads IMPLICIT_THIS and
            // runs the array algorithm on a real array or array-like object.
            install_proto_method(proto_obj, "pop", array_prototype_pop_thunk as *const u8, 0);
            install_proto_method(
                proto_obj,
                "shift",
                array_prototype_shift_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "reverse",
                array_prototype_reverse_thunk as *const u8,
                0,
            );
            install_proto_method_rest_with_length(
                proto_obj,
                "push",
                array_prototype_push_thunk as *const u8,
                1,
                0,
            );
            install_proto_method_rest_with_length(
                proto_obj,
                "unshift",
                array_prototype_unshift_thunk as *const u8,
                1,
                0,
            );
            install_proto_method_rest_with_length(
                proto_obj,
                "splice",
                array_prototype_splice_thunk as *const u8,
                2,
                0,
            );
            // `sort` / `concat` get real thunks too: a borrowed
            // `obj.sort = Array.prototype.sort; obj.sort()` must run the
            // generic engine on the receiver (test262 sort/S15.4.4.11_A3_T1,
            // A4_T3, concat/S15.4.4.4_A2_T1) — the previous noop thunk
            // silently returned undefined.
            install_proto_method(
                proto_obj,
                "sort",
                array_prototype_sort_thunk as *const u8,
                1,
            );
            // Iteration / search methods: real generic-engine thunks (rest
            // shape — spec `.length` recorded separately below).
            type RestThunk = extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64;
            let arraylike_thunks: [(&str, RestThunk, u32); 14] = [
                ("forEach", array_proto_forEach_thunk, 1),
                ("map", array_proto_map_thunk, 1),
                ("filter", array_proto_filter_thunk, 1),
                ("some", array_proto_some_thunk, 1),
                ("every", array_proto_every_thunk, 1),
                ("find", array_proto_find_thunk, 1),
                ("findIndex", array_proto_findIndex_thunk, 1),
                ("findLast", array_proto_findLast_thunk, 1),
                ("findLastIndex", array_proto_findLastIndex_thunk, 1),
                ("reduce", array_proto_reduce_thunk, 1),
                ("reduceRight", array_proto_reduceRight_thunk, 1),
                ("indexOf", array_proto_indexOf_thunk, 1),
                ("lastIndexOf", array_proto_lastIndexOf_thunk, 1),
                ("includes", array_proto_includes_thunk, 1),
            ];
            for (name, thunk, len) in arraylike_thunks {
                install_proto_method_rest_with_length(proto_obj, name, thunk as *const u8, len, 0);
            }
            install_proto_method(proto_obj, "at", array_proto_at_thunk as *const u8, 1);
            install_proto_method(proto_obj, "join", array_proto_join_thunk as *const u8, 1);
            install_proto_method_rest_with_length(
                proto_obj,
                "concat",
                array_prototype_concat_thunk as *const u8,
                1,
                0,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "ArrayBuffer" => {
            install_noop_proto_methods(proto_obj, &[("slice", 2)]);
            unsafe {
                crate::closure::js_register_closure_arity(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                let getter = crate::closure::js_closure_alloc(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                if !getter.is_null() {
                    let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                    install_builtin_getter(proto_obj, "byteLength", getter_bits);
                    set_accessor_descriptor(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        AccessorDescriptor {
                            get: getter_bits,
                            set: 0,
                        },
                    );
                    set_property_attrs(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        PropertyAttrs::new(true, false, true),
                    );
                }
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "SharedArrayBuffer" => {
            // Mirror the ArrayBuffer.prototype shape: a brand-checking `slice`
            // (instances dispatch through buffer_dispatch; `.call(notSab)`
            // throws here), a `byteLength` accessor whose getter brand-checks
            // the shared registry, and the `Symbol.toStringTag`.
            install_proto_method(
                proto_obj,
                "slice",
                shared_array_buffer_slice_thunk as *const u8,
                2,
            );
            unsafe {
                crate::closure::js_register_closure_arity(
                    shared_array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                let getter = crate::closure::js_closure_alloc(
                    shared_array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                if !getter.is_null() {
                    let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                    install_builtin_getter(proto_obj, "byteLength", getter_bits);
                    set_accessor_descriptor(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        AccessorDescriptor {
                            get: getter_bits,
                            set: 0,
                        },
                    );
                    set_property_attrs(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        PropertyAttrs::new(true, false, true),
                    );
                }
            }
            set_intrinsic_to_string_tag(proto_obj, "SharedArrayBuffer");
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "DataView" => {
            // Install the reflectable `byteLength`/`byteOffset`/`buffer`
            // accessors and the `get*`/`set*` numeric methods on
            // `DataView.prototype` (own module). Instances already work via
            // codegen / `buffer_dispatch`; these only close the reflection +
            // `DataView.prototype.getInt32.call(dv, …)` cascade.
            super::super::dataview_proto_thunks::install_dataview_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Object" => {
            install_proto_method(
                proto_obj,
                "toString",
                object_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "propertyIsEnumerable",
                object_prototype_property_is_enumerable_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toLocaleString",
                object_prototype_to_locale_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                object_prototype_value_of_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "propertyIsEnumerable",
                object_prototype_property_is_enumerable_thunk as *const u8,
                1,
            );
        }
        "Function" => {
            // `Function.prototype` has own `length` (0) and `name` ("") data
            // properties, each `{ writable: false, enumerable: false,
            // configurable: true }` (ECMA-262 20.2.3). Install them first so
            // `length` precedes `name` in `getOwnPropertyNames` order, matching
            // the built-in-function property order Test262 checks.
            {
                let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                js_object_set_field_by_name(
                    proto_obj,
                    len_key,
                    f64::from_bits(JSValue::number(0.0).bits()),
                );
                super::super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "length".to_string(),
                    super::super::PropertyAttrs::new(false, false, true),
                );
                let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
                let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
                js_object_set_field_by_name(
                    proto_obj,
                    name_key,
                    f64::from_bits(JSValue::string_ptr(empty).bits()),
                );
                super::super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "name".to_string(),
                    super::super::PropertyAttrs::new(false, false, true),
                );
            }
            install_proto_method(
                proto_obj,
                "apply",
                function_prototype_apply_thunk as *const u8,
                2,
            );
            install_proto_method_rest(
                proto_obj,
                "bind",
                function_prototype_bind_thunk as *const u8,
                1,
            );
            // #4101: dedicated toString thunk (source reconstruction + brand
            // check) instead of the shared no-op.
            install_proto_method(
                proto_obj,
                "toString",
                function_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method_rest(
                proto_obj,
                "call",
                function_prototype_call_thunk as *const u8,
                1,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_function_has_instance_symbol(proto_obj);
        }
        "String" => {
            // #4713: generic-`this` char-access methods + `Symbol.iterator`, and
            // every other coercing method (slice/indexOf/split/replace/…) get real
            // reflective thunks (RequireObjectCoercible + ToString) so
            // `String.prototype.slice.call(receiver, …)` works on a boxed/object
            // receiver.
            string_proto_thunks::install_string_proto_methods("String", proto_obj);
            // Install noop Object methods first (includes a noop `valueOf`), then
            // override `toString` and `valueOf` with brand-checking thunks — they
            // must throw TypeError for non-String receivers (ECMA-262 §22.1.3.28
            // `thisStringValue`), unlike the generic coercing methods above.
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::string_proto_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::string_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Number" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("toExponential", 1),
                    ("toFixed", 1),
                    ("toPrecision", 1),
                    ("toString", 1),
                ],
            );
            // OBJECT_PROTO_METHODS installs noop `valueOf`/`toLocaleString`, so
            // it must run BEFORE the brand thunks below — otherwise it clobbers
            // them back to no-ops.
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: `valueOf`/`toLocaleString` brand-check `this` and throw a
            // `TypeError` on an incompatible reflective receiver instead of
            // falling back to `Object.prototype` (`"[object Object]"`).
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::number_proto_value_of_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "toLocaleString",
                primitive_proto_thunks::number_proto_to_locale_string_thunk as *const u8,
                0,
            );
        }
        "Boolean" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: brand-checking `toString`/`valueOf` (mirror `Number`).
            // Installed after OBJECT_PROTO_METHODS so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::boolean_proto_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::boolean_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Symbol" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: Symbol.prototype previously had no own methods, so
            // reflective `Symbol.prototype.toString.call(sym)` resolved to
            // `Object.prototype.toString` (`"[object Symbol]"`) and an
            // incompatible receiver returned `"[object Object]"` instead of
            // throwing. Install brand-checking thunks that re-dispatch to the
            // canonical symbol logic (`"Symbol(x)"`). After OBJECT_PROTO_METHODS
            // so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::symbol_proto_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::symbol_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "BigInt" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: mirror `Symbol` — brand-checking `toString`(radix)/`valueOf`
            // re-dispatched to the canonical BigInt logic (`(5n).toString(2)`
            // → `"101"`). After OBJECT_PROTO_METHODS so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::bigint_proto_to_string_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::bigint_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Date" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("getDate", 0),
                    ("getDay", 0),
                    ("getFullYear", 0),
                    ("getHours", 0),
                    ("getMilliseconds", 0),
                    ("getMinutes", 0),
                    ("getMonth", 0),
                    ("getSeconds", 0),
                    ("getTime", 0),
                    ("getTimezoneOffset", 0),
                    ("getUTCDate", 0),
                    ("getUTCDay", 0),
                    ("getUTCFullYear", 0),
                    ("getUTCHours", 0),
                    ("getUTCMilliseconds", 0),
                    ("getUTCMinutes", 0),
                    ("getUTCMonth", 0),
                    ("getUTCSeconds", 0),
                    ("getYear", 0),
                    ("setDate", 1),
                    ("setFullYear", 3),
                    ("setHours", 4),
                    ("setMilliseconds", 1),
                    ("setMinutes", 3),
                    ("setMonth", 2),
                    ("setSeconds", 2),
                    ("setTime", 1),
                    ("setUTCDate", 1),
                    ("setUTCFullYear", 3),
                    ("setUTCHours", 4),
                    ("setUTCMilliseconds", 1),
                    ("setUTCMinutes", 3),
                    ("setUTCMonth", 2),
                    ("setUTCSeconds", 2),
                    ("setYear", 1),
                    ("toDateString", 0),
                    ("toISOString", 0),
                    ("toJSON", 1),
                    ("toLocaleDateString", 0),
                    ("toLocaleString", 0),
                    ("toLocaleTimeString", 0),
                    ("toTimeString", 0),
                    ("toUTCString", 0),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // Overwrite the no-op getter entries with brand-checking thunks so
            // `Date.prototype.getX.call(this)` performs `thisTimeValue(this)`
            // (TypeError on a non-Date receiver) and dispatches correctly.
            // MUST run after the OBJECT_PROTO_METHODS block, which would
            // otherwise re-clobber `valueOf` with the generic Object no-op.
            date_proto_thunks::install_date_proto_getters(proto_obj);
            // Same treatment for the mutating setters: `Date.prototype.setX`
            // brand-checks `this`, reads `[[DateValue]]` before coercing args,
            // then mutates the cell. Also after the OBJECT_PROTO_METHODS block.
            date_proto_thunks::install_date_proto_setters(proto_obj);
            // Overwrite the no-op toLocaleString with a real Intl-aware thunk
            // that forwards locale/options to temporal_locale_string.
            date_proto_thunks::install_date_proto_to_locale_string(proto_obj);
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toString",
                date_prototype_to_string_thunk as *const u8,
                0,
            );
            // `Date.prototype[Symbol.toPrimitive]` — a generic `OrdinaryToPrimitive`
            // dispatcher (non-enumerable own method, `.name` "[Symbol.toPrimitive]",
            // `.length` 1). test262 `built-ins/Date/prototype/Symbol.toPrimitive/*`.
            date_proto_thunks::install_date_proto_to_primitive(proto_obj);
        }
        "RegExp" => {
            // Real accessor getters (`source`/`flags`/`global`/…) so reflection
            // (`getOwnPropertyDescriptor(RegExp.prototype, "source").get`) and
            // brand-checked `.call(this)` work, and instances inherit them.
            super::super::regex_proto_thunks::install_regex_proto_accessors(proto_obj);
            // Real brand-checking `exec`/`test`/`toString`/`compile` (Annex B).
            super::super::regex_proto_thunks::install_regex_proto_methods(proto_obj);
            // `compile` is installed as a real brand-checking thunk by
            // `install_regex_proto_methods` when `regex-engine` is on; without an
            // engine it falls back to the Annex-B no-op here.
            #[cfg(not(feature = "regex-engine"))]
            install_noop_proto_methods(proto_obj, &[("compile", 2)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "URLPattern" => {
            install_proto_method_rest(proto_obj, "exec", url_pattern_exec_thunk as *const u8, 1);
            install_proto_method_rest(proto_obj, "test", url_pattern_test_thunk as *const u8, 1);
            for name in [
                "hasRegExpGroups",
                "hash",
                "hostname",
                "password",
                "pathname",
                "port",
                "protocol",
                "search",
                "username",
            ] {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                js_object_set_field_by_name(
                    proto_obj,
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                super::super::set_builtin_property_attrs(
                    proto_obj as usize,
                    name.to_string(),
                    super::super::PropertyAttrs::new(false, false, true),
                );
            }
        }
        "Promise" => {
            install_proto_method(
                proto_obj,
                "catch",
                crate::promise::promise_prototype_catch_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "finally",
                crate::promise::promise_prototype_finally_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "then",
                crate::promise::promise_prototype_then_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "TextEncoder" => {
            install_noop_proto_methods(proto_obj, &[("encode", 1), ("encodeInto", 2)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "TextDecoder" => {
            install_noop_proto_methods(proto_obj, &[("decode", 1)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Headers" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getSetCookie", 0),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Request" | "Response" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("blob", 0),
                    ("bytes", 0),
                    ("clone", 0),
                    ("formData", 0),
                    ("json", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Blob" | "File" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("bytes", 0),
                    ("slice", 0),
                    ("stream", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "FormData" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getAll", 1),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "WebSocket" => {
            websocket_global::install_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Crypto" => {
            install_webcrypto_proto_getter(
                proto_obj,
                "subtle",
                webcrypto_subtle_getter_thunk as *const u8,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "getRandomValues",
                webcrypto_get_random_values_thunk as *const u8,
                1,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "randomUUID",
                webcrypto_random_uuid_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "CryptoKey" => {
            for (name, func_ptr) in [
                ("algorithm", cryptokey_algorithm_getter_thunk as *const u8),
                (
                    "extractable",
                    cryptokey_extractable_getter_thunk as *const u8,
                ),
                ("type", cryptokey_type_getter_thunk as *const u8),
                ("usages", cryptokey_usages_getter_thunk as *const u8),
            ] {
                install_webcrypto_proto_getter(proto_obj, name, func_ptr);
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "SubtleCrypto" => {
            for (name, func_ptr, length) in [
                (
                    "encapsulateBits",
                    subtle_crypto_encapsulate_bits_thunk as *const u8,
                    2,
                ),
                (
                    "decapsulateBits",
                    subtle_crypto_decapsulate_bits_thunk as *const u8,
                    3,
                ),
                (
                    "encapsulateKey",
                    subtle_crypto_encapsulate_key_thunk as *const u8,
                    5,
                ),
                (
                    "decapsulateKey",
                    subtle_crypto_decapsulate_key_thunk as *const u8,
                    6,
                ),
            ] {
                install_webcrypto_proto_method_rest_with_length(proto_obj, name, func_ptr, length);
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_proto_method(
                proto_obj,
                "toString",
                error_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
        }
        // Typed-array constructors: keep the reified per-kind prototype
        // method set (#2142) on each per-kind `.prototype` so direct
        // reads like `Int8Array.prototype.at` continue to return a
        // function. The accessor descriptors
        // (`length`/`byteLength`/`byteOffset`/`buffer`) are installed
        // *only* on the shared `%TypedArray%.prototype` (#2145, in
        // `ensure_typed_array_intrinsic`) — reached via
        // `Object.getPrototypeOf(Int8Array.prototype) ===
        // %TypedArray%.prototype`. Pre-#2145 they were also stamped on
        // each per-kind proto because `getPrototypeOf(per_kind)`
        // returned identity; now that it walks to the intrinsic, they
        // belong on the parent (matches Node's
        // `getOwnPropertyDescriptor(Int8Array.prototype, "length")` =
        // `undefined`).
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => {
            // Per spec the per-kind prototype is nearly empty: every method,
            // accessor, `Symbol.iterator`, `Symbol.toStringTag`, `toString`,
            // and `toLocaleString` lives on the shared `%TypedArray%.prototype`
            // (this proto's `[[Prototype]]`) and is *inherited*, not own — so
            // `Int8Array.prototype.hasOwnProperty("map") === false` and
            // `Int8Array.prototype.map === %TypedArray%.prototype.map`
            // (test262 `prototype/*/inherited.js`). The only own properties are
            // `constructor` (set in the constructor-setup path) and
            // `BYTES_PER_ELEMENT`. The static-prototype link to the intrinsic
            // is wired alongside the `OBJ_FLAG_TYPED_ARRAY_PROTO` flag so the
            // generic property-get chain walk resolves the inherited methods.
        }
        _ => {}
    }
}

pub(crate) fn install_error_prototype_data_properties(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) {
    let name = match builtin_name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" | "SuppressedError" => builtin_name,
        _ => return,
    };
    if proto_obj.is_null() {
        return;
    }

    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_value =
        crate::string::js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        proto_obj,
        name_key,
        crate::value::js_nanbox_string(name_value as i64),
    );
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let message_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message_value = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    js_object_set_field_by_name(
        proto_obj,
        message_key,
        crate::value::js_nanbox_string(message_value as i64),
    );
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        "message".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

fn install_webcrypto_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    install_proto_method(proto_obj, method_name, func_ptr, arity);
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

fn install_webcrypto_proto_method_rest_with_length(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    length: u32,
) {
    install_proto_method_rest_with_length(proto_obj, method_name, func_ptr, length, 0);
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

fn install_webcrypto_proto_getter(proto_obj: *mut ObjectHeader, name: &str, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let value = if closure.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        super::super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        crate::value::js_nanbox_pointer(closure as i64)
    };
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto_obj, key, f64::from_bits(crate::value::TAG_UNDEFINED));
    super::super::set_builtin_accessor_descriptor(
        proto_obj as usize,
        name.to_string(),
        super::super::AccessorDescriptor {
            get: value.to_bits(),
            set: 0,
        },
        super::super::PropertyAttrs::new(true, true, true),
    );
}
