//! Build-time emission of independently linkable builtin export declarations.
use std::{fmt::Write, path::Path};
pub fn emit(out: &Path) {
    use perry_dispatch::native_callable_exports::{
        export_bucket, export_value_symbol, CALLABLE_EXPORT_TABLE,
    };
    let mut source = String::new();
    for &(module, properties) in CALLABLE_EXPORT_TABLE {
        let Some(bucket) = export_bucket(module) else {
            continue;
        };
        for &property in properties {
            let symbol = export_value_symbol(module, property).unwrap();
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
                format!("super::super::native_module_dispatch::dispatch_native_module_method(ctx.obj, {property:?}, args, len)")
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
        {constructor}
    }}
    static INFO: NativeExportInfo = NativeExportInfo {{ function: {{
        let mut info = crate::closure::JsFunctionInfo::of_native_args(body, 0);
        info.flags |= crate::codegen_abi::FN_NATIVE_EXPORT | crate::codegen_abi::FN_BUILTIN;
        info
    }}, construct }};
    export_value({module:?}, {property:?}, &INFO.function, observe_writes != 0,
        |name, value, addr| unsafe {{ {attach}(name, value, addr) }})
}}
"#
            )
            .unwrap();
        }
    }
    std::fs::write(out.join("native_export_values.rs"), source).unwrap();
}
