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
    if (*info).flags & crate::codegen_abi::FN_NATIVE_EXPORT == 0 {
        return None;
    }
    ((*info.cast::<NativeExportInfo>()).construct)(args, len)
}

#[inline(always)]
unsafe fn no_decoration(_name: &str, value: f64, _addr: usize) -> f64 {
    value
}

#[inline(always)]
fn export_value(
    module: &'static str,
    property: &'static str,
    info: &'static JsFunctionInfo,
    observe_writes: bool,
    decorate: impl FnOnce(&str, f64, usize) -> f64,
) -> f64 {
    if !is_native_module_callable_export(module, property) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if observe_writes {
        if let Some(value) = native_namespace_user_value(module, property) {
            return value;
        }
    }
    let key = format!("{module}\0{property}");
    if let Some(bits) = NATIVE_ESM_EXPORT_VALUES.with(|v| v.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }
    // ESM aliases have separate live cells; the underlying callable is shared.
    let property = canonical_native_callable_property(module, property);
    let module = canonical_native_callable_module(module, property);
    let callable_key = format!("{module}\0{property}");
    let existing = NATIVE_CALLABLE_EXPORTS.with(|v| v.borrow().get(&callable_key).copied());
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = if let Some(bits) = existing {
        scope.root_nanbox_f64(f64::from_bits(bits))
    } else if let Some(value) = intrinsic_export(module, property) {
        let value = scope.root_nanbox_f64(value);
        NATIVE_CALLABLE_EXPORTS.with(|v| {
            v.borrow_mut().insert(callable_key, value.get_nanbox_u64());
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
        value
    } else {
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
        let length = callable_export_arity_table::native_callable_export_arity(module, property)
            .unwrap_or(0);
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
        decorate(
            property,
            value.get_nanbox_f64(),
            crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
        );
        crate::closure::native_constructor::seal_selected(module, property, value.get_nanbox_f64());
        value
    };
    NATIVE_ESM_EXPORT_VALUES.with(|v| {
        v.borrow_mut().insert(key, value.get_nanbox_u64());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.get_nanbox_u64());
    value.get_nanbox_f64()
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
    if !crate::closure::is_closure_ptr(closure as usize)
        || (*(*closure).info).code == crate::closure::BOUND_METHOD_FUNC_PTR
    {
        return None;
    }
    Some(crate::closure::js_native_call_value(
        value,
        crate::closure::JsThis::from_f64(receiver),
        args,
        len,
    ))
}
