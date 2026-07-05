//! `Object.getOwnPropertyDescriptor(s)`, `Object.getOwnPropertyNames`, and
//! `Object.create` (with descriptor bag) — descriptor introspection and
//! creation, split out of `object_ops.rs` to keep that file under the
//! 2000-line cap (#2816/#2817/#2843). Pure relocation; `use super::*` gives
//! the same visibility the parent module has.

use super::*;

fn property_name_array_index(name: &str) -> Option<u32> {
    if name.is_empty() || (name.len() > 1 && name.as_bytes()[0] == b'0') {
        return None;
    }
    let value = name.parse::<u32>().ok()?;
    if value == u32::MAX || value.to_string() != name {
        return None;
    }
    Some(value)
}

pub(crate) fn sort_property_names_ecma(names: &mut Vec<String>) {
    let mut indexed = Vec::new();
    let mut rest = Vec::new();
    for name in names.drain(..) {
        if let Some(index) = property_name_array_index(&name) {
            indexed.push((index, name));
        } else {
            rest.push(name);
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    names.extend(indexed.into_iter().map(|(_, name)| name));
    names.extend(rest);
}

fn push_unique_name(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

fn boxed_string_payload(value: f64) -> Option<f64> {
    if crate::builtins::boxed_primitive_to_string_tag(value) != Some("String") {
        return None;
    }
    crate::builtins::boxed_primitive_payload(value).map(|(_, payload)| payload)
}

unsafe fn string_value_utf16_len(str_value: f64) -> Option<u32> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, blen) = crate::string::str_bytes_from_jsvalue(str_value, &mut scratch)?;
    if ptr.is_null() {
        return Some(0);
    }
    Some(crate::string::compute_utf16_len(ptr, blen))
}

unsafe fn boxed_string_own_property_names(obj_value: f64, str_value: f64) -> f64 {
    let mut names: Vec<String> = Vec::new();
    let utf16_len = string_value_utf16_len(str_value).unwrap_or(0);
    for i in 0..utf16_len {
        names.push(i.to_string());
    }
    names.push("length".to_string());

    let obj = extract_obj_ptr(obj_value);
    if !obj.is_null() {
        let keys = (*obj).keys_array;
        if !keys.is_null() {
            let len = crate::array::js_array_length(keys) as usize;
            let order = ecma_own_key_order(keys);
            let pos = |j: usize| -> u32 {
                match &order {
                    Some(ord) => ord[j],
                    None => j as u32,
                }
            };
            let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            for j in 0..len {
                let key_val = crate::array::js_array_get(keys, pos(j));
                let Some(name_bytes) = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
                else {
                    continue;
                };
                if let Ok(name) = std::str::from_utf8(name_bytes) {
                    push_unique_name(&mut names, name.to_string());
                }
            }
        }
    }

    sort_property_names_ecma(&mut names);
    let result = crate::array::js_array_alloc(names.len() as u32);
    for name in names {
        let str_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::array::js_array_push(result, JSValue::string_ptr(str_ptr));
    }
    f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.getOwnPropertyDescriptor(obj, key) — returns a data descriptor
/// `{ value, writable, enumerable, configurable }` for data properties, or an
/// accessor descriptor `{ get, set, enumerable, configurable }` for properties
/// installed via `Object.defineProperty(obj, key, { get, set })`. Returns
/// TAG_UNDEFINED if the property doesn't exist.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        // #2818: ToObject(null/undefined) throws TypeError, matching Node.
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // A Proxy is a small registered id, not a heap object — the ordinary
        // resolution below would deref the fake pointer and segfault. The
        // Reflect entry point shares `[[GetOwnProperty]]` semantics (trap +
        // invariant checks + FromPropertyDescriptor) and forwards non-proxies
        // straight back here, so there's no recursion. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            return crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
        }

        // A per-evaluation class object (`ClassExprFresh`, #1772/#1787) is a
        // POINTER-tagged heap object, not a `0x7FFE` class ref, so the
        // `class_ref_id` branch below never fires for it. Its static METHODS
        // live in the class registry keyed by the header class_id (not as own
        // properties), so `getOwnPropertyDescriptor(C, "staticMethod")` reported
        // `undefined` — which broke NestJS's tslib `__decorate` chain
        // (`descriptor.value` on undefined → "reading 'value'") when the Logger
        // class took the fresh path (its getter/methods capture module locals
        // like `DEFAULT_LOGGER`, forcing `ClassExprFresh`). Mirror the class-ref
        // branch's static-method descriptor: a `static m(){}` is a `{ writable,
        // enumerable: false, configurable }` own data property of the
        // constructor. `is_class_object_value` is pointer-safe (it checks the
        // NaN-box tag before any deref). Own per-evaluation static FIELDS fall
        // through to the ordinary own-property path (checked here via
        // `own_key_present` so a field shadows a same-named template method).
        // Skip Symbol keys here: `metadata_key_to_string` / `js_string_coerce`
        // would stringify a Symbol and wrongly return a string-named static
        // method descriptor instead of letting it reach the symbol descriptor
        // path below.
        if super::class_registry::is_class_object_value(obj_value)
            && crate::symbol::js_is_symbol(key_value) == 0
        {
            if let Some(method_name) = metadata_key_to_string(key_value) {
                let obj = extract_obj_ptr(obj_value);
                let key_str = crate::builtins::js_string_coerce(key_value);
                if !obj.is_null() && !key_str.is_null() && !own_key_present(obj, key_str) {
                    let class_id = super::js_object_get_class_id(obj as *const ObjectHeader);
                    if class_id != 0
                        && !super::class_registry::class_is_key_deleted(class_id, &method_name)
                        && super::class_registry::class_has_own_static_method(
                            class_id,
                            &method_name,
                        )
                    {
                        let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
                        let value =
                            super::js_class_method_bind(obj_value, leaked.as_ptr(), leaked.len());
                        return build_data_descriptor(value, true, false, true);
                    }
                }
            }
        }

        // Private elements (`#x`) are stored on the static side / in a class
        // instance's keys_array but are never reflectable own properties, so
        // their descriptor is always undefined. (Plain `{"#fff": 1}` literals
        // carry class_id 0 and are handled by the ordinary path below.)
        {
            let kjv = crate::JSValue::from_bits(key_value.to_bits());
            let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            if let Some(b) = crate::string::js_string_key_bytes(kjv, &mut buf) {
                if b.first() == Some(&b'#') {
                    let is_class = class_ref_id(obj_value).is_some() || {
                        let obj = extract_obj_ptr(obj_value);
                        !obj.is_null()
                            && (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000
                            && crate::object::is_valid_obj_ptr(obj as *const u8)
                            && (*obj).class_id != 0
                    };
                    if is_class {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                }
            }
        }

        // #2818: string primitives box to String objects whose own
        // properties are the index keys "0".."len-1" (writable:false,
        // enumerable:true, configurable:false) plus "length"
        // (writable:false, enumerable:false, configurable:false).
        if obj_jv.is_any_string() {
            return string_primitive_descriptor(obj_value, key_value);
        }
        if let Some(str_value) = boxed_string_payload(obj_value) {
            let desc = string_primitive_descriptor(str_value, key_value);
            if desc.to_bits() != crate::value::TAG_UNDEFINED {
                return desc;
            }
        }

        if crate::symbol::js_is_symbol(key_value) != 0 {
            let owner = crate::symbol::obj_key_from_f64(obj_value);
            let sym_key = crate::symbol::sym_key_from_f64(key_value);
            if owner == 0 || sym_key == 0 {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let attrs = crate::symbol::get_symbol_property_attrs(owner, sym_key)
                .unwrap_or(PropertyAttrs::new(true, true, true));
            if let Some((get, set)) = crate::symbol::symbol_accessor_descriptor_bits(owner, sym_key)
            {
                // A `0` get/set means "absent half" — surface it as `undefined`
                // (not the number `0`) so a get-only accessor reflects
                // `{ get, set: undefined }`.
                let undef = crate::value::TAG_UNDEFINED;
                return build_accessor_descriptor(
                    f64::from_bits(if get == 0 { undef } else { get }),
                    f64::from_bits(if set == 0 { undef } else { set }),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            if let Some(value_bits) = crate::symbol::symbol_property_root_bits(owner, sym_key) {
                return build_data_descriptor(
                    f64::from_bits(value_bits),
                    attrs.writable(),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // TypedArrays are Integer-Indexed exotic objects: a canonical numeric
        // index key resolves to the element as a writable/enumerable/
        // configurable data property (valid index) or to no own property
        // (out-of-bounds / invalid index), never the ordinary keys table.
        match super::typed_array_own_index(obj_value, key_value) {
            super::TypedArrayOwnIndex::Element(value) => {
                return build_data_descriptor(value, true, true, true);
            }
            super::TypedArrayOwnIndex::OutOfBounds => {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            super::TypedArrayOwnIndex::NotTypedArray => {}
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            return crate::typedarray_props::typed_array_get_own_property_descriptor(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
        }

        // Date / RegExp / Error exotic instances: own properties live in the
        // expando side tables (plus a few builtin own slots), never in an
        // `ObjectHeader` — the ordinary path below would bit-cast the cell.
        if let Some((addr, kind)) = super::exotic_expando::exotic_expando_kind_of_value(obj_value) {
            use super::exotic_expando::ExoticKind;
            let Some(name) = super::metadata_key_to_string(key_value) else {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            };
            if let Some(acc) = super::get_accessor_descriptor(addr, &name) {
                let attrs = super::get_property_attrs(addr, &name)
                    .unwrap_or(PropertyAttrs::new(false, false, false));
                let undef = crate::value::TAG_UNDEFINED;
                return build_accessor_descriptor(
                    f64::from_bits(if acc.get == 0 { undef } else { acc.get }),
                    f64::from_bits(if acc.set == 0 { undef } else { acc.set }),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            if let Some(bits) = super::exotic_expando::value_lookup(kind, addr, &name) {
                let attrs = super::get_property_attrs(addr, &name)
                    .unwrap_or(PropertyAttrs::new(true, true, true));
                return build_data_descriptor(
                    f64::from_bits(bits),
                    attrs.writable(),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            // Builtin own slots: RegExp `lastIndex` (writable, non-enum,
            // non-config) and Error `message`/`stack` (writable, non-enum,
            // configurable).
            if kind == ExoticKind::RegExp && name == "lastIndex" {
                let attrs = super::get_property_attrs(addr, &name)
                    .unwrap_or(PropertyAttrs::new(true, false, false));
                let re = addr as *const crate::regex::RegExpHeader;
                return build_data_descriptor(
                    f64::from_bits((*re).last_index),
                    attrs.writable(),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            if kind == ExoticKind::Error && matches!(name.as_str(), "message" | "stack") {
                let attrs = super::get_property_attrs(addr, &name)
                    .unwrap_or(PropertyAttrs::new(true, false, true));
                let err = addr as *mut crate::error::ErrorHeader;
                let s = if name == "message" {
                    crate::error::js_error_get_message(err)
                } else {
                    crate::error::js_error_get_stack(err)
                };
                return build_data_descriptor(
                    f64::from_bits(crate::js_nanbox_string(s as i64).to_bits()),
                    attrs.writable(),
                    attrs.enumerable(),
                    attrs.configurable(),
                );
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        if let Some(class_id) = class_ref_id(obj_value) {
            let method_name = metadata_key_to_string(key_value);
            if let Some(method_name) = method_name {
                if super::class_registry::class_is_key_deleted(class_id, &method_name) {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                // `C.prototype` is a non-writable, non-enumerable, non-configurable
                // own data property of the class constructor (ECMA-262
                // MakeConstructor). Only the constructor ref carries it — the
                // prototype ref's own `prototype` lookup falls through.
                // (Test262 definition/prototype-property.)
                if method_name == "prototype" && super::class_prototype_ref_id(obj_value).is_none()
                {
                    let proto = super::native_module::class_prototype_ref_value(class_id);
                    return build_data_descriptor(proto, false, false, false);
                }
                if method_name == "name"
                    && super::class_prototype_ref_id(obj_value).is_none()
                    && super::class_registry::lookup_static_method_in_chain(class_id, "name")
                        .is_none()
                {
                    if let Some(class_name) = super::class_registry::class_name_for_id(class_id) {
                        let s = crate::string::js_string_from_bytes(
                            class_name.as_ptr(),
                            class_name.len() as u32,
                        );
                        return build_data_descriptor(
                            crate::js_nanbox_string(s as i64),
                            false,
                            false,
                            true,
                        );
                    }
                }
                // Class accessors reflect as accessor descriptors: instance
                // `get x(){}` is an own property of `C.prototype`, a static
                // accessor an own property of `C` itself. The raw vtable
                // func_ptrs are wrapped as callable function values.
                let accessor = if super::class_prototype_ref_id(obj_value).is_some() {
                    super::class_registry::class_own_accessor_ptrs(class_id, &method_name)
                } else {
                    super::class_registry::class_own_static_accessor_ptrs(class_id, &method_name)
                };
                if let Some((g, s)) = accessor {
                    return build_accessor_descriptor(
                        super::class_registry::class_accessor_function_value(
                            g,
                            false,
                            &method_name,
                        ),
                        super::class_registry::class_accessor_function_value(s, true, &method_name),
                        false,
                        true,
                    );
                }
                if method_name == "constructor" || class_has_own_method(class_id, &method_name) {
                    let value = if method_name == "constructor"
                        && super::class_prototype_ref_id(obj_value).is_some()
                        && class_has_own_method(class_id, &method_name)
                    {
                        class_prototype_method_value_for_name(class_id, &method_name)
                    } else if method_name == "constructor" {
                        obj_value
                    } else {
                        class_prototype_method_value_for_name(class_id, &method_name)
                    };
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C2,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
                    *fields = value;
                    *fields.add(1) = f64::from_bits(TAG_TRUE);
                    *fields.add(2) = f64::from_bits(TAG_FALSE);
                    *fields.add(3) = f64::from_bits(TAG_TRUE);
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
                // Static methods are own properties of the class *constructor*
                // (not the prototype). `getOwnPropertyDescriptor(C, "m")` for a
                // `static m() {}` must report a `{ writable, enumerable: false,
                // configurable }` data property — `hasOwnProperty(C, "m")`
                // already returns true, so without this the two disagreed and
                // verifyProperty threw "reading 'enumerable'" on undefined
                // (Test262 elements/after-same-line-static-*).
                if super::class_prototype_ref_id(obj_value).is_none()
                    && super::class_registry::class_has_own_static_method(class_id, &method_name)
                {
                    // Bind the static method to the constructor ref to produce a
                    // callable value, mirroring the `C.m` read path. The name
                    // bytes are leaked (bounded by the static descriptor set) so
                    // the pointer js_class_method_bind stashes stays valid.
                    let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
                    let value =
                        super::js_class_method_bind(obj_value, leaked.as_ptr(), leaked.len());
                    return build_data_descriptor(value, true, false, true);
                }
                // Static FIELDS are own data properties of the constructor,
                // created via CreateDataPropertyOrThrow → writable, enumerable,
                // configurable all true. Codegen registers each declared
                // static field in CLASS_DYNAMIC_PROPS at module init.
                if super::class_prototype_ref_id(obj_value).is_none() {
                    if let Some(v) =
                        super::class_registry::class_own_static_field_value(class_id, &method_name)
                    {
                        return build_data_descriptor(v, true, true, true);
                    }
                }
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // #2059: function objects (closures) are not `ObjectHeader`s — routing
        // them through `extract_obj_ptr`/`own_key_present` below reads an
        // out-of-bounds "keys_array" slot (offset 16, past a 0-capture
        // closure's payload) and segfaults. Resolve their descriptors here:
        // the built-in `name`/`length` slots (non-writable, non-enumerable,
        // configurable per spec) plus any user-attached own data property.
        {
            let jsv = crate::JSValue::from_bits(obj_value.to_bits());
            if jsv.is_pointer() {
                let ptr = jsv.as_pointer::<u8>() as usize;
                if crate::closure::is_closure_ptr(ptr) {
                    let key_str = crate::builtins::js_string_coerce(key_value);
                    if key_str.is_null() {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                    let name_ptr =
                        (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key_str).byte_len as usize;
                    let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                        .unwrap_or("");

                    // #3655: a `delete`d configurable slot is no longer own.
                    if crate::closure::closure_is_key_deleted(ptr, name) {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }

                    // (value, writable, enumerable, configurable). `name`/`length` are the
                    // built-in own data slots; anything else falls back to the
                    // user-attached dynamic-property side table.
                    // Built-in `name`/`length` are spec'd `{ writable: false,
                    // configurable: true }`. A registered descriptor (e.g.
                    // from `install_proto_method`, #3143, or a user
                    // `Object.defineProperty`) overrides those defaults.
                    let registered = super::get_property_attrs(ptr, name);
                    let writable_default = registered.map(|a| a.writable());
                    let configurable_default = registered.map(|a| a.configurable()).unwrap_or(true);
                    if let Some(acc) = super::get_accessor_descriptor(ptr, name) {
                        let attrs =
                            registered.unwrap_or(super::PropertyAttrs::new(false, false, false));
                        let get = if acc.get == 0 {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            f64::from_bits(acc.get)
                        };
                        let set = if acc.set == 0 {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            f64::from_bits(acc.set)
                        };
                        return build_accessor_descriptor(
                            get,
                            set,
                            attrs.enumerable(),
                            attrs.configurable(),
                        );
                    }
                    let resolved: Option<(f64, bool, bool, bool)> = match name {
                        "length" => {
                            let closure_value = crate::value::js_nanbox_pointer(ptr as i64);
                            let arity = if let Some(arity) =
                                super::native_module::bound_native_callable_value_arity(
                                    closure_value,
                                ) {
                                arity
                            } else if let Some(len) =
                                super::native_module::builtin_closure_length(ptr)
                            {
                                // #3143: per-closure spec length for built-in
                                // proto methods (shared func_ptr can't carry it).
                                len
                            } else {
                                crate::closure::closure_length(
                                    ptr as *const crate::closure::ClosureHeader,
                                )
                                .unwrap_or(0)
                            };
                            // Numbers are NaN-boxed as their raw f64 bits.
                            Some((
                                arity as f64,
                                writable_default.unwrap_or(false),
                                false,
                                configurable_default,
                            ))
                        }
                        "name" => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, "name");
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                // Function `.name` is spec'd non-writable; honor
                                // a registered override but otherwise report
                                // `writable: false` (#3143), not the old default
                                // of `true`.
                                Some((
                                    dynv,
                                    writable_default.unwrap_or(false),
                                    false,
                                    configurable_default,
                                ))
                            } else {
                                let func_ptr = (*(ptr as *const crate::closure::ClosureHeader))
                                    .func_ptr
                                    as usize;
                                let fname = crate::builtins::function_name_for_ptr(func_ptr)
                                    .unwrap_or_default();
                                let s = crate::string::js_string_from_bytes(
                                    fname.as_ptr(),
                                    fname.len() as u32,
                                );
                                Some((crate::js_nanbox_string(s as i64), false, false, true))
                            }
                        }
                        _ => {
                            let dynv = crate::closure::closure_get_dynamic_prop(ptr, name);
                            if dynv.to_bits() != crate::value::TAG_UNDEFINED {
                                let attrs = registered
                                    .unwrap_or(super::PropertyAttrs::new(true, true, true));
                                Some((
                                    dynv,
                                    attrs.writable(),
                                    attrs.enumerable(),
                                    attrs.configurable(),
                                ))
                            } else {
                                None
                            }
                        }
                    };
                    let Some((value, writable, enumerable, configurable)) = resolved else {
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    };
                    let packed = b"value\0writable\0enumerable\0configurable";
                    let desc = js_object_alloc_with_shape(
                        0x0D_E5_C0,
                        4,
                        packed.as_ptr(),
                        packed.len() as u32,
                    );
                    let header_size = std::mem::size_of::<ObjectHeader>();
                    let fields = (desc as *mut u8).add(header_size) as *mut f64;
                    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
                    *fields = value;
                    *fields.add(1) = f64::from_bits(if writable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(2) = f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
                    *fields.add(3) =
                        f64::from_bits(if configurable { TAG_TRUE } else { TAG_FALSE });
                    super::rebuild_object_field_layout(desc, 4);
                    return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
                }
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key as a Rust string for descriptor lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };

        if let Some(desc) = super::arguments_object_descriptor(obj, key_str) {
            return desc;
        }
        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = obj as *const crate::array::ArrayHeader;
                let Some(ref name) = key_rust else {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                };
                let is_frozen = crate::array::array_is_frozen(arr);
                if name == "length" {
                    // `defineProperty(arr, "length", {writable:false})` records
                    // the flag in the attrs side table — honor it here.
                    let writable = !is_frozen
                        && get_property_attrs(obj as usize, "length")
                            .map(|a| a.writable())
                            .unwrap_or(true);
                    return build_data_descriptor(
                        crate::array::js_array_length(arr) as f64,
                        writable,
                        false,
                        false,
                    );
                }
                if let Some(index) = super::canonical_array_index(name) {
                    // An array index converted to an accessor via
                    // `Object.defineProperty(arr, i, { get/set })` is recorded in
                    // the accessor side table; report it as an accessor descriptor.
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        let attrs = get_property_attrs(obj as usize, name)
                            .unwrap_or(PropertyAttrs::new(false, false, false));
                        let get = if acc.get == 0 {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            f64::from_bits(acc.get)
                        };
                        let set = if acc.set == 0 {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            f64::from_bits(acc.set)
                        };
                        return build_accessor_descriptor(
                            get,
                            set,
                            attrs.enumerable(),
                            attrs.configurable(),
                        );
                    }
                    if super::has_own_helpers::array_own_key_present(arr, key_str) {
                        let value = crate::array::js_array_get_f64(arr, index);
                        // A dense element defaults to writable/enumerable/
                        // configurable (frozen drops writable+configurable). A
                        // prior `Object.defineProperty(arr, i, {...})` records
                        // explicit attributes in the side table — honor those.
                        let attrs = get_property_attrs(obj as usize, name)
                            .unwrap_or_else(|| PropertyAttrs::new(!is_frozen, true, !is_frozen));
                        return build_data_descriptor(
                            value,
                            attrs.writable(),
                            attrs.enumerable(),
                            attrs.configurable(),
                        );
                    }
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                // Named (non-index) accessor installed via defineProperty.
                if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                    let attrs = get_property_attrs(obj as usize, name)
                        .unwrap_or(PropertyAttrs::new(false, false, false));
                    let undef = crate::value::TAG_UNDEFINED;
                    return build_accessor_descriptor(
                        f64::from_bits(if acc.get == 0 { undef } else { acc.get }),
                        f64::from_bits(if acc.set == 0 { undef } else { acc.set }),
                        attrs.enumerable(),
                        attrs.configurable(),
                    );
                }
                if let Some(value) = crate::array::array_named_property_get(arr, key_str) {
                    let attrs = get_property_attrs(obj as usize, name)
                        .unwrap_or(PropertyAttrs::new(true, true, true));
                    return build_data_descriptor(
                        value,
                        attrs.writable(),
                        attrs.enumerable(),
                        attrs.configurable(),
                    );
                }
                if name == "constructor" {
                    let ctor = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
                    if ctor_value.is_pointer() {
                        let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
                        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
                        if crate::value::js_nanbox_get_pointer(proto) as usize == obj as usize {
                            return build_data_descriptor(ctor, true, false, true);
                        }
                    }
                }
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let (Some(module_name), Some(key_name)) =
                (read_native_module_name(obj), key_rust.as_deref())
            {
                if native_module_has_enumerable_key(&module_name, key_name) {
                    if module_name == "fs" {
                        match key_name {
                            "ReadStream" | "WriteStream" | "FileReadStream" | "FileWriteStream"
                            | "Utf8Stream" => {
                                let get =
                                    super::native_module::fs_namespace_descriptor_getter_value(
                                        key_name,
                                    );
                                let set = if key_name == "Utf8Stream" {
                                    f64::from_bits(crate::value::TAG_UNDEFINED)
                                } else {
                                    super::native_module::fs_namespace_descriptor_setter_value(
                                        key_name,
                                    )
                                };
                                return build_accessor_descriptor(get, set, true, true);
                            }
                            "promises" => {
                                let get =
                                    super::native_module::fs_namespace_descriptor_getter_value(
                                        key_name,
                                    );
                                return build_accessor_descriptor(
                                    get,
                                    f64::from_bits(crate::value::TAG_UNDEFINED),
                                    true,
                                    true,
                                );
                            }
                            "constants" => {
                                let value = js_object_get_field_by_name(obj, key_str);
                                return build_data_descriptor(
                                    f64::from_bits(value.bits()),
                                    false,
                                    true,
                                    false,
                                );
                            }
                            _ => {}
                        }
                    }
                    let value = js_object_get_field_by_name(obj, key_str);
                    if matches!(
                        module_name.as_str(),
                        "process" | "process.namespace" | "process.default"
                    ) && key_name == "permission"
                    {
                        let value = crate::process::process_metadata_property("permission")
                            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                        return build_data_descriptor(value, false, true, false);
                    }
                    return build_data_descriptor(f64::from_bits(value.bits()), true, true, true);
                }
            }
        }

        // A declared class's materialized `.prototype` object: instance
        // accessors (`get x(){}`) live in the class vtable, not the object's
        // fields, but they ARE own properties of the prototype.
        if let Some(cid) = super::class_registry::class_id_for_decl_prototype_object(obj as usize) {
            if let Some(ref name) = key_rust {
                if super::class_registry::class_is_key_deleted(cid, name) {
                    // `delete C.prototype.x` recorded the accessor as removed.
                } else if let Some((g, s)) =
                    super::class_registry::class_own_accessor_ptrs(cid, name)
                {
                    return build_accessor_descriptor(
                        super::class_registry::class_accessor_function_value(g, false, name),
                        super::class_registry::class_accessor_function_value(s, true, name),
                        false,
                        true,
                    );
                }
            }
        }

        // Check whether the key is actually present on the object. A property can
        // legitimately hold `undefined`, and accessor descriptors have no value slot,
        // so we check the keys_array directly instead of relying on "value != undefined".
        let present = own_key_present(obj, key_str);
        if !present {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Look up descriptor flags (default: all true).
        let attrs = key_rust
            .as_ref()
            .and_then(|k| get_property_attrs(obj as usize, k))
            .unwrap_or(PropertyAttrs::new(true, true, true));
        let bool_to_f64 = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });

        // Accessor descriptor path.
        if let Some(acc) = key_rust
            .as_ref()
            .and_then(|k| get_accessor_descriptor(obj as usize, k))
        {
            let packed = b"get\0set\0enumerable\0configurable";
            let desc =
                js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
            let header_size = std::mem::size_of::<ObjectHeader>();
            let fields = (desc as *mut u8).add(header_size) as *mut f64;
            // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
            *fields = if acc.get != 0 {
                f64::from_bits(acc.get)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(1) = if acc.set != 0 {
                f64::from_bits(acc.set)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            // GC_STORE_AUDIT(INIT): descriptor boolean fields are pointer-free and layout is rebuilt below.
            *fields.add(2) = bool_to_f64(attrs.enumerable());
            *fields.add(3) = bool_to_f64(attrs.configurable());
            super::rebuild_object_field_layout(desc, 4);
            return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
        }

        // Data descriptor path.
        let value = js_object_get_field_by_name(obj, key_str);
        let packed = b"value\0writable\0enumerable\0configurable";
        let desc = js_object_alloc_with_shape(
            0x0D_E5_C0, // unique shape_id for property descriptors
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let header_size = std::mem::size_of::<ObjectHeader>();
        let fields = (desc as *mut u8).add(header_size) as *mut f64;
        // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
        *fields = f64::from_bits(value.bits()); // value
        *fields.add(1) = bool_to_f64(attrs.writable()); // writable
        *fields.add(2) = bool_to_f64(attrs.enumerable()); // enumerable
        *fields.add(3) = bool_to_f64(attrs.configurable()); // configurable
        super::rebuild_object_field_layout(desc, 4);
        f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Build a `{ value, writable, enumerable, configurable }` data descriptor
/// object. Shared by the string-primitive descriptor path (#2818).
pub(crate) unsafe fn build_data_descriptor(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bf = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    let packed = b"value\0writable\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_E5_C0, 4, packed.as_ptr(), packed.len() as u32);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let fields = (desc as *mut u8).add(header_size) as *mut f64;
    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
    *fields = value;
    *fields.add(1) = bf(writable);
    *fields.add(2) = bf(enumerable);
    *fields.add(3) = bf(configurable);
    super::rebuild_object_field_layout(desc, 4);
    f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
}

pub(crate) unsafe fn build_accessor_descriptor(
    get: f64,
    set: f64,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bf = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    let packed = b"get\0set\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let fields = (desc as *mut u8).add(header_size) as *mut f64;
    // GC_STORE_AUDIT(INIT): descriptor object is freshly allocated; layout is rebuilt before publication.
    *fields = get;
    *fields.add(1) = set;
    *fields.add(2) = bf(enumerable);
    *fields.add(3) = bf(configurable);
    super::rebuild_object_field_layout(desc, 4);
    f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
}

/// #2818: own-property descriptor for a string primitive receiver. Index keys
/// in range yield the single-char value descriptor (writable:false,
/// enumerable:true, configurable:false); "length" yields the length value
/// descriptor (writable:false, enumerable:false, configurable:false). Any
/// other key is absent → undefined.
unsafe fn string_primitive_descriptor(str_value: f64, key_value: f64) -> f64 {
    let key_str = crate::builtins::js_string_coerce(key_value);
    if key_str.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (sptr, sblen) = match crate::string::str_bytes_from_jsvalue(str_value, &mut scratch) {
        Some((p, b)) if !p.is_null() => (p, b),
        _ => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let utf16_len = crate::string::compute_utf16_len(sptr, sblen);

    if name == "length" {
        return build_data_descriptor(utf16_len as f64, false, false, false);
    }

    if let Some(index) = super::canonical_array_index(name) {
        if index < utf16_len {
            // Materialize the single UTF-16 unit at `index` as a 1-char string.
            let bytes = std::slice::from_raw_parts(sptr, sblen as usize);
            let s = std::str::from_utf8(bytes).unwrap_or("");
            if let Some(ch) = s.chars().nth(index as usize) {
                let mut buf = [0u8; 4];
                let cs = ch.encode_utf8(&mut buf);
                let cstr = crate::string::js_string_from_bytes(cs.as_ptr(), cs.len() as u32);
                let char_val = f64::from_bits(JSValue::string_ptr(cstr).bits());
                return build_data_descriptor(char_val, false, true, false);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Object.getOwnPropertyNames(obj) — returns all own property names (including non-enumerable).
/// Takes a NaN-boxed f64 object pointer, returns a NaN-boxed f64 array pointer.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_names(obj_value: f64) -> f64 {
    unsafe {
        // #2818: ToObject(null/undefined) throws TypeError, matching Node.
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }
        // A Proxy is a small registered id, not a heap object — route it to the
        // `ownKeys` trap (string subset) before the handle-dispatch fallback,
        // which would mis-read the fake pointer and return an empty array.
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            return crate::proxy::proxy_own_property_names(obj_value);
        }
        if obj_jv.is_pointer() {
            let raw = crate::value::js_nanbox_get_pointer(obj_value) as usize;
            if crate::value::addr_class::is_small_handle(raw) {
                if let Some(dispatch) = super::class_registry::handle_own_property_names_dispatch()
                {
                    let names = dispatch(raw as i64);
                    if names.to_bits() != crate::value::TAG_UNDEFINED {
                        return names;
                    }
                }
                let empty = crate::array::js_array_alloc(0);
                return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
            }
        }
        // #5268: a native-module namespace/default object (`fs`, `path`, …)
        // must enumerate its export surface here, not the internal
        // `__module__` sentinel that the generic field walk would return.
        // graceful-fs's `clone.js` does
        // `getOwnPropertyNames(fs).forEach(k => defineProperty(copy, k,
        // getOwnPropertyDescriptor(fs, k)))`; with only `__module__` listed,
        // the clone dropped every fs method (`readFileSync` → undefined).
        // Mirror `Object.keys` (vt_own_keys_array → native_module_enumerable_keys).
        if obj_jv.is_pointer() {
            let obj_ptr = crate::value::js_nanbox_get_pointer(obj_value) as *const ObjectHeader;
            if !obj_ptr.is_null() {
                if let Some(arr) = super::native_module::vt_own_keys_array(obj_ptr) {
                    return f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000);
                }
            }
        }
        if let Some(str_value) = boxed_string_payload(obj_value) {
            return boxed_string_own_property_names(obj_value, str_value);
        }
        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let result = crate::typedarray_props::typed_array_own_property_names(
                addr as *const crate::typedarray::TypedArrayHeader,
                false,
            );
            return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
        }
        // Date / RegExp / Error exotic instances: expando keys (including
        // non-enumerable ones) + per-kind builtin own slots.
        if let Some((addr, kind)) = super::exotic_expando::exotic_expando_kind_of_value(obj_value) {
            use super::exotic_expando::ExoticKind;
            let mut names = match kind {
                ExoticKind::RegExp => vec!["lastIndex".to_string()],
                ExoticKind::Error => vec!["message".to_string(), "stack".to_string()],
                ExoticKind::Date | ExoticKind::Temporal | ExoticKind::Promise => Vec::new(),
            };
            for key in super::exotic_expando::exotic_own_keys(kind, addr, false) {
                if !names.contains(&key) {
                    names.push(key);
                }
            }
            let arr = crate::array::js_array_alloc(names.len().max(1) as u32);
            let mut out = arr;
            for name in names {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                out = crate::array::js_array_push(out, crate::value::JSValue::string_ptr(key));
            }
            return f64::from_bits((out as u64) | 0x7FFD_0000_0000_0000);
        }
        if let Some(class_id) = class_ref_id(obj_value) {
            let is_prototype_ref = super::class_prototype_ref_id(obj_value).is_some();
            let mut names: Vec<String> = if is_prototype_ref {
                vec!["constructor".to_string()]
            } else {
                vec![
                    "length".to_string(),
                    "name".to_string(),
                    "prototype".to_string(),
                ]
            };
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(reg) = registry.as_ref() {
                    if let Some(vtable) = reg.get(&class_id) {
                        if is_prototype_ref {
                            let mut method_names: Vec<String> =
                                vtable.methods.keys().cloned().collect();
                            method_names.sort();
                            for name in method_names {
                                push_unique_name(&mut names, name);
                            }
                            let mut getter_names: Vec<String> =
                                vtable.getters.keys().cloned().collect();
                            getter_names.sort();
                            for name in getter_names {
                                push_unique_name(&mut names, name);
                            }
                            let mut setter_names: Vec<String> =
                                vtable.setters.keys().cloned().collect();
                            setter_names.sort();
                            for name in setter_names {
                                push_unique_name(&mut names, name.clone());
                            }
                        }
                    }
                }
            }
            if !is_prototype_ref {
                if let Ok(static_methods) = CLASS_STATIC_METHODS.read() {
                    if let Some(map) = static_methods.as_ref().and_then(|m| m.get(&class_id)) {
                        let mut method_names: Vec<String> = map.keys().cloned().collect();
                        method_names.sort();
                        for name in method_names {
                            push_unique_name(&mut names, name);
                        }
                    }
                }
                if let Ok(static_accessors) = CLASS_STATIC_ACCESSORS.read() {
                    if let Some(map) = static_accessors.as_ref().and_then(|m| m.get(&class_id)) {
                        let mut accessor_names: Vec<String> = map.keys().cloned().collect();
                        accessor_names.sort();
                        for name in accessor_names {
                            push_unique_name(&mut names, name);
                        }
                    }
                }
                CLASS_DYNAMIC_PROPS.with(|m| {
                    if let Some(props) = m.borrow().get(&class_id) {
                        let mut prop_names: Vec<String> = props.keys().cloned().collect();
                        prop_names.sort();
                        for name in prop_names {
                            push_unique_name(&mut names, name);
                        }
                    }
                });
            }
            // Private elements (`#x`) live on the static side / prototype
            // vtable under `#`-prefixed keys but are never reflectable own
            // properties of `C` or `C.prototype`.
            names.retain(|n| !n.starts_with('#'));
            sort_property_names_ecma(&mut names);
            let result = crate::array::js_array_alloc(names.len() as u32);
            for name in names {
                let str_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::array::js_array_push(result, JSValue::string_ptr(str_ptr));
            }
            return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
        }

        // String / array values have no `ObjectHeader.keys_array`; their own
        // property names are the index names `"0".."len-1"` plus `"length"`.
        // Reading a bogus `keys_array` off their header segfaulted (#800).
        {
            const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
            let jv = JSValue::from_bits(obj_value.to_bits());
            let n: Option<u32> = if jv.is_any_string() {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                match crate::string::str_bytes_from_jsvalue(obj_value, &mut scratch) {
                    Some((p, blen)) if !p.is_null() => {
                        Some(crate::string::compute_utf16_len(p, blen))
                    }
                    _ => Some(0),
                }
            } else if crate::array::js_array_is_array(obj_value).to_bits() == TAG_TRUE_BITS {
                let ap = extract_obj_ptr(obj_value) as *const crate::array::ArrayHeader;
                Some(crate::array::js_array_length(ap))
            } else {
                None
            };
            if let Some(n) = n {
                let result = crate::array::js_array_alloc(n + 1);
                if crate::array::js_array_is_array(obj_value).to_bits() == TAG_TRUE_BITS {
                    let ap = extract_obj_ptr(obj_value) as *const crate::array::ArrayHeader;
                    for i in 0..n {
                        if super::has_own_helpers::array_own_key_present(ap, {
                            let s = i.to_string();
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
                        }) {
                            let s = i.to_string();
                            let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                            crate::array::js_array_push(result, JSValue::string_ptr(k));
                        }
                    }
                    let lk = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                    crate::array::js_array_push(result, JSValue::string_ptr(lk));
                    let named = crate::array::array_named_property_names(ap, false);
                    for name in &named {
                        let k =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        crate::array::js_array_push(result, JSValue::string_ptr(k));
                    }
                    // Accessor-only named properties (defineProperty {get/set})
                    // are own keys too (gOPN includes non-enumerable).
                    if super::descriptors_in_use() {
                        for name in super::accessor_descriptor_keys_for_obj(ap as usize) {
                            if super::canonical_array_index(&name).is_some()
                                || named.contains(&name)
                            {
                                continue;
                            }
                            let k = crate::string::js_string_from_bytes(
                                name.as_ptr(),
                                name.len() as u32,
                            );
                            crate::array::js_array_push(result, JSValue::string_ptr(k));
                        }
                    }
                } else {
                    for i in 0..n {
                        let s = i.to_string();
                        let k = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        crate::array::js_array_push(result, JSValue::string_ptr(k));
                    }
                    let lk = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                    crate::array::js_array_push(result, JSValue::string_ptr(lk));
                }
                return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
            }
        }

        // #3655: functions/closures. Own keys are `length`, `name`, then any
        // user-attached props, then `prototype` (constructors) — matching V8's
        // ordering. All honor `delete`. Reading `keys_array` off a closure
        // (below) would be out of bounds.
        if obj_jv.is_pointer() {
            let ptr = crate::value::js_nanbox_get_pointer(obj_value) as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let mut names: Vec<String> = Vec::new();
                if !crate::closure::closure_is_key_deleted(ptr, "length") {
                    names.push("length".to_string());
                }
                if !crate::closure::closure_is_key_deleted(ptr, "name") {
                    names.push("name".to_string());
                }
                let has_prototype = crate::closure::closure_has_own_dynamic_prop(ptr, "prototype")
                    && !crate::closure::closure_is_key_deleted(ptr, "prototype");
                // User-attached props (snapshot is already sorted); the
                // built-in slots are emitted explicitly so skip them here.
                for (name, _) in crate::closure::closure_dynamic_props_snapshot(ptr) {
                    if matches!(name.as_str(), "length" | "name" | "prototype") {
                        continue;
                    }
                    if crate::closure::closure_is_key_deleted(ptr, &name) {
                        continue;
                    }
                    names.push(name);
                }
                for name in super::accessor_descriptor_keys_for_obj(ptr) {
                    if matches!(name.as_str(), "length" | "name" | "prototype") {
                        continue;
                    }
                    if crate::closure::closure_is_key_deleted(ptr, &name) {
                        continue;
                    }
                    push_unique_name(&mut names, name);
                }
                if has_prototype {
                    names.push("prototype".to_string());
                }
                let result = crate::array::js_array_alloc(names.len() as u32);
                for name in names {
                    let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    crate::array::js_array_push(result, JSValue::string_ptr(s));
                }
                return f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000);
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        // A heap value that isn't a plain ordinary object (Date `DateCell`,
        // RegExp, Map/Set, Promise, …) has no `ObjectHeader.keys_array` — reading
        // one off its header dereferences garbage and segfaults. `Object.create({},
        // new Date(0))` / `Object.defineProperties(obj, new RegExp())` reach here
        // with such a value. Perry doesn't model expando properties on these
        // exotic objects, so report no own keys rather than crashing.
        if !is_valid_obj_ptr(obj as *const u8) {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        {
            let gc =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
                let empty = crate::array::js_array_alloc(0);
                return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
            }
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        // Clone the keys array — Object.getOwnPropertyNames includes ALL keys (even non-enumerable).
        let len = crate::array::js_array_length(keys) as usize;
        let order = ecma_own_key_order(keys);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        // Private elements (`#x`) live in a class instance's keys_array but are
        // never reflectable own properties. Drop them for class instances
        // (class_id != 0); plain `{"#fff": 1}` literals keep class_id 0.
        let hide_private = (*obj).class_id != 0;
        let result = crate::array::js_array_alloc(len as u32);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, pos(i));
            if hide_private {
                if let Some(b) = crate::string::js_string_key_bytes(key_val, &mut sso_buf) {
                    if b.first() == Some(&b'#')
                        || super::field_get_set::is_internal_runtime_key_bytes(b)
                    {
                        continue;
                    }
                }
            }
            crate::array::js_array_push_f64(result, f64::from_bits(key_val.bits()));
        }
        f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.getOwnPropertyDescriptors(obj) — returns a new object whose own
/// property keys (the same set `Object.getOwnPropertyNames` reports, including
/// non-enumerable keys and class-ref method names) each map to the property
/// descriptor produced by `js_object_get_own_property_descriptor`. Spec:
/// "for each own property key K of O, set result[K] = descriptor(O, K)".
///
/// effect's `SchemaAST.annotations` builds a fresh AST node via
/// `Object.create(Object.getPrototypeOf(ast), Object.getOwnPropertyDescriptors(ast))`,
/// so without this the plural call lowered to a null callee and Schema.ts
/// module init threw `TypeError: value is not a function` (#1791/#1758).
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptors(obj_value: f64) -> f64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    unsafe {
        // Enumerate own keys exactly like Object.getOwnPropertyNames — this
        // handles class refs and plain objects, and includes non-enumerable
        // keys, matching the spec's [[OwnPropertyKeys]] string-key set.
        let names_value = js_object_get_own_property_names(obj_value);
        let names_arr =
            crate::value::js_nanbox_get_pointer(names_value) as *const crate::array::ArrayHeader;

        // Fresh result object that collects { key: descriptor } entries.
        // Like js_object_entries / js_object_get_own_property_names above, the
        // intermediate allocations aren't rooted — Perry's builder helpers
        // follow this convention.
        let result = js_object_alloc(0, 0);

        if !names_arr.is_null() {
            let len = crate::array::js_array_length(names_arr) as usize;
            for i in 0..len {
                let key_val = crate::array::js_array_get(names_arr, i as u32);
                let key_f64 = f64::from_bits(key_val.bits());
                let desc = js_object_get_own_property_descriptor(obj_value, key_f64);
                // Spec step: only add the entry when the descriptor is not
                // undefined (the key was removed between key-collection and the
                // descriptor read, e.g. by a Proxy trap).
                if desc.to_bits() == crate::value::TAG_UNDEFINED {
                    continue;
                }
                let key_str = crate::builtins::js_string_coerce(key_f64);
                if !key_str.is_null() {
                    js_object_set_field_by_name(result, key_str, desc);
                }
            }
        }

        // [[OwnPropertyKeys]] includes symbol keys after the string keys, and
        // `Object.getOwnPropertyDescriptors` must report a descriptor for each
        // (including non-enumerable ones). `getOwnPropertyNames` above only
        // covers the string subset, so enumerate the symbol keys separately and
        // install each descriptor under its symbol key on the result object.
        // (test262 getOwnPropertyDescriptors/symbols-included, order-after-*.)
        let result_value = f64::from_bits((result as u64) | POINTER_TAG);
        let sym_arr_raw = crate::symbol::js_object_get_own_property_symbols(obj_value);
        if sym_arr_raw != 0 {
            let sym_arr = sym_arr_raw as *const crate::array::ArrayHeader;
            if !sym_arr.is_null() {
                let slen = crate::array::js_array_length(sym_arr) as usize;
                for i in 0..slen {
                    let sym_val = crate::array::js_array_get(sym_arr, i as u32);
                    let sym_f64 = f64::from_bits(sym_val.bits());
                    let desc = js_object_get_own_property_descriptor(obj_value, sym_f64);
                    if desc.to_bits() == crate::value::TAG_UNDEFINED {
                        continue;
                    }
                    crate::symbol::js_object_set_symbol_property(result_value, sym_f64, desc);
                }
            }
        }
        result_value
    }
}

/// Object.create(proto[, propertiesObject]) — create an object with the given
/// prototype and (optionally) define properties from a descriptor bag.
///
/// `props_value` is the (NaN-boxed) properties object, or `undefined` when the
/// caller passed only one argument. #2816: the prototype argument must be an
/// object or `null`; primitives / `undefined` throw
/// `TypeError: Object prototype may only be an Object or null`.
#[no_mangle]
pub extern "C" fn js_object_create_with_props(proto_value: f64, props_value: f64) -> f64 {
    // #2816 prototype validation: only an object or `null` is permitted. A
    // Symbol is pointer-tagged but not an object, so reject it explicitly.
    let proto_jv = crate::value::JSValue::from_bits(proto_value.to_bits());
    let proto_is_symbol = unsafe { crate::symbol::js_is_symbol(proto_value) != 0 };
    let proto_ok = proto_jv.is_null()
        || crate::proxy::js_proxy_is_proxy(proto_value) != 0
        || (!proto_is_symbol
            && (unsafe { value_is_object_like(proto_value) }
                || super::class_ref_id(proto_value).is_some()));
    if !proto_ok {
        // V8 renders the offending value: `... an Object or null: 5`.
        let rendered = unsafe { describe_value_for_type_error(proto_value) };
        throw_object_type_error_with_suffix(
            "Object prototype may only be an Object or null: ",
            &rendered,
        );
    }

    let result = js_object_create(proto_value);

    // #2816: apply the descriptor bag, if one was supplied.
    let props_jv = crate::value::JSValue::from_bits(props_value.to_bits());
    if !props_jv.is_undefined() {
        return js_object_define_properties(result, props_value);
    }
    result
}

#[used]
static KEEP_OBJECT_CREATE_WITH_PROPS: extern "C" fn(f64, f64) -> f64 = js_object_create_with_props;
