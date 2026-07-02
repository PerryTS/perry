use super::super::*;
use super::*;
// Math.* thunks live in the sibling `builtin_thunks` module (split out of
// `global_this`); pull them in directly so `install_math_namespace` resolves
// `math_*_thunk` without routing through the trunk re-exports.
use super::builtin_thunks::*;

pub(crate) fn install_math_namespace(ns_obj: *mut ObjectHeader) {
    if ns_obj.is_null() {
        return;
    }
    for (name, func_ptr, arity) in [
        ("abs", math_abs_thunk as *const u8, 1),
        ("acos", math_acos_thunk as *const u8, 1),
        ("acosh", math_acosh_thunk as *const u8, 1),
        ("asin", math_asin_thunk as *const u8, 1),
        ("asinh", math_asinh_thunk as *const u8, 1),
        ("atan", math_atan_thunk as *const u8, 1),
        ("atanh", math_atanh_thunk as *const u8, 1),
        ("atan2", math_atan2_thunk as *const u8, 2),
        ("ceil", math_ceil_thunk as *const u8, 1),
        ("cbrt", math_cbrt_thunk as *const u8, 1),
        ("expm1", math_expm1_thunk as *const u8, 1),
        ("clz32", math_clz32_thunk as *const u8, 1),
        ("cos", math_cos_thunk as *const u8, 1),
        ("cosh", math_cosh_thunk as *const u8, 1),
        ("exp", math_exp_thunk as *const u8, 1),
        ("floor", math_floor_thunk as *const u8, 1),
        ("fround", math_fround_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
    install_proto_method_rest_with_length(ns_obj, "hypot", math_hypot_thunk as *const u8, 2, 0);
    for (name, func_ptr, arity) in [
        ("imul", math_imul_thunk as *const u8, 2),
        ("log", math_log_thunk as *const u8, 1),
        ("log1p", math_log1p_thunk as *const u8, 1),
        ("log2", math_log2_thunk as *const u8, 1),
        ("log10", math_log10_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
    install_proto_method_rest_with_length(ns_obj, "max", math_max_thunk as *const u8, 2, 0);
    install_proto_method_rest_with_length(ns_obj, "min", math_min_thunk as *const u8, 2, 0);
    for (name, func_ptr, arity) in [
        ("pow", math_pow_thunk as *const u8, 2),
        ("random", math_random_thunk as *const u8, 0),
        ("round", math_round_thunk as *const u8, 1),
        ("sign", math_sign_thunk as *const u8, 1),
        ("sin", math_sin_thunk as *const u8, 1),
        ("sinh", math_sinh_thunk as *const u8, 1),
        ("sqrt", math_sqrt_thunk as *const u8, 1),
        ("tan", math_tan_thunk as *const u8, 1),
        ("tanh", math_tanh_thunk as *const u8, 1),
        ("trunc", math_trunc_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }

    let constant_attrs = super::super::PropertyAttrs::new(false, false, false);
    for (name, value) in [
        ("E", std::f64::consts::E),
        ("LN10", std::f64::consts::LN_10),
        ("LN2", std::f64::consts::LN_2),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("PI", std::f64::consts::PI),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ] {
        set_intrinsic_data_prop(ns_obj, name, value, constant_attrs);
    }

    install_proto_method(ns_obj, "f16round", math_f16round_thunk as *const u8, 1);
}

// ---- TC39 Temporal namespace (#4686) -------------------------------------
//
// Each `Temporal.<Type>` constructor is a constructable native closure hung off
// the `Temporal` namespace object. `new Temporal.Duration(...)` resolves the
// closure via a normal property read, then `js_new_function_construct` invokes
// it; the thunk allocates a Temporal cell and returns it, which overrides the
// empty default `this` (see `constructor_return_overrides_this`). Statics
// (`from`, `compare`) are installed on the constructor closure with call-arity
// 0 so every argument lands in the rest array the thunk reads.

#[cfg(feature = "temporal")]
extern "C" fn temporal_duration_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_duration_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_duration_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_instant_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_instant_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_instant_from_epoch_ms_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_epoch_milliseconds_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_instant_from_epoch_ns_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_epoch_nanoseconds_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_instant_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_date_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_year_month_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_year_month_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_year_month_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::compare_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_month_day_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_month_day::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_plain_month_day_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_month_day::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_zoned_date_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::construct(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_zoned_date_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::from_static(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_zoned_date_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::compare_static(&global_this_rest_array_values(rest))
}

// Temporal.Now is a namespace (not a constructor) — method thunks on a plain
// object, installed like Math. Each reads the host clock fresh.
#[cfg(feature = "temporal")]
extern "C" fn temporal_now_instant_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::instant(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_now_timezone_id_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::time_zone_id(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_now_plain_date_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_date_time_iso(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_now_plain_date_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_date_iso(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_now_plain_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_time_iso(&global_this_rest_array_values(rest))
}

#[cfg(feature = "temporal")]
extern "C" fn temporal_now_zoned_date_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::zoned_date_time_iso(&global_this_rest_array_values(rest))
}

/// Build the `Temporal.Now` namespace object (a plain object of method thunks).
#[cfg(feature = "temporal")]
fn build_temporal_now_namespace() -> f64 {
    let now_obj = js_object_alloc(0, 0);
    if now_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    for (name, thunk, len) in [
        ("instant", temporal_now_instant_thunk as *const u8, 0u32),
        ("timeZoneId", temporal_now_timezone_id_thunk as *const u8, 0),
        (
            "plainDateTimeISO",
            temporal_now_plain_date_time_iso_thunk as *const u8,
            0,
        ),
        (
            "plainDateISO",
            temporal_now_plain_date_iso_thunk as *const u8,
            0,
        ),
        (
            "plainTimeISO",
            temporal_now_plain_time_iso_thunk as *const u8,
            0,
        ),
        (
            "zonedDateTimeISO",
            temporal_now_zoned_date_time_iso_thunk as *const u8,
            0,
        ),
    ] {
        install_proto_method_rest_with_length(now_obj, name, thunk, len, 0);
    }
    set_intrinsic_to_string_tag(now_obj, "Temporal.Now");
    crate::value::js_nanbox_pointer(now_obj as i64)
}

/// Install a constructable `Temporal.<name>` constructor closure on the
/// `Temporal` namespace object and return it so statics can be hung off it.
/// Variadic (all args in the rest array, call-arity 0). Unlike
/// `install_constructor_static`, it does NOT mark the closure non-constructable
/// — `new Temporal.<name>(...)` must dispatch through the generic construct
/// path and use the returned cell.
/// Generic accessor-getter thunk shared by every `Temporal.<Type>.prototype`
/// getter. The property name and expected brand kind are stored on the closure
/// instance (`__tname` / `__tkind`); the receiver comes from `IMPLICIT_THIS`.
/// Throws `TypeError` on a non-Temporal or wrong-brand receiver (the getter
/// `branding.js` tests: `blank.call(undefined)`, `years.call({})`, …).
#[cfg(feature = "temporal")]
extern "C" fn temporal_proto_getter_thunk(closure: *const crate::closure::ClosureHeader) -> f64 {
    let recv = super::super::js_implicit_this_get();
    let cl = closure as usize;
    let kind = crate::closure::closure_get_dynamic_prop(cl, "__tkind");
    let expected = crate::value::JSValue::from_bits(kind.to_bits()).to_number() as u8;
    let name = crate::temporal::dispatch::read_string(crate::closure::closure_get_dynamic_prop(
        cl, "__tname",
    ));
    match crate::temporal::temporal_kind(recv) {
        Some(k) if k as u8 == expected => crate::temporal::dispatch::get_property(recv, &name)
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
        _ => crate::object::throw_object_type_error(
            b"Temporal getter called on an incompatible receiver",
        ),
    }
}

/// Generic method thunk shared by every `Temporal.<Type>.prototype` method.
/// Rest-ABI (fixed arity 0): all args arrive in `rest`. Brand-checks the
/// `IMPLICIT_THIS` receiver, then forwards to the per-type dispatch router —
/// used when a prototype method is invoked through indirection
/// (`Temporal.Duration.prototype.add.call(d, x)`); the normal `d.add(x)` path
/// is the brand arm in `js_native_call_method`.
#[cfg(feature = "temporal")]
extern "C" fn temporal_proto_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let recv = super::super::js_implicit_this_get();
    let cl = closure as usize;
    let kind = crate::closure::closure_get_dynamic_prop(cl, "__tkind");
    let expected = crate::value::JSValue::from_bits(kind.to_bits()).to_number() as u8;
    let name = crate::temporal::dispatch::read_string(crate::closure::closure_get_dynamic_prop(
        cl, "__tname",
    ));
    match crate::temporal::temporal_kind(recv) {
        Some(k) if k as u8 == expected => {
            let args = global_this_rest_array_values(rest);
            crate::temporal::dispatch::call_method(recv, &name, &args)
        }
        _ => crate::object::throw_object_type_error(
            b"Temporal method called on an incompatible receiver",
        ),
    }
}

/// Install a brand-checked accessor getter (`{ get, set: undefined,
/// enumerable: false, configurable: true }`) on a Temporal prototype.
#[cfg(feature = "temporal")]
fn install_temporal_proto_getter(proto: *mut ObjectHeader, kind: u8, name: &str) {
    let c = crate::closure::js_closure_alloc(temporal_proto_getter_thunk as *const u8, 0);
    if c.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(temporal_proto_getter_thunk as *const u8, 0);
    let cl = c as usize;
    crate::closure::closure_set_dynamic_prop(cl, "__tkind", kind as f64);
    crate::closure::closure_set_dynamic_prop(
        cl,
        "__tname",
        crate::temporal::dispatch::string(name),
    );
    super::super::native_module::set_bound_native_closure_name(c, &format!("get {name}"));
    super::super::native_module::set_builtin_closure_length(cl, 0);
    super::super::native_module::set_builtin_closure_non_constructable(cl);
    unsafe {
        install_builtin_getter(
            proto,
            name,
            crate::value::js_nanbox_pointer(c as i64).to_bits(),
        );
    }
}

/// Install a brand-checked method (`{ writable: true, enumerable: false,
/// configurable: true }`, non-constructable, with spec `.name`/`.length`) on a
/// Temporal prototype.
#[cfg(feature = "temporal")]
fn install_temporal_proto_method(proto: *mut ObjectHeader, kind: u8, name: &str, spec_length: u32) {
    let c = crate::closure::js_closure_alloc(temporal_proto_method_thunk as *const u8, 0);
    if c.is_null() {
        return;
    }
    // Rest ABI so every argument is bundled regardless of the shared thunk's
    // fixed signature.
    crate::closure::js_register_closure_rest(temporal_proto_method_thunk as *const u8, 0);
    let cl = c as usize;
    crate::closure::closure_set_dynamic_prop(cl, "__tkind", kind as f64);
    crate::closure::closure_set_dynamic_prop(
        cl,
        "__tname",
        crate::temporal::dispatch::string(name),
    );
    super::super::native_module::set_bound_native_closure_name(c, name);
    super::super::native_module::set_builtin_closure_length(cl, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(cl);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, crate::value::js_nanbox_pointer(c as i64));
    super::super::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    super::super::set_builtin_property_attrs(
        cl,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        cl,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
}

/// Build and wire a `Temporal.<Type>.prototype` object: a real object carrying
/// the type's accessor getters and methods (for reflection + indirect `.call`),
/// linked to its constructor via `ctor.prototype` / `proto.constructor`.
#[cfg(feature = "temporal")]
fn install_temporal_prototype(
    ctor: *mut crate::closure::ClosureHeader,
    kind: u8,
    getters: &[&str],
    methods: &[(&str, u32)],
) {
    if ctor.is_null() {
        return;
    }
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }
    for g in getters {
        install_temporal_proto_getter(proto, kind, g);
    }
    for (m, len) in methods {
        install_temporal_proto_method(proto, kind, m, *len);
    }
    // ctor.prototype = proto  ({ writable:false, enumerable:false, configurable:false })
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, proto_key, proto_value);
    super::super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::super::PropertyAttrs::new(false, false, false),
    );
    // proto.constructor = ctor  ({ writable:true, enumerable:false, configurable:true })
    let ctor_value = crate::value::js_nanbox_pointer(ctor as i64);
    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(proto, ctor_key, ctor_value);
    super::super::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

#[cfg(feature = "temporal")]
fn install_temporal_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    spec_length: u32,
) -> *mut crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return std::ptr::null_mut();
    }
    crate::closure::js_register_closure_rest(func_ptr, 0);
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::super::set_builtin_property_attrs(
        ns_obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    closure
}

/// Read a built-in closure's installed `name` dynamic prop as a Rust `String`
/// (used by the shared Temporal prototype thunks to recover which getter /
/// method they back). Empty string if absent.
#[cfg(feature = "temporal")]
fn temporal_closure_name(closure: *const crate::closure::ClosureHeader) -> String {
    let v = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
    if !JSValue::from_bits(v.to_bits()).is_string() {
        return String::new();
    }
    let ptr = crate::value::js_get_string_pointer_unified(v) as *const crate::string::StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Throw `TypeError: <type>.prototype.<member> called on incompatible receiver`
/// for a Temporal prototype getter / method invoked on a non-branded `this`
/// (the spec brand check). Used by the reflective `.call`/`.apply` paths;
/// normal `zdt.foo()` dispatches via the brand arm and never reaches here.
#[cfg(feature = "temporal")]
fn temporal_brand_type_error(type_name: &str, member: &str) -> ! {
    crate::object::throw_object_type_error(
        format!("{type_name}.prototype.{member} called on incompatible receiver").as_bytes(),
    )
}

/// Shared body for a `Temporal.ZonedDateTime.prototype` accessor getter invoked
/// reflectively. Resolves `this` from `IMPLICIT_THIS`, brand-checks it is a
/// `ZonedDateTime`, and returns the getter's value.
#[cfg(feature = "temporal")]
extern "C" fn temporal_zdt_proto_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // The accessor's name is `"get <prop>"`; recover the bare property.
    let name = temporal_closure_name(closure);
    let prop = name.strip_prefix("get ").unwrap_or(&name);
    if crate::temporal::temporal_kind(this) != Some(crate::temporal::TemporalKind::ZonedDateTime) {
        temporal_brand_type_error("Temporal.ZonedDateTime", prop);
    }
    crate::temporal::dispatch::get_property(this, prop)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Shared body for a `Temporal.ZonedDateTime.prototype` method invoked
/// reflectively (`.prototype.equals.call(zdt, …)`). Brand-checks `this` then
/// dispatches to the per-type method router.
#[cfg(feature = "temporal")]
extern "C" fn temporal_zdt_proto_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let name = temporal_closure_name(closure);
    if crate::temporal::temporal_kind(this) != Some(crate::temporal::TemporalKind::ZonedDateTime) {
        temporal_brand_type_error("Temporal.ZonedDateTime", &name);
    }
    crate::temporal::dispatch::call_method(this, &name, &global_this_rest_array_values(rest))
}

/// Install one accessor getter onto a Temporal prototype with the spec
/// descriptor (`enumerable:false, configurable:true`, `set:undefined`) and the
/// proper getter `name` (`"get <prop>"`) / `length` (0). Mirrors the RegExp
/// prototype getter install.
#[cfg(feature = "temporal")]
fn install_temporal_getter(proto: *mut ObjectHeader, prop: &str, func_ptr: *const u8) {
    unsafe {
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::super::native_module::set_bound_native_closure_name(closure, &format!("get {prop}"));
        super::super::native_module::set_builtin_closure_length(closure as usize, 0);
        let key = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
        super::super::object_ops::ensure_key_in_keys_array(proto, key);
        let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        super::super::object_ops::install_builtin_getter(proto, prop, getter_bits);
        super::super::set_accessor_descriptor(
            proto as usize,
            prop.to_string(),
            super::super::AccessorDescriptor {
                get: getter_bits,
                set: 0,
            },
        );
        super::super::set_property_attrs(
            proto as usize,
            prop.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
        super::super::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        super::super::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// Build the `Temporal.ZonedDateTime.prototype` object: every getter as an
/// accessor property + every method as a non-constructable built-in function,
/// each with the spec `name`/`length`/descriptor, plus `[Symbol.toStringTag]`.
/// These satisfy the reflective test262 cases (branding / prop-desc / length /
/// name / not-a-constructor / builtin); ordinary `zdt.foo()` calls still
/// dispatch via the Temporal brand arm and never touch this object.
#[cfg(feature = "temporal")]
fn build_zoned_date_time_prototype() -> *mut ObjectHeader {
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }
    const GETTERS: &[&str] = &[
        "year",
        "month",
        "monthCode",
        "day",
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
        "era",
        "eraYear",
        "epochMilliseconds",
        "epochNanoseconds",
        "dayOfWeek",
        "dayOfYear",
        "weekOfYear",
        "yearOfWeek",
        "daysInWeek",
        "daysInMonth",
        "daysInYear",
        "monthsInYear",
        "inLeapYear",
        "hoursInDay",
        "offset",
        "offsetNanoseconds",
        "timeZoneId",
        "calendarId",
    ];
    for g in GETTERS {
        install_temporal_getter(proto, g, temporal_zdt_proto_getter_thunk as *const u8);
    }
    // (name, spec_length)
    const METHODS: &[(&str, u32)] = &[
        ("add", 1),
        ("subtract", 1),
        ("until", 1),
        ("since", 1),
        ("round", 1),
        ("equals", 1),
        ("with", 1),
        ("withCalendar", 1),
        ("withPlainTime", 0),
        ("withTimeZone", 1),
        ("toInstant", 0),
        ("toPlainDate", 0),
        ("toPlainTime", 0),
        ("toPlainDateTime", 0),
        ("toString", 0),
        ("toJSON", 0),
        ("toLocaleString", 0),
        ("valueOf", 0),
        ("startOfDay", 0),
        ("getTimeZoneTransition", 0),
    ];
    for (name, len) in METHODS {
        install_proto_method_rest_with_length(
            proto,
            name,
            temporal_zdt_proto_method_thunk as *const u8,
            *len,
            0,
        );
    }
    set_intrinsic_to_string_tag(proto, "Temporal.ZonedDateTime");
    proto
}

/// Map a value to the [`TemporalKind`] it constructs *iff* it is one of the
/// eight `Temporal.<X>` constructor closures (matched by func-ptr, so a
/// same-named user closure never matches). Used by `instanceof` to make
/// `zdt instanceof Temporal.ZonedDateTime` resolve to `true` even though
/// Temporal values dispatch via brand arms, not a real prototype chain.
#[cfg(feature = "temporal")]
pub(crate) fn temporal_ctor_kind(type_ref: f64) -> Option<crate::temporal::TemporalKind> {
    use crate::temporal::TemporalKind;
    let jv = JSValue::from_bits(type_ref.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let closure = jv.as_pointer::<crate::closure::ClosureHeader>();
    if closure.is_null() {
        return None;
    }
    let (tag, fp) = unsafe { ((*closure).type_tag, (*closure).func_ptr) };
    if tag != crate::closure::CLOSURE_MAGIC {
        return None;
    }
    let fp = fp as usize;
    let table: [(*const u8, TemporalKind); 8] = [
        (
            temporal_duration_ctor_thunk as *const u8,
            TemporalKind::Duration,
        ),
        (
            temporal_instant_ctor_thunk as *const u8,
            TemporalKind::Instant,
        ),
        (
            temporal_plain_date_ctor_thunk as *const u8,
            TemporalKind::PlainDate,
        ),
        (
            temporal_plain_time_ctor_thunk as *const u8,
            TemporalKind::PlainTime,
        ),
        (
            temporal_plain_date_time_ctor_thunk as *const u8,
            TemporalKind::PlainDateTime,
        ),
        (
            temporal_plain_year_month_ctor_thunk as *const u8,
            TemporalKind::PlainYearMonth,
        ),
        (
            temporal_plain_month_day_ctor_thunk as *const u8,
            TemporalKind::PlainMonthDay,
        ),
        (
            temporal_zoned_date_time_ctor_thunk as *const u8,
            TemporalKind::ZonedDateTime,
        ),
    ];
    table
        .iter()
        .find(|(ptr, _)| *ptr as usize == fp)
        .map(|(_, k)| *k)
}

/// Temporal gated off: no Temporal constructor exists, so nothing is ever a
/// Temporal constructor. Kept compiled because `instanceof` / class-registry
/// dispatch (always linked) call it.
#[cfg(not(feature = "temporal"))]
pub(crate) fn temporal_ctor_kind(_type_ref: f64) -> Option<crate::temporal::TemporalKind> {
    None
}

/// Resolve `Temporal.<kind>.prototype` for a Temporal value's `kind` by
/// navigating the live `globalThis.Temporal.<Name>.prototype` chain (the
/// prototype object is stamped on each constructor closure's `prototype`
/// dynamic prop at namespace-install time). Returns `undefined` if the
/// namespace isn't reachable. Used by `Object.getPrototypeOf` on a Temporal
/// cell — which has no `[[Prototype]]` link of its own (it dispatches via brand
/// arms) but whose reflective prototype IS `Temporal.<Type>.prototype`. (#5587)
#[cfg(feature = "temporal")]
pub(crate) fn temporal_kind_prototype(kind: crate::temporal::TemporalKind) -> f64 {
    use crate::temporal::TemporalKind::*;
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let name: &[u8] = match kind {
        Duration => b"Duration",
        Instant => b"Instant",
        PlainDate => b"PlainDate",
        PlainTime => b"PlainTime",
        PlainDateTime => b"PlainDateTime",
        PlainYearMonth => b"PlainYearMonth",
        PlainMonthDay => b"PlainMonthDay",
        ZonedDateTime => b"ZonedDateTime",
    };
    let g = super::js_get_global_this();
    let gp = (g.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    if gp.is_null() {
        return undef;
    }
    let tkey = crate::string::js_string_from_bytes(b"Temporal".as_ptr(), 8);
    let temporal = js_object_get_field_by_name(gp, tkey);
    if !temporal.is_pointer() {
        return undef;
    }
    let tp = (temporal.bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    let ckey = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let ctor = js_object_get_field_by_name(tp, ckey);
    if !ctor.is_pointer() {
        return undef;
    }
    let cp = (ctor.bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    let pkey = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let proto = js_object_get_field_by_name(cp, pkey);
    f64::from_bits(proto.bits())
}

/// `Temporal.PlainDate.prototype` accessor getters and method shapes (#4691).
#[cfg(feature = "temporal")]
const PLAIN_DATE_GETTERS: &[&str] = &[
    "calendarId",
    "era",
    "eraYear",
    "year",
    "month",
    "monthCode",
    "day",
    "dayOfWeek",
    "dayOfYear",
    "weekOfYear",
    "yearOfWeek",
    "daysInWeek",
    "daysInMonth",
    "daysInYear",
    "monthsInYear",
    "inLeapYear",
];
#[cfg(feature = "temporal")]
const PLAIN_DATE_METHODS: &[(&str, u32)] = &[
    ("toPlainYearMonth", 0),
    ("toPlainMonthDay", 0),
    ("add", 1),
    ("subtract", 1),
    ("with", 1),
    ("withCalendar", 1),
    ("until", 1),
    ("since", 1),
    ("equals", 1),
    ("toPlainDateTime", 0),
    ("toZonedDateTime", 1),
    ("toString", 0),
    ("toLocaleString", 0),
    ("toJSON", 0),
    ("valueOf", 0),
];

/// `Temporal.PlainDateTime.prototype` accessor getters and method shapes (#4693).
#[cfg(feature = "temporal")]
const PLAIN_DATE_TIME_GETTERS: &[&str] = &[
    "calendarId",
    "era",
    "eraYear",
    "year",
    "month",
    "monthCode",
    "day",
    "dayOfWeek",
    "dayOfYear",
    "weekOfYear",
    "yearOfWeek",
    "daysInWeek",
    "daysInMonth",
    "daysInYear",
    "monthsInYear",
    "inLeapYear",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
#[cfg(feature = "temporal")]
const PLAIN_DATE_TIME_METHODS: &[(&str, u32)] = &[
    ("with", 1),
    ("withPlainTime", 0),
    ("withCalendar", 1),
    ("add", 1),
    ("subtract", 1),
    ("until", 1),
    ("since", 1),
    ("round", 1),
    ("equals", 1),
    ("toString", 0),
    ("toLocaleString", 0),
    ("toJSON", 0),
    ("valueOf", 0),
    ("toZonedDateTime", 1),
    ("toPlainDate", 0),
    ("toPlainTime", 0),
];

#[cfg(feature = "temporal")]
pub(crate) fn install_temporal_namespace(ns_obj: *mut ObjectHeader) {
    if ns_obj.is_null() {
        return;
    }
    // Temporal.Duration (#4688)
    let duration = install_temporal_constructor(
        ns_obj,
        "Duration",
        temporal_duration_ctor_thunk as *const u8,
        0,
    );
    if !duration.is_null() {
        install_constructor_static_with_call_arity(
            duration,
            "from",
            temporal_duration_from_thunk as *const u8,
            1,
            0,
            true,
        );
        install_constructor_static_with_call_arity(
            duration,
            "compare",
            temporal_duration_compare_thunk as *const u8,
            2,
            0,
            true,
        );
        install_temporal_prototype(
            duration,
            crate::temporal::TemporalKind::Duration as u8,
            &[
                "years",
                "months",
                "weeks",
                "days",
                "hours",
                "minutes",
                "seconds",
                "milliseconds",
                "microseconds",
                "nanoseconds",
                "sign",
                "blank",
            ],
            &[
                ("with", 1),
                ("negated", 0),
                ("abs", 0),
                ("add", 1),
                ("subtract", 1),
                ("round", 1),
                ("total", 1),
                ("toString", 0),
                ("toJSON", 0),
                ("toLocaleString", 0),
                ("valueOf", 0),
            ],
        );
    }

    // Temporal.Instant (#4690)
    let instant = install_temporal_constructor(
        ns_obj,
        "Instant",
        temporal_instant_ctor_thunk as *const u8,
        1,
    );
    if !instant.is_null() {
        install_temporal_from_compare(
            instant,
            temporal_instant_from_thunk as *const u8,
            temporal_instant_compare_thunk as *const u8,
        );
        install_constructor_static_with_call_arity(
            instant,
            "fromEpochMilliseconds",
            temporal_instant_from_epoch_ms_thunk as *const u8,
            1,
            0,
            true,
        );
        install_constructor_static_with_call_arity(
            instant,
            "fromEpochNanoseconds",
            temporal_instant_from_epoch_ns_thunk as *const u8,
            1,
            0,
            true,
        );
        install_temporal_prototype(
            instant,
            crate::temporal::TemporalKind::Instant as u8,
            &["epochMilliseconds", "epochNanoseconds"],
            &[
                ("add", 1),
                ("subtract", 1),
                ("until", 1),
                ("since", 1),
                ("round", 1),
                ("equals", 1),
                ("toZonedDateTimeISO", 1),
                ("toString", 0),
                ("toJSON", 0),
                ("toLocaleString", 0),
                ("valueOf", 0),
            ],
        );
    }

    // Temporal.PlainDate (#4691)
    let plain_date = install_temporal_constructor(
        ns_obj,
        "PlainDate",
        temporal_plain_date_ctor_thunk as *const u8,
        3,
    );
    if !plain_date.is_null() {
        install_temporal_from_compare(
            plain_date,
            temporal_plain_date_from_thunk as *const u8,
            temporal_plain_date_compare_thunk as *const u8,
        );
        install_temporal_prototype(
            plain_date,
            crate::temporal::TemporalKind::PlainDate as u8,
            PLAIN_DATE_GETTERS,
            PLAIN_DATE_METHODS,
        );
    }

    // Temporal.PlainTime (#4692)
    let plain_time = install_temporal_constructor(
        ns_obj,
        "PlainTime",
        temporal_plain_time_ctor_thunk as *const u8,
        0,
    );
    if !plain_time.is_null() {
        install_temporal_from_compare(
            plain_time,
            temporal_plain_time_from_thunk as *const u8,
            temporal_plain_time_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainDateTime (#4693)
    let plain_date_time = install_temporal_constructor(
        ns_obj,
        "PlainDateTime",
        temporal_plain_date_time_ctor_thunk as *const u8,
        3,
    );
    if !plain_date_time.is_null() {
        install_temporal_from_compare(
            plain_date_time,
            temporal_plain_date_time_from_thunk as *const u8,
            temporal_plain_date_time_compare_thunk as *const u8,
        );
        install_temporal_prototype(
            plain_date_time,
            crate::temporal::TemporalKind::PlainDateTime as u8,
            PLAIN_DATE_TIME_GETTERS,
            PLAIN_DATE_TIME_METHODS,
        );
    }

    // Temporal.PlainYearMonth (#4694)
    let plain_year_month = install_temporal_constructor(
        ns_obj,
        "PlainYearMonth",
        temporal_plain_year_month_ctor_thunk as *const u8,
        2,
    );
    if !plain_year_month.is_null() {
        install_temporal_from_compare(
            plain_year_month,
            temporal_plain_year_month_from_thunk as *const u8,
            temporal_plain_year_month_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainMonthDay (#4694) — `from` only, no `compare` per spec.
    let plain_month_day = install_temporal_constructor(
        ns_obj,
        "PlainMonthDay",
        temporal_plain_month_day_ctor_thunk as *const u8,
        2,
    );
    if !plain_month_day.is_null() {
        install_constructor_static_with_call_arity(
            plain_month_day,
            "from",
            temporal_plain_month_day_from_thunk as *const u8,
            1,
            0,
            true,
        );
    }

    // Temporal.ZonedDateTime (#4695)
    let zoned = install_temporal_constructor(
        ns_obj,
        "ZonedDateTime",
        temporal_zoned_date_time_ctor_thunk as *const u8,
        2,
    );
    if !zoned.is_null() {
        install_temporal_from_compare(
            zoned,
            temporal_zoned_date_time_from_thunk as *const u8,
            temporal_zoned_date_time_compare_thunk as *const u8,
        );
        // Real `Temporal.ZonedDateTime.prototype` with getter/method descriptors
        // so reflective test262 cases resolve (branding / prop-desc / length /
        // name / not-a-constructor). `ctor.prototype` is non-writable/non-enum/
        // non-config; `proto.constructor` is writable/non-enum/config (spec).
        let proto = build_zoned_date_time_prototype();
        if !proto.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto as i64);
            crate::closure::closure_set_dynamic_prop(zoned as usize, "prototype", proto_value);
            super::super::set_builtin_property_attrs(
                zoned as usize,
                "prototype".to_string(),
                super::super::PropertyAttrs::new(false, false, false),
            );
            set_intrinsic_data_prop(
                proto,
                "constructor",
                crate::value::js_nanbox_pointer(zoned as i64),
                super::super::PropertyAttrs::new(true, false, true),
            );
        }
    }

    // Populate each `Temporal.<Type>.prototype` with real accessor getters,
    // method functions, `@@toStringTag`, and a `constructor` back-reference so
    // Test262's prototype introspection (prop-desc / branding / length / name /
    // builtin / not-a-constructor) sees spec-correct shapes. Instance dispatch
    // still goes through the brand routers — these are reflection-only.
    use super::super::temporal_proto::populate_prototype;
    populate_prototype(
        duration,
        "Temporal.Duration",
        &[
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
            "sign",
            "blank",
        ],
        &[
            ("with", 1),
            ("negated", 0),
            ("abs", 0),
            ("add", 1),
            ("subtract", 1),
            ("round", 1),
            ("total", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        instant,
        "Temporal.Instant",
        // Per the current Temporal spec, `Temporal.Instant.prototype` exposes
        // only `epochMilliseconds` and `epochNanoseconds`; the older
        // `epochSeconds` / `epochMicroseconds` accessors were removed (Node v26
        // ships neither, and `get()` never implemented them).
        &["epochMilliseconds", "epochNanoseconds"],
        &[
            ("add", 1),
            ("subtract", 1),
            ("until", 1),
            ("since", 1),
            ("round", 1),
            ("equals", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
            ("toZonedDateTimeISO", 1),
        ],
    );
    populate_prototype(
        plain_date,
        "Temporal.PlainDate",
        &[
            "year",
            "month",
            "monthCode",
            "day",
            "dayOfWeek",
            "dayOfYear",
            "weekOfYear",
            "yearOfWeek",
            "daysInWeek",
            "daysInMonth",
            "daysInYear",
            "monthsInYear",
            "inLeapYear",
            "calendarId",
            "era",
            "eraYear",
        ],
        &[
            ("toPlainYearMonth", 0),
            ("toPlainMonthDay", 0),
            ("add", 1),
            ("subtract", 1),
            ("with", 1),
            ("withCalendar", 1),
            ("until", 1),
            ("since", 1),
            ("equals", 1),
            ("toPlainDateTime", 0),
            ("toZonedDateTime", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        plain_time,
        "Temporal.PlainTime",
        &[
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
        ],
        &[
            ("add", 1),
            ("subtract", 1),
            ("with", 1),
            ("until", 1),
            ("since", 1),
            ("round", 1),
            ("equals", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        plain_date_time,
        "Temporal.PlainDateTime",
        &[
            "year",
            "month",
            "monthCode",
            "day",
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
            "dayOfWeek",
            "dayOfYear",
            "weekOfYear",
            "yearOfWeek",
            "daysInWeek",
            "daysInMonth",
            "daysInYear",
            "monthsInYear",
            "inLeapYear",
            "calendarId",
            "era",
            "eraYear",
        ],
        &[
            ("with", 1),
            ("withPlainTime", 0),
            ("withCalendar", 1),
            ("add", 1),
            ("subtract", 1),
            ("until", 1),
            ("since", 1),
            ("round", 1),
            ("equals", 1),
            ("toPlainDate", 0),
            ("toPlainTime", 0),
            ("toZonedDateTime", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        plain_year_month,
        "Temporal.PlainYearMonth",
        &[
            "year",
            "month",
            "monthCode",
            "daysInMonth",
            "daysInYear",
            "monthsInYear",
            "inLeapYear",
            "calendarId",
            "era",
            "eraYear",
        ],
        &[
            ("with", 1),
            ("add", 1),
            ("subtract", 1),
            ("until", 1),
            ("since", 1),
            ("equals", 1),
            ("toPlainDate", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        plain_month_day,
        "Temporal.PlainMonthDay",
        &["monthCode", "day", "calendarId"],
        &[
            ("with", 1),
            ("equals", 1),
            ("toPlainDate", 1),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );
    populate_prototype(
        zoned,
        "Temporal.ZonedDateTime",
        &[
            "year",
            "month",
            "monthCode",
            "day",
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
            "epochMilliseconds",
            "epochNanoseconds",
            "timeZoneId",
            "calendarId",
            "dayOfWeek",
            "dayOfYear",
            "weekOfYear",
            "yearOfWeek",
            "hoursInDay",
            "daysInWeek",
            "daysInMonth",
            "daysInYear",
            "monthsInYear",
            "inLeapYear",
            "offset",
            "offsetNanoseconds",
            "era",
            "eraYear",
        ],
        &[
            ("with", 1),
            ("withPlainTime", 0),
            ("withTimeZone", 1),
            ("withCalendar", 1),
            ("add", 1),
            ("subtract", 1),
            ("until", 1),
            ("since", 1),
            ("round", 1),
            ("equals", 1),
            ("startOfDay", 0),
            ("getTimeZoneTransition", 1),
            ("toInstant", 0),
            ("toPlainDate", 0),
            ("toPlainTime", 0),
            ("toPlainDateTime", 0),
            ("toString", 0),
            ("toJSON", 0),
            ("toLocaleString", 0),
            ("valueOf", 0),
        ],
    );

    // Temporal.Now namespace (#4689)
    let now_value = build_temporal_now_namespace();
    let now_key = crate::string::js_string_from_bytes(b"Now".as_ptr(), 3);
    js_object_set_field_by_name(ns_obj, now_key, now_value);
    super::super::set_builtin_property_attrs(
        ns_obj as usize,
        "Now".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

/// Install the standard `from` (spec length 1) and `compare` (spec length 2)
/// statics — both variadic with call-arity 0 — on a Temporal constructor.
#[cfg(feature = "temporal")]
fn install_temporal_from_compare(
    ctor: *mut crate::closure::ClosureHeader,
    from_thunk: *const u8,
    compare_thunk: *const u8,
) {
    install_constructor_static_with_call_arity(ctor, "from", from_thunk, 1, 0, true);
    install_constructor_static_with_call_arity(ctor, "compare", compare_thunk, 2, 0, true);
}
