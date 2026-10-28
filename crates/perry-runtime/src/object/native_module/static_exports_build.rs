//! Build-time emission of independently linkable builtin export declarations.
use std::{fmt::Write, path::Path};
pub fn emit(out: &Path) {
    use perry_dispatch::native_callable_exports::{
        canonical_native_callable_module, canonical_native_callable_property,
        export_bucket, export_value_symbol, CALLABLE_EXPORT_TABLE,
    };
    let mut source = String::new();
    for &(module, properties) in CALLABLE_EXPORT_TABLE {
        let Some(bucket) = export_bucket(module) else {
            continue;
        };
        for &property in properties {
            let symbol = export_value_symbol(module, property).unwrap();
            let raw_module = module;
            let raw_property = property;
            let property = canonical_native_callable_property(module, property);
            let module = canonical_native_callable_module(module, property);
            let module = if module == "util/types" { "util.types" } else { module };
            let attach = match bucket {
                "child_process" | "cluster" | "crypto" | "dgram" | "domain" | "events" | "fs"
                | "http" | "inspector" | "net" | "readline" | "repl" | "stream" | "tls" | "tty"
                | "zlib" => format!("super::constructor_shapes::nm_attach_{bucket}"),
                "assert" | "async_hooks" | "module" | "perf" | "sqlite" | "util" | "wasi" => {
                    let name = if bucket == "perf" {
                        "perf_hooks"
                    } else {
                        bucket
                    };
                    format!("super::callable_exports::nm_attach_{name}")
                }
                "worker_threads" => "super::constructor_shapes::nm_attach_worker".to_string(),
                "v8" => "crate::node_v8::attach_v8_constructor".to_string(),
                _ => "no_decoration".to_string(),
            };
            let constructor = match bucket {
                "fs" | "events" | "stream" | "child_process" | "cluster" | "readline"
                | "repl" | "tls" | "tty" | "vm" | "wasi" => format!("super::super::class_registry::nm_ctor_{bucket}({module:?}, {property:?}, args, len)"),
                "net" => format!("super::super::native_module_dispatch::nm_ctor_net({module:?}, {property:?}, args, len)"),
                _ => "None".to_string(),
            };
            let invoke = if bucket == "worker_threads" {
                format!(
                    "crate::collection_iter::throw_type_error({:?})",
                    format!("{module}.{property} is not supported")
                )
            } else {
                format!("super::super::native_module_dispatch::nm_dispatch_{bucket}(&ctx, {module:?}, {property:?})")
            };
            let dependencies = if (bucket == "events"
                && matches!(property, "EventEmitter" | "EventEmitterAsyncResource"))
                || (bucket == "stream"
                    && matches!(
                        property,
                        "Stream" | "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                    )) {
                "super::install_nm_ee_ops(); crate::stream_hooks::install();"
            } else {
                ""
            };
            writeln!(
                source,
                r#"
#[no_mangle]
pub extern "C" fn {symbol}(observe_writes: i32) -> f64 {{
    {dependencies}
    unsafe extern "C" fn body(callee: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis, args: *const f64, len: usize) -> f64 {{
        let ns = crate::closure::js_closure_get_capture_f64(callee, 0);
        with_arguments(args, len, |args| {{
        let ctx = super::super::native_module_dispatch::NmCtx {{
            obj: crate::value::js_nanbox_get_pointer(ns) as *const super::super::ObjectHeader,
            args_ptr: args, args_len: len, assert_skip_prototype: false,
        }};
        {invoke}
        }})
    }}
    unsafe fn construct(args: *const f64, len: usize) -> Option<f64> {{
        let _ = (args, len);
        with_arguments(args, len, |args| {{ {constructor} }})
    }}
    static INFO: NativeExportInfo = NativeExportInfo {{ function: {{
        let mut info = crate::closure::JsFunctionInfo::of_native_args(body, 0);
        info.flags |= crate::codegen_abi::FN_NATIVE_EXPORT | crate::codegen_abi::FN_BUILTIN;
        info
    }}, construct }};
    export_value({raw_module:?}, {raw_property:?}, &INFO.function, observe_writes != 0,
        |_name, value, addr| unsafe {{ {attach}({property:?}, value, addr) }})
}}
"#
            )
            .unwrap();
        }
    }
    // A stream constructor's actual prototype chain reaches Stream, whose
    // own function properties expose the whole stream surface in Node.
    writeln!(
        source,
        "#[inline(always)]\npub(crate) fn stream_export_value(name: &str) -> f64 {{ match name {{"
    )
    .unwrap();
    let (_, properties) = CALLABLE_EXPORT_TABLE
        .iter()
        .find(|(module, _)| *module == "stream")
        .unwrap();
    for property in *properties {
        let getter = export_value_symbol("stream", property).unwrap();
        writeln!(source, "{property:?} => {getter}(0),").unwrap();
    }
    writeln!(
        source,
        "_ => f64::from_bits(crate::value::TAG_UNDEFINED), }} }}"
    )
    .unwrap();
    let mut emitted = std::collections::BTreeSet::new();
    for entry in perry_api_manifest::API_MANIFEST {
        let (module, property) = (entry.module, entry.name);
        if property == "default"
            || !perry_api_manifest::is_node_core_module(module)
            || !perry_api_manifest::module_has_public_named_export(module, property)
            || export_bucket(module).is_none()
            || export_value_symbol(module, property).is_some()
            || !emitted.insert((module, property))
        {
            continue;
        }
        let symbol =
            perry_dispatch::native_callable_exports::constant_value_symbol(module, property);
        // These resolvers normally sit behind the whole-module install hook.
        // A statically named declaration calls its owner directly instead.
        let owner = match module {
            "process" => "constants::nm_const_process",
            "inspector" | "inspector/promises" => "constants::nm_const_inspector",
            "tls" => "constants::nm_const_tls",
            "zlib" => "constants::nm_const_zlib",
            "http" | "https" => "constants::nm_const_http",
            "cluster" => "constants::nm_const_cluster",
            _ => "constants::get_native_module_constant",
        };
        let args = if owner.ends_with("get_native_module_constant") {
            "0.0"
        } else {
            "0.0, false"
        };
        let mut children = String::new();
        for candidate in [
            format!("{module}.{property}"),
            format!("{module}/{property}"),
        ] {
            if let Some((_, properties)) = CALLABLE_EXPORT_TABLE
                .iter()
                .find(|(name, _)| *name == candidate)
            {
                for child in *properties {
                    if let Some(getter) = export_value_symbol(&candidate, child) {
                        writeln!(children, "{getter}(0);").unwrap();
                    }
                }
            }
        }
        writeln!(
            source,
            r#"
#[no_mangle]
pub extern "C" fn {symbol}(observe_writes: i32) -> f64 {{
    selected_value({module:?}, {property:?}, observe_writes != 0, || unsafe {{
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(super::{owner}({module:?}, {property:?}, {args})
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)));
        // An object-valued declaration owns its callable members. Mint their
        // existing identity cells from their own bodies, without a dispatcher.
        {children}
        value.get_nanbox_f64()
    }})
}}
"#
        )
        .unwrap();
    }
    std::fs::write(out.join("native_export_values.rs"), source).unwrap();
}
