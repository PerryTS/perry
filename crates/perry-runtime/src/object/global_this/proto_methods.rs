use super::*;
// Array.prototype thunks live in the sibling `array_error` module (split out of
// `global_this`); pull them in directly so the prototype-install tables resolve
// `array_proto_*_thunk` without routing through the trunk re-exports.
use super::array_error::*;

fn web_method_receiver(name: &str) -> *mut ObjectHeader {
    let receiver = crate::object::js_implicit_this_get();
    if crate::object::web_builtin_to_string_tag(receiver) == Some(name) {
        return crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader;
    }
    let message = format!("Value of this must be of type {name}");
    let text = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_typeerror_new(text);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64))
}

extern "C" fn url_prototype_href_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::url::js_url_get_href(web_method_receiver("URL"))
}

extern "C" fn abort_controller_prototype_abort_thunk(
    _closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    crate::url::js_abort_controller_abort_reason(web_method_receiver("AbortController"), reason);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn abort_signal_prototype_throw_if_aborted_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::url::js_abort_signal_throw_if_aborted(web_method_receiver("AbortSignal"))
}

fn web_method_enumerable(proto_obj: *mut ObjectHeader, name: &str) {
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

/// Install a FIXED-string `Symbol.toStringTag` data property (`{ value: tag,
/// writable: false, enumerable: false, configurable: true }`, ES2019
/// WebIDL/`get %TypedArray%.prototype [ @@toStringTag ]` sibling shape but a
/// plain data property rather than a getter -- these Web API interfaces
/// each own a single fixed tag, unlike the shared TypedArray prototype) on
/// `proto_obj`. #10555: `Object.prototype.toString.call(x)` and
/// `x[Symbol.toStringTag]` for these types are ALSO answered directly by
/// `crate::object::web_builtin_to_string_tag` (`object/to_string_tag.rs`)
/// for every instance shape that reaches it -- most of these types' own
/// instances never link `[[Prototype]]` back to this very `proto_obj` (see
/// that function's doc comment), so that synthesized answer is load-bearing
/// for `x[Symbol.toStringTag]`/`toString.call(x)` on an INSTANCE. This
/// installs the matching descriptor on the constructor's `.prototype`
/// object itself so `Object.getOwnPropertyDescriptor(Ctor.prototype,
/// Symbol.toStringTag)` also reflects a real, correctly-shaped descriptor
/// (test262-style reflection, and libraries that copy descriptors off the
/// prototype rather than reading the instance).
unsafe fn install_web_builtin_to_string_tag(proto_obj: *mut ObjectHeader, tag: &str) {
    if proto_obj.is_null() {
        return;
    }
    let symbol = crate::symbol::well_known_symbol("toStringTag");
    if symbol.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    let value = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
    crate::symbol::js_object_set_symbol_property(
        crate::value::js_nanbox_pointer(proto_obj as i64),
        crate::value::js_nanbox_pointer(symbol as i64),
        value,
    );
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        symbol as usize,
        crate::object::PropertyAttrs::new(false, false, true),
    );
}

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
/// These methods retain dedicated thunks for spec-accurate call behavior —
/// `Array.prototype.slice` (ramda's curry/variadic helpers
/// reach through `Array.prototype.slice.call(args, …)` and depend on it
/// returning a real sliced array, even via indirection),
/// `Array.prototype.toString` (generic `join` dispatch), and
/// `Object.prototype.toString` (ramda's `_isArguments.js` IIFE calls
/// `Object.prototype.toString.call(arguments)` at module-init time).
/// All other methods are noop-backed: typeof + `.name` introspection
/// works, but a stored-and-called-indirect reference returns undefined.
/// The common forms — `arr.map(fn)` (codegen's NativeMethodCall) and
/// `Array.prototype.map.call(arr, fn)` (HIR rewrite, see
/// `try_builtin_prototype_method_apply_call`) — are unaffected.
/// #7760: install `value` as `proto_obj`'s OWN `[Symbol.iterator]`, with the
/// spec descriptor. Mirrors `collection_proto_thunks::install_collection_iterator_symbol`;
/// kept here because `Array.prototype` is populated in this module.
fn install_builtin_iterator_symbol(proto_obj: *mut ObjectHeader, value: f64) {
    if proto_obj.is_null() || value.to_bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let iter = crate::symbol::well_known_symbol("iterator");
    if iter.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto_obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits()),
            value,
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        iter as usize,
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// #10482: install `Object.prototype.__proto__` as a REAL accessor
/// descriptor — `{ get, set, enumerable: false, configurable: true }`, per
/// ECMA-262 Annex B §B.3.1 — instead of the purely behavioral special-casing
/// Perry had before (reads/writes worked through `__proto__` as a magic key
/// name in several call sites, but nothing on `Object.prototype` reflected
/// it). `hasOwnProperty`/`Object.hasOwn`/`getOwnPropertyNames`/
/// `getOwnPropertyDescriptor`/`"__proto__" in {}`/`Reflect.ownKeys` all read
/// `ACCESSOR_DESCRIPTORS`/`PROPERTY_DESCRIPTORS` unconditionally, so a real
/// entry here is what makes them agree with Node.
///
/// Uses `set_builtin_accessor_descriptor` (gate-neutral): it does not flip
/// `GLOBAL_DESCRIPTORS_IN_USE` / `ACCESSORS_IN_USE`, so ordinary property
/// read/write fast paths are unaffected for every OTHER key. `__proto__`
/// itself was already treated as unconditionally interceptable by
/// `object_proto_may_intercept_key` / `plain_custom_prototype_may_intercept`
/// (see `object/descriptor_state.rs`) before this change, so installing a
/// real descriptor for it changes no hot-path gate this key didn't already
/// trip — only what reflection sees.
///
/// The getter delegates to `js_object_get_prototype_of`, which already
/// implements the getter's exact spec shape (ToObject-style wrapper
/// resolution for primitives, Proxy/Temporal/handle receivers, and a throw
/// on `null`/`undefined`). The setter delegates to
/// `proxy::legacy_dunder_proto_set`, the same Annex-B logic `proxy.rs`'s
/// `ordinary_set_with_receiver` used to inline for this one key (#6828) —
/// now shared so both call sites can never drift apart. Once this
/// descriptor exists, `own_set_descriptor` finds it and dispatches through
/// the ordinary accessor-setter path before that inlined special case is
/// ever reached (see the comment there).
fn install_object_prototype_dunder_proto(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let getter = crate::closure::js_closure_alloc(
        object_prototype_dunder_proto_getter_thunk as *const u8,
        0,
    );
    let setter = crate::closure::js_closure_alloc(
        object_prototype_dunder_proto_setter_thunk as *const u8,
        0,
    );
    if getter.is_null() || setter.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(
        object_prototype_dunder_proto_getter_thunk as *const u8,
        0,
    );
    crate::closure::js_register_closure_arity(
        object_prototype_dunder_proto_setter_thunk as *const u8,
        1,
    );
    super::super::native_module::set_bound_native_closure_name(getter, "get __proto__");
    super::super::native_module::set_bound_native_closure_name(setter, "set __proto__");
    super::super::native_module::set_builtin_closure_length(getter as usize, 0);
    super::super::native_module::set_builtin_closure_length(setter as usize, 1);
    super::super::native_module::set_builtin_closure_non_constructable(getter as usize);
    super::super::native_module::set_builtin_closure_non_constructable(setter as usize);
    let get_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
    let set_bits = crate::value::js_nanbox_pointer(setter as i64).to_bits();
    // A descriptor alone doesn't make the name enumerable by
    // `getOwnPropertyNames`/`hasOwnProperty`/`Object.hasOwn`/
    // `Reflect.ownKeys` — those walk the object's OWN KEYS ARRAY, which
    // `set_builtin_accessor_descriptor` (deliberately gate-neutral) never
    // touches. Write an ordinary placeholder field first, exactly like
    // `perf_hooks::install_perf_getter`: this appends `"__proto__"` to the
    // keys array via the ordinary field-set path, and the accessor
    // descriptor installed right after takes over every actual read/write —
    // the placeholder `undefined` is never observed.
    let key = crate::string::js_string_from_bytes(b"__proto__".as_ptr(), 9);
    js_object_set_field_by_name(proto_obj, key, f64::from_bits(crate::value::TAG_UNDEFINED));
    super::super::set_builtin_accessor_descriptor(
        proto_obj as usize,
        "__proto__".to_string(),
        super::super::AccessorDescriptor {
            get: get_bits,
            set: set_bits,
        },
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

extern "C" fn object_prototype_dunder_proto_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    // Spec (Annex B §B.3.1 `get __proto__`): `ToObject(this).[[GetPrototypeOf]]()`.
    // `js_object_get_prototype_of` already implements exactly this shape —
    // wrapper-prototype resolution for primitives, Proxy/Temporal/handle
    // receivers, and a throw on `null`/`undefined` (the `ToObject` failure
    // case) — so the getter is a direct delegation, not a reimplementation.
    let receiver = crate::object::js_implicit_this_get();
    crate::object::js_object_get_prototype_of(receiver)
}

extern "C" fn object_prototype_dunder_proto_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    crate::proxy::legacy_dunder_proto_set(receiver, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Install a native accessor (getter only) on a builtin prototype. The getter
/// is an ordinary `ClosureHeader` that reads its receiver from `IMPLICIT_THIS`
/// and brand-checks it, so `Object.getOwnPropertyDescriptor(P, k).get.call({})`
/// throws like node's.
///
/// Two things this must NOT do, both learned rather than assumed:
///
/// * it must not use `install_fresh_accessor_property`, which flips the
///   process-wide `GLOBAL_DESCRIPTORS_IN_USE` gate (#6809) and would push every
///   later dynamic property write in ANY program that merely constructs a
///   `TextDecoder` onto the descriptor-interception slow walk;
/// * it must put the key in the prototype's keys array, or
///   `Object.getOwnPropertyNames(TextDecoder.prototype)` and `for...in` on an
///   instance would not see it.
///
/// `install_builtin_getter` does both. It records the ECMA-262 builtin shape
/// (`enumerable: false`), which is right for its other callers
/// (`ArrayBuffer.prototype.byteLength`), so the WebIDL flags are restated on
/// the same entry afterwards: interface members are
/// `{ enumerable: true, configurable: true }`, and node prints
/// `for-in d: decode,encoding,fatal,ignoreBOM`.
#[cfg(feature = "global-text")]
fn install_text_accessor(proto_obj: *mut ObjectHeader, name: &str, getter: *const u8) {
    unsafe {
        crate::closure::js_register_closure_arity(getter, 0);
        let closure = crate::closure::js_closure_alloc(getter, 0);
        if closure.is_null() {
            return;
        }
        let get_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        // Spec: an accessor getter's own `.name` is `"get " + key` and its
        // `.length` is 0, and it is not constructable.
        super::super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        super::super::native_module::set_builtin_closure_length(closure as usize, 0);
        super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
        install_builtin_getter(proto_obj, name, get_bits);
        super::super::set_builtin_accessor_descriptor(
            proto_obj as usize,
            name.to_string(),
            super::super::AccessorDescriptor {
                get: get_bits,
                set: 0,
            },
            super::super::PropertyAttrs::new(true, true, true),
        );
    }
}

/// Mark an already-installed prototype method enumerable. WebIDL interface
/// members are enumerable; `install_proto_method` records the ECMA-262 builtin
/// shape (`enumerable: false`), which is right for its other callers, so the
/// text family restates its own entries here instead.
#[cfg(feature = "global-text")]
fn make_proto_member_enumerable(proto_obj: *mut ObjectHeader, name: &str) {
    super::super::set_builtin_property_attrs(
        proto_obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
}

pub(crate) fn populate_builtin_prototype_methods(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // #3662: Map/Set/WeakMap/WeakSet prototypes get brand-checking thunks
    // (own module, to keep this file under the 2000-line gate).
    if collection_proto_thunks::install_collection_proto_methods(builtin_name, proto_obj) {
        set_intrinsic_to_string_tag(proto_obj, builtin_name);
        install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        return;
    }
    // #7947: WeakRef / FinalizationRegistry prototypes get brand-checking
    // thunks, so `WeakRef.prototype.deref.call(wr)`, `wr.deref.bind(wr)` and
    // `typeof wr.deref` resolve instead of answering `undefined`.
    if super::super::weakref_proto_thunks::install_weakref_proto_methods(builtin_name, proto_obj) {
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
            install_proto_method(
                proto_obj,
                "toString",
                array_prototype_to_string_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("entries", 0),
                    ("flatMap", 1),
                    ("keys", 0),
                    ("toLocaleString", 0),
                    ("toReversed", 0),
                    ("toSorted", 1),
                    ("toSpliced", 2),
                    ("with", 2),
                ],
            );
            install_proto_method(
                proto_obj,
                "flat",
                array_prototype_flat_thunk as *const u8,
                0,
            );
            // Generic mutators get REAL thunks (vs the noop above) so a borrowed
            // reference works: `obj.pop = Array.prototype.pop; obj.pop()` and
            // `Array.prototype.splice.call(obj, …)`. Each reads IMPLICIT_THIS and
            // runs the array algorithm on a real array or array-like object.
            // #7760: `values` was a NOOP in the list above, and
            // `Array.prototype[Symbol.iterator]` was not an own property at all
            // — the symbol getter synthesized a receiver-bound method on every
            // read. Install one real function and make it the own `@@iterator`,
            // matching Map/Set/String/%TypedArray%, which already do this
            // (`install_collection_iterator_symbol`). Attributes are the spec's
            // `{ writable: true, enumerable: false, configurable: true }`.
            let values_value = install_proto_method(
                proto_obj,
                "values",
                array_prototype_values_thunk as *const u8,
                0,
            );
            install_builtin_iterator_symbol(proto_obj, values_value);
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
            // #6908: `fill` / `copyWithin` get real thunks too — a Proxy
            // receiver resolves them through `Get(proxy, name)` to the
            // prototype thunk, so the previous noop backing silently returned
            // undefined without firing a trap.
            install_proto_method_rest_with_length(
                proto_obj,
                "fill",
                array_prototype_fill_thunk as *const u8,
                1,
                0,
            );
            install_proto_method_rest_with_length(
                proto_obj,
                "copyWithin",
                array_prototype_copy_within_thunk as *const u8,
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
            install_proto_method(proto_obj, "slice", array_buffer_slice_thunk as *const u8, 2);
            // ES2024 (#10873): `resize`, `transfer`, `transferToFixedLength`.
            install_proto_method(
                proto_obj,
                "resize",
                array_buffer_resize_thunk as *const u8,
                1,
            );
            // The thunks take the optional `newLength` (call arity 1); the
            // spec `.length` of both is 0.
            let transfers: [(&str, *const u8); 2] = [
                ("transfer", array_buffer_transfer_thunk as *const u8),
                (
                    "transferToFixedLength",
                    array_buffer_transfer_to_fixed_length_thunk as *const u8,
                ),
            ];
            for (name, thunk) in transfers {
                let installed = install_proto_method(proto_obj, name, thunk, 1);
                let closure = crate::value::js_nanbox_get_pointer(installed) as usize;
                if closure != 0 {
                    super::super::native_module::set_builtin_closure_length(closure, 0);
                }
            }
            let getters: [(&str, *const u8); 4] = [
                (
                    "byteLength",
                    array_buffer_byte_length_getter_thunk as *const u8,
                ),
                (
                    "resizable",
                    array_buffer_resizable_getter_thunk as *const u8,
                ),
                (
                    "maxByteLength",
                    array_buffer_max_byte_length_getter_thunk as *const u8,
                ),
                ("detached", array_buffer_detached_getter_thunk as *const u8),
            ];
            for (name, thunk) in getters {
                unsafe {
                    crate::closure::js_register_closure_arity(thunk, 0);
                    let getter = crate::closure::js_closure_alloc(thunk, 0);
                    if !getter.is_null() {
                        let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                        install_builtin_getter(proto_obj, name, getter_bits);
                    }
                }
            }
            set_intrinsic_to_string_tag(proto_obj, "ArrayBuffer");
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
            set_intrinsic_to_string_tag(proto_obj, "DataView");
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
            install_object_prototype_dunder_proto(proto_obj);
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
            let to_string = install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::bigint_proto_to_string_thunk as *const u8,
                1,
            );
            super::super::native_module::set_builtin_closure_length(
                crate::value::js_nanbox_get_pointer(to_string) as usize,
                0,
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
        #[cfg(feature = "global-url")]
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
        // #10759: `URLSearchParams` previously had ONLY the `#10555` arm
        // below (moved here): `install_web_builtin_to_string_tag` and
        // nothing else, because its methods dispatch through type-directed
        // static dispatch / the small-int/handle dispatch tables and never
        // needed reified closures for ordinary `x.method()` calls. That
        // design has no answer for a method read AS A VALUE --
        // `URLSearchParams.prototype.append`, `.prototype["has"]`, or
        // through a Proxy `get` trap indirection -- which returned
        // `undefined` instead of a callable closure. node-fetch's
        // `Headers extends URLSearchParams` -- whose constructor returns
        // `new Proxy(this, { get(target, p, receiver) { ... return
        // (...)=> URLSearchParams.prototype[p].call(target, ...); } })` --
        // then threw "Function.prototype.call was called on a value that is
        // not a function" on the very first `headers.has(...)`, reached by
        // every `fetch()` call before the request is even sent. Same
        // mechanism as the `Stream.prototype`/`Object.hasOwnProperty`/
        // `Function.toString` fixes elsewhere (`install_static.rs`,
        // `node_stream_dispatch.rs`): install the no-op-backed reified
        // closures so a value read resolves to a real (name-carrying)
        // function, which `Function.prototype.call`/`.apply`'s
        // `try_dispatch_value_called_proto_method` re-dispatches by name
        // through `try_url_search_params_dynamic_dispatch` using the
        // caller-supplied receiver. Method set + arities verified against
        // `node --experimental-strip-types` (v26.5.1). The
        // `install_web_builtin_to_string_tag` call is retained so
        // `Object.getOwnPropertyDescriptor(URLSearchParams.prototype,
        // Symbol.toStringTag)` keeps reflecting a real descriptor -- see
        // that function's doc comment. The other six members of the
        // `#10555` group have their value-read methods below (#10808).
        "URLSearchParams" => {
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
                    ("sort", 0),
                    ("toString", 0),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            unsafe { install_web_builtin_to_string_tag(proto_obj, "URLSearchParams") };
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
            set_intrinsic_to_string_tag(proto_obj, "Promise");
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        // #340/#341: these carried `install_noop_proto_methods` placeholders —
        // the prototype looked right to reflection while the real work went
        // through the handle-dispatch table. Instances are ordinary objects
        // now, so the prototype carries the real implementations and an
        // ordinary property lookup finds them.
        #[cfg(feature = "global-text")]
        "TextEncoder" => {
            install_proto_method(
                proto_obj,
                "encode",
                crate::text::text_encoder_encode_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "encodeInto",
                crate::text::text_encoder_encode_into_thunk as *const u8,
                2,
            );
            install_text_accessor(
                proto_obj,
                "encoding",
                crate::text::text_encoder_encoding_getter as *const u8,
            );
            for name in ["encode", "encodeInto"] {
                make_proto_member_enumerable(proto_obj, name);
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            unsafe { install_web_builtin_to_string_tag(proto_obj, "TextEncoder") };
        }
        #[cfg(feature = "global-text")]
        "TextDecoder" => {
            install_proto_method(
                proto_obj,
                "decode",
                crate::text::text_decoder_decode_thunk as *const u8,
                1,
            );
            install_text_accessor(
                proto_obj,
                "encoding",
                crate::text::text_decoder_encoding_getter as *const u8,
            );
            install_text_accessor(
                proto_obj,
                "fatal",
                crate::text::text_decoder_fatal_getter as *const u8,
            );
            install_text_accessor(
                proto_obj,
                "ignoreBOM",
                crate::text::text_decoder_ignore_bom_getter as *const u8,
            );
            make_proto_member_enumerable(proto_obj, "decode");
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            unsafe { install_web_builtin_to_string_tag(proto_obj, "TextDecoder") };
        }
        #[cfg(feature = "global-webfetch")]
        "Headers" => {
            // WHATWG aliases Headers.prototype[Symbol.iterator] to the exact
            // same function as `.entries`.  Besides reflection parity, axios
            // deliberately requires this to be an own prototype member before
            // treating an untrusted header source as iterable.
            let entries_value = install_proto_method(
                proto_obj,
                "entries",
                global_this_builtin_noop_thunk as *const u8,
                0,
            );
            install_builtin_iterator_symbol(proto_obj, entries_value);
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
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
            unsafe { install_web_builtin_to_string_tag(proto_obj, "Headers") };
        }
        #[cfg(feature = "global-webfetch")]
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
            // Web Fetch accessors must be visible as accessor descriptors on
            // the intrinsic prototype, not just as compiler-known handle
            // fields. Libraries such as srvx copy `Response.prototype`
            // descriptors onto lightweight response wrappers and provide their
            // own getter body. If these names are absent from reflection, the
            // wrapper's inherited `.body`/`.headers` reads become undefined and
            // streamed SSR responses are dropped before the underlying stream
            // is ever pulled.
            let accessors: &[&str] = if builtin_name == "Response" {
                &[
                    "body",
                    "bodyUsed",
                    "headers",
                    "ok",
                    "redirected",
                    "status",
                    "statusText",
                    "type",
                    "url",
                ]
            } else {
                &[
                    "body",
                    "bodyUsed",
                    "cache",
                    "credentials",
                    "destination",
                    "duplex",
                    "headers",
                    "integrity",
                    "keepalive",
                    "method",
                    "mode",
                    "redirect",
                    "referrer",
                    "referrerPolicy",
                    "signal",
                    "url",
                ]
            };
            unsafe {
                crate::closure::js_register_closure_arity(
                    global_this_builtin_noop_thunk as *const u8,
                    0,
                );
                for name in accessors {
                    let getter = crate::closure::js_closure_alloc(
                        global_this_builtin_noop_thunk as *const u8,
                        0,
                    );
                    if !getter.is_null() {
                        let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                        install_builtin_getter(proto_obj, name, getter_bits);
                    }
                }
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            unsafe { install_web_builtin_to_string_tag(proto_obj, builtin_name) };
        }
        #[cfg(feature = "global-webfetch")]
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
            unsafe { install_web_builtin_to_string_tag(proto_obj, builtin_name) };
        }
        #[cfg(feature = "global-webfetch")]
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
            unsafe { install_web_builtin_to_string_tag(proto_obj, "FormData") };
        }
        #[cfg(feature = "global-websocket")]
        "WebSocket" => {
            websocket_global::install_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        #[cfg(feature = "global-webcrypto")]
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
        #[cfg(feature = "global-webcrypto")]
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
        #[cfg(feature = "global-webcrypto")]
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
        // #10555 + #10808: these Web API types need BOTH kinds of own
        // prototype property, and the two installs are complementary, not
        // alternatives. WebIDL reifies an interface's *attributes* as
        // accessor properties and its *operations* as data properties whose
        // value is a function, and Node carries both on the same prototype
        // object at once (`Object.keys(URL.prototype)` interleaves
        // `toString`, the twelve component accessors, and `toJSON`). The
        // `Symbol.toStringTag` data property from #10555 is a third,
        // independent own property -- see
        // `install_web_builtin_to_string_tag`'s doc comment.
        //
        // The operations have to be REAL values, not type-directed static
        // dispatch: static dispatch only ever covers `url.toString()` in call
        // position, while `Object.keys(URL.prototype)`, `'toString' in url`,
        // `const { abort } = controller`, `URL.prototype.toString.call(x)` and
        // `for...in` all read the property. That is the #10310 defect class,
        // and #10759 already had to convert `URLSearchParams` for it.
        //
        // Every call below allocates (a closure, its name string, the key
        // string), so `proto_obj` can move underneath us: re-read the
        // prototype from the rooted handle before each one rather than
        // reusing the incoming raw pointer.
        "URL" => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let proto_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto_obj as i64));
            let proto = || {
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader
            };
            unsafe { install_web_builtin_to_string_tag(proto(), "URL") };
            // Install order is Node's own key order: `toString`, then the
            // WebIDL component accessors, then `toJSON`.
            install_proto_method(proto(), "toString", url_prototype_href_thunk as *const u8, 0);
            web_method_enumerable(proto(), "toString");
            // Gated like the other `global-url` member tables: the compiler
            // enables it for any program that names `URL`, so a program that
            // cannot reach this prototype links none of the accessors.
            #[cfg(feature = "global-url")]
            crate::url::prototype::install_url_prototype_accessors(proto());
            install_proto_method(proto(), "toJSON", url_prototype_href_thunk as *const u8, 0);
            web_method_enumerable(proto(), "toJSON");
        }
        "AbortController" => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let proto_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto_obj as i64));
            let proto = || {
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader
            };
            // The thunk takes the `reason` argument, but WebIDL's
            // `abort(optional any reason)` is an optional argument, so the
            // spec `.length` is 0 -- overwrite the arity-derived value.
            let method = install_proto_method(
                proto(),
                "abort",
                abort_controller_prototype_abort_thunk as *const u8,
                1,
            );
            if JSValue::from_bits(method.to_bits()).is_pointer() {
                let closure = crate::value::js_nanbox_get_pointer(method) as usize;
                super::super::native_module::set_builtin_closure_length(closure, 0);
            }
            web_method_enumerable(proto(), "abort");
            unsafe { install_web_builtin_to_string_tag(proto(), "AbortController") };
        }
        "AbortSignal" => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let proto_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto_obj as i64));
            let proto = || {
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader
            };
            // DELIBERATELY NOT `web_method_enumerable`. Node deviates from
            // plain WebIDL for this one member: `throwIfAborted` and the
            // `reason` accessor are both NON-enumerable on
            // `AbortSignal.prototype`, while every other operation in this
            // group is enumerable. Measured on the pinned oracle
            // (`.node-version`, v26.5.1):
            //
            //   AbortSignal.throwIfAborted  method    enumerable=false
            //                                         configurable=true
            //                                         writable=true length=0
            //   AbortSignal.reason          accessor  enumerable=false
            //   AbortSignal.aborted         accessor  enumerable=true
            //   Object.keys(AbortSignal.prototype) === ["aborted", "onabort"]
            //
            // contrast `AbortController.abort` / `EventTarget.*` / `Event.*`,
            // all enumerable=true. `install_proto_method`'s own default is
            // `{ writable: true, enumerable: false, configurable: true }`, so
            // the correct thing here is to add NO enumerability override.
            // Do not "fix" this into the shared helper -- a uniform install
            // across the five arms silently diverges from Node.
            // `test_gap_10808_web_proto_methods.ts` pins it.
            install_proto_method(
                proto(),
                "throwIfAborted",
                abort_signal_prototype_throw_if_aborted_thunk as *const u8,
                0,
            );
            unsafe { install_web_builtin_to_string_tag(proto(), "AbortSignal") };
        }
        "EventTarget" | "Event" => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let proto_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto_obj as i64));
            crate::event_target::install_web_event_proto_methods(builtin_name, proto_obj);
            let proto =
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader;
            unsafe { install_web_builtin_to_string_tag(proto, builtin_name) };
        }
        "CustomEvent" => unsafe { install_web_builtin_to_string_tag(proto_obj, "CustomEvent") },
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
