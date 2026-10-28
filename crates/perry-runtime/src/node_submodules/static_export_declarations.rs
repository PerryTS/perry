//! Each export owns its body metadata. Whole namespaces refer to those same
//! declarations, so taking one member keeps identity without retaining siblings.
use super::*;
macro_rules! declarations {
    ($(($spec:ident, $module:literal, $key:literal,
        [$(($getter:ident, $name:literal, $info:expr)),* $(,)?])),* $(,)?) => {
        $(pub(super) static $spec: SubmoduleSpec = SubmoduleSpec {
            key: $key,
            exports: &[$(&$getter::SPEC),*],
        };
        $(#[allow(non_snake_case)]
        mod $getter {
            use super::*;
            pub(super) static SPEC: ExportSpec = ExportSpec { name: $name, info: $info };
            #[no_mangle]
            pub extern "C" fn $getter(observe_writes: i32) -> f64 {
                crate::object::native_module::selected_native_export_value(
                    $module, $name, observe_writes != 0, || {
                        selected_export($spec.key, &SPEC)
                    })
            }
        })*)*
    };
}
perry_dispatch::native_submodule_exports!(declarations);

#[inline(always)]
fn selected_export(key: &'static str, export: &'static ExportSpec) -> f64 {
    if let Some(value) = special_export_value(key, export.name) {
        return value;
    }
    if key == "sys" {
        if let Some(value) = sys_util_export_value(export.name) {
            return value;
        }
    }
    // node:test's callable aliases have one declaration identity.
    let name = if key == "test" {
        let name = match export.name {
            "default" | "test" | "it" => "test",
            "suite" | "describe" => "suite",
            _ => export.name,
        };
        find_export(&SUBMOD_TEST, name)
            .map(|e| e.name)
            .unwrap_or(export.name)
    } else {
        export.name
    };
    let ptr = ensure_selected_export(
        key,
        export,
        name,
        crate::object::native_module::constructor_shapes::nm_attach_readline,
    );
    f64::from_bits(JSValue::pointer(ptr.cast()).bits())
}
