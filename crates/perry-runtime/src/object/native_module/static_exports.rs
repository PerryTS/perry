//! Export-specific callable bodies. The existing ESM and callable export cells
//! own identity; their body metadata directly names the selected implementation.
//! Reading one statically named member never installs a module dispatch bucket.
use super::*;
use crate::closure::JsFunctionInfo;
use perry_dispatch::native_callable_exports::{
    canonical_native_callable_module, canonical_native_callable_property,
};

/// The flag on `function` licenses this exact immutable descriptor layout.
/// Construction follows the export's own body, without a module registry.
#[repr(C)]
struct NativeExportInfo {
    function: JsFunctionInfo,
    construct: unsafe fn(*const f64, usize) -> Option<f64>,
}

pub(crate) unsafe fn construct(value: f64, args: *const f64, len: usize) -> Option<f64> {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    if !crate::closure::is_closure_ptr(ptr as usize) {
        return None;
    }
    let info = (*ptr).info;
    debug_assert!(
        !info.is_null(),
        "a reachable closure carries an info record"
    );
    if (*info).flags & crate::codegen_abi::FN_NATIVE_EXPORT == 0 {
        return None;
    }
    ((*info.cast::<NativeExportInfo>()).construct)(args, len)
}

#[inline(always)]
unsafe fn no_decoration(_name: &str, value: f64, _addr: usize) -> f64 {
    value
}

enum ExportStep {
    Done(f64),
    Existing(f64),
    Fresh(f64),
}

/// Everything before decoration: cache hits, the intrinsic and the freshly
/// published (undecorated) callable. One copy serves every export.
#[inline(never)]
fn export_begin(
    module: &'static str,
    property: &'static str,
    info: &'static JsFunctionInfo,
    observe_writes: bool,
    canonical_module: &'static str,
    canonical_property: &'static str,
) -> ExportStep {
    if !is_native_module_callable_export(module, property) {
        return ExportStep::Done(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if observe_writes {
        if let Some(value) = native_namespace_user_value(module, property) {
            return ExportStep::Done(value);
        }
    }
    let key = format!("{module}\0{property}");
    if let Some(bits) = NATIVE_ESM_EXPORT_VALUES.with(|v| v.borrow().get(&key).copied()) {
        return ExportStep::Done(f64::from_bits(bits));
    }
    // ESM aliases have separate live cells; the underlying callable is shared.
    let (module, property) = (canonical_module, canonical_property);
    let callable_key = format!("{module}\0{property}");
    let existing = NATIVE_CALLABLE_EXPORTS.with(|v| v.borrow().get(&callable_key).copied());
    let scope = crate::gc::RuntimeHandleScope::new();
    if let Some(bits) = existing {
        return ExportStep::Existing(f64::from_bits(bits));
    }
    if let Some(value) = intrinsic_export(module, property) {
        let value = scope.root_nanbox_f64(value);
        NATIVE_CALLABLE_EXPORTS.with(|v| {
            v.borrow_mut().insert(callable_key, value.get_nanbox_u64());
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
        return ExportStep::Existing(value.get_nanbox_f64());
    }
    install_native_module_vtable();
    let ns = scope.root_nanbox_f64(js_create_native_module_namespace(
        module.as_ptr(),
        module.len(),
    ));
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 3));
    closure.with_mut_ptr(|c| {
        crate::closure::js_closure_set_capture_f64(c, 0, ns.get_nanbox_f64());
        crate::closure::js_closure_set_capture_ptr(c, 1, property.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(c, 2, property.len() as i64);
    });
    let length =
        callable_export_arity_table::native_callable_export_arity(module, property).unwrap_or(0);
    closure.with_mut_ptr(|c| {
        callable_exports::set_bound_native_closure_metadata(
            c,
            callable_exports::native_callable_export_display_name(module, property),
            length,
        )
    });
    let value = scope.root_nanbox_f64(closure.with_mut_ptr(
        |c: *mut crate::closure::ClosureHeader| crate::value::js_nanbox_pointer(c as i64),
    ));
    // Publish before decoration: constructor/prototype graphs can cycle.
    NATIVE_CALLABLE_EXPORTS.with(|v| {
        v.borrow_mut().insert(callable_key, value.get_nanbox_u64());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
    ExportStep::Fresh(value.get_nanbox_f64())
}

/// The ESM live cell of this export now names `value`.
#[inline(never)]
fn export_finish(module: &str, property: &str, value: f64) -> f64 {
    let key = format!("{module}\0{property}");
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    NATIVE_ESM_EXPORT_VALUES.with(|v| {
        v.borrow_mut().insert(key, value.get_nanbox_u64());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
    value.get_nanbox_f64()
}

/// Only the decoration and sealing stay in the export's own body: both are
/// specialized by its names, so an undecorated export links none of them.
#[inline(always)]
fn export_value(
    module: &'static str,
    property: &'static str,
    info: &'static JsFunctionInfo,
    observe_writes: bool,
    decorate: impl FnOnce(&str, f64, usize) -> f64,
) -> f64 {
    let canonical_property = canonical_native_callable_property(module, property);
    let canonical_module = canonical_native_callable_module(module, canonical_property);
    match export_begin(
        module,
        property,
        info,
        observe_writes,
        canonical_module,
        canonical_property,
    ) {
        ExportStep::Done(value) => value,
        ExportStep::Existing(value) => export_finish(module, property, value),
        ExportStep::Fresh(value) => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let value = scope.root_nanbox_f64(value);
            decorate(
                canonical_property,
                value.get_nanbox_f64(),
                crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
            );
            crate::closure::native_constructor::seal_selected(
                canonical_module,
                canonical_property,
                value.get_nanbox_f64(),
            );
            export_finish(module, property, value.get_nanbox_f64())
        }
    }
}

#[inline(always)]
unsafe fn with_arguments<R>(args: *const f64, len: usize, f: impl FnOnce(*const f64) -> R) -> R {
    let input = if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, len)
    };
    let cells: Vec<std::cell::UnsafeCell<f64>> = input
        .iter()
        .map(|v| std::cell::UnsafeCell::new(*v))
        .collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    for cell in &cells {
        scope.root_nanbox_cell(cell);
    }
    f(cells.as_ptr().cast::<f64>())
}

include!(concat!(env!("OUT_DIR"), "/native_export_values.rs"));

/// These declarations share an intrinsic value with the global object. Giving
/// them a fresh callable would break Buffer/URL/TextDecoder identity.
#[inline(always)]
fn intrinsic_export(module: &str, property: &str) -> Option<f64> {
    if let Some(value) = worker_messaging_constructor_value(module, property) {
        return Some(value);
    }
    if module == "buffer" && property == "Buffer" {
        return Some(callable_exports::buffer_constructor_value());
    }
    if matches!(
        (module, property),
        ("buffer", "Blob" | "File")
            | ("url", "URL" | "URLSearchParams" | "URLPattern")
            | ("util", "TextEncoder" | "TextDecoder")
    ) {
        return Some(js_get_global_this_builtin_value(
            property.as_ptr(),
            property.len(),
        ));
    }
    if module == "crypto.webcrypto" {
        return super::super::global_this::webcrypto_method_value(property);
    }
    if module == "crypto.subtle" {
        return super::super::global_this::subtle_crypto_method_value(property);
    }
    None
}

/// The same existing ESM cell owns primitive and object-valued declarations.
/// The initializer is statically selected, so reading a member cannot acquire
/// a reference to every export in its containing module.
#[inline(always)]
pub(crate) fn selected_value(
    module: &'static str,
    property: &'static str,
    observe_writes: bool,
    initialize: impl FnOnce() -> f64,
) -> f64 {
    install_native_module_vtable();
    if observe_writes {
        if let Some(value) = native_namespace_user_value(module, property) {
            return value;
        }
    }
    let live = native_module_constant_is_live(module, property);
    let key = format!("{module}\0{property}");
    if !live {
        if let Some(bits) = NATIVE_ESM_EXPORT_VALUES.with(|v| v.borrow().get(&key).copied()) {
            return f64::from_bits(bits);
        }
    }
    let value = initialize();
    if !live && value.to_bits() != crate::value::TAG_UNDEFINED {
        NATIVE_ESM_EXPORT_VALUES.with(|v| {
            v.borrow_mut().insert(key, value.to_bits());
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    }
    value
}

/// A real body is entered directly, including shared global intrinsics.
/// Legacy bound-method sentinels still need their installed dispatch bucket;
/// entering one here would recurse through this namespace dispatcher.
#[inline(never)]
pub(crate) unsafe fn call_minted(
    module: &str,
    property: &str,
    receiver: f64,
    args: *const f64,
    len: usize,
) -> Option<f64> {
    let property = super::canonical_native_callable_property(module, property);
    let module = canonical_native_callable_module(module, property);
    let key = format!("{module}\0{property}");
    let value = NATIVE_CALLABLE_EXPORTS
        .with(|values| values.borrow().get(&key).copied())
        .map(f64::from_bits)?;
    let closure =
        crate::value::js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    if !crate::closure::is_closure_ptr(closure as usize) {
        return None;
    }
    debug_assert!(
        !(*closure).info.is_null(),
        "a reachable closure carries an info record"
    );
    if (*(*closure).info).code == crate::closure::BOUND_METHOD_FUNC_PTR {
        return None;
    }
    Some(crate::closure::js_native_call_value(
        value,
        crate::closure::JsThis::from_f64(receiver),
        args,
        len,
    ))
}

#[cfg(test)]
mod messaging_constructor_ownership_tests {
    use super::*;
    use crate::gc::RuntimeHandleScope;
    use crate::value::{js_nanbox_get_pointer, JSValue};

    #[test]
    fn messaging_constructor_ownership_survives_global_reassignment() {
        crate::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
        for name in ["MessageChannel", "MessagePort", "BroadcastChannel"] {
            let canonical = scope.root_nanbox_f64(
                worker_messaging_constructor_value("worker_threads", name).unwrap(),
            );
            assert!(!JSValue::from_bits(canonical.get_nanbox_u64()).is_undefined());
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            ));
            let previous =
                scope.root_nanbox_f64(key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_get_field_by_name_f64(
                        js_nanbox_get_pointer(global.get_nanbox_f64()) as *const _,
                        key,
                    )
                }));
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                crate::object::js_object_set_field_by_name(
                    js_nanbox_get_pointer(global.get_nanbox_f64()) as *mut _,
                    key,
                    123.0,
                )
            });
            let selected = scope.root_nanbox_f64(
                intrinsic_export("worker_threads", name)
                    .unwrap_or(f64::from_bits(JSValue::undefined().bits())),
            );
            let generic = scope.root_nanbox_f64(
                unsafe { get_native_module_constant("worker_threads", name, 0.0) }
                    .unwrap_or(f64::from_bits(JSValue::undefined().bits())),
            );
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                crate::object::js_object_set_field_by_name(
                    js_nanbox_get_pointer(global.get_nanbox_f64()) as *mut _,
                    key,
                    previous.get_nanbox_f64(),
                )
            });
            assert_eq!(selected.get_nanbox_u64(), canonical.get_nanbox_u64());
            assert_eq!(generic.get_nanbox_u64(), canonical.get_nanbox_u64());
        }
        assert!(intrinsic_export("worker_threads", "Worker").is_none());
        assert!(intrinsic_export("unrelated", "MessageChannel").is_none());
    }

    #[test]
    fn messaging_constructor_ownership_excludes_foreign_realm_publication() {
        crate::gc::gc_init();
        let scope = RuntimeHandleScope::new();
        let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
        let canonical = scope.root_nanbox_f64(
            worker_messaging_constructor_value("worker_threads", "MessageChannel").unwrap(),
        );
        assert!(!JSValue::from_bits(canonical.get_nanbox_u64()).is_undefined());
        let foreign = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let other = scope.root_nanbox_f64(
            worker_messaging_constructor_value("worker_threads", "MessagePort").unwrap(),
        );
        publish_worker_messaging_constructor(
            foreign.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
            "MessageChannel",
            other.get_nanbox_f64(),
        );
        let observed = scope.root_nanbox_f64(
            worker_messaging_constructor_value("worker_threads", "MessageChannel").unwrap(),
        );
        publish_worker_messaging_constructor(
            js_nanbox_get_pointer(global.get_nanbox_f64()) as *mut _,
            "MessageChannel",
            canonical.get_nanbox_f64(),
        );
        assert_eq!(observed.get_nanbox_u64(), canonical.get_nanbox_u64());
    }
}
