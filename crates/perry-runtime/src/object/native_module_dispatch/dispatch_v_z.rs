//! Per-module native-module dispatch buckets, relocated from
//! `native_module_dispatch.rs` to keep each file under the size budget
//! (issue #1103 split). Pure relocation — no logic change. The
//! `nm_general_closures!` macro is supplied by the parent module.
use super::*;

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_v8(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("v8", "serialize") => crate::node_v8::js_v8_serialize(arg(0)),
        ("v8", "deserialize") => crate::node_v8::js_v8_deserialize(arg(0)),
        ("v8", "getHeapStatistics") => crate::node_v8::js_v8_get_heap_statistics(),
        ("v8", "getHeapSpaceStatistics") => crate::node_v8::js_v8_get_heap_space_statistics(),
        ("v8", "getHeapCodeStatistics") => crate::node_v8::js_v8_get_heap_code_statistics(),
        ("v8", "cachedDataVersionTag") => crate::node_v8::js_v8_cached_data_version_tag(),
        ("v8", "getHeapSnapshot") => crate::node_v8::js_v8_get_heap_snapshot(arg(0)),
        ("v8", "writeHeapSnapshot") => crate::node_v8::js_v8_write_heap_snapshot(arg(0), arg(1)),

        // #3142: `new v8.GCProfiler()` keeps a small started flag on the
        // native-module instance. `stop()` returns a report only after start.
        ("v8.GCProfiler", "start") => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            crate::node_v8::js_v8_gc_profiler_start(recv)
        }
        ("v8.GCProfiler", "stop") => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            crate::node_v8::js_v8_gc_profiler_stop(recv)
        }

        // node:repl non-interactive server and constructor surface.
        ("v8.Serializer", m) | ("v8.DefaultSerializer", m) => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            match m {
                "writeHeader" => crate::node_v8::v8_serializer_write_header(recv),
                "writeValue" => crate::node_v8::v8_serializer_write_value(recv, arg(0)),
                "writeUint32" => crate::node_v8::v8_serializer_write_uint32(recv, arg(0)),
                "writeUint64" => crate::node_v8::v8_serializer_write_uint64(recv, arg(0), arg(1)),
                "writeDouble" => crate::node_v8::v8_serializer_write_double(recv, arg(0)),
                "writeRawBytes" => crate::node_v8::v8_serializer_write_raw_bytes(recv, arg(0)),
                "releaseBuffer" => crate::node_v8::v8_serializer_release_buffer(recv),
                // `_setTreatArrayBufferViewsAsHostObjects` is a no-op for us
                // (our writer always treats them as host objects).
                _ => f64::from_bits(JSValue::undefined().bits()),
            }
        }

        // #3680: `v8.Deserializer` / `v8.DefaultDeserializer` instance methods.
        ("v8.Deserializer", m) | ("v8.DefaultDeserializer", m) => {
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            match m {
                "readHeader" => crate::node_v8::v8_deserializer_read_header(recv),
                "readValue" => crate::node_v8::v8_deserializer_read_value(recv),
                "readUint32" => crate::node_v8::v8_deserializer_read_uint32(recv),
                "readUint64" => crate::node_v8::v8_deserializer_read_uint64(recv),
                "readDouble" => crate::node_v8::v8_deserializer_read_double(recv),
                "readRawBytes" => crate::node_v8::v8_deserializer_read_raw_bytes(recv, arg(0)),
                _ => f64::from_bits(JSValue::undefined().bits()),
            }
        }

        // #3679: `v8.startupSnapshot` namespace methods. Perry never builds a
        // startup snapshot, so `isBuildingSnapshot()` is `0` and the
        // serialize/deserialize-callback registrars throw like Node does when
        // called outside a snapshot-building context.
        ("v8.startupSnapshot", m) => match m {
            "isBuildingSnapshot" => crate::node_v8::js_v8_is_building_snapshot(),
            "addSerializeCallback" | "addDeserializeCallback" | "setDeserializeMainFunction" => {
                // #3141: Node's `ERR_NOT_BUILDING_SNAPSHOT` is a plain `Error`,
                // not a `TypeError`.
                crate::fs::validate::throw_error_with_code(
                    "Operation not allowed when not building startup snapshot.",
                    "ERR_NOT_BUILDING_SNAPSHOT",
                )
            }
            _ => f64::from_bits(JSValue::undefined().bits()),
        },

        // #3139: `v8.promiseHooks` namespace. Hook registrars install real
        // Promise-lifecycle callbacks (fired from `promise/{then,microtasks,
        // async_step}.rs`) and return a stop function that removes the hook.
        ("v8.promiseHooks", m) => match m {
            "onInit" => crate::v8::js_v8_promise_hooks_on_init(arg(0)),
            "onBefore" => crate::v8::js_v8_promise_hooks_on_before(arg(0)),
            "onAfter" => crate::v8::js_v8_promise_hooks_on_after(arg(0)),
            "onSettled" => crate::v8::js_v8_promise_hooks_on_settled(arg(0)),
            "createHook" => crate::v8::js_v8_promise_hooks_create_hook(arg(0)),
            _ => f64::from_bits(JSValue::undefined().bits()),
        },
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_vm(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("vm", m) => crate::node_vm::dispatch_vm_method(m, arg(0), arg(1), arg(2)),
        // ── tty module ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_wasi(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("wasi", "WASI") => crate::wasi::js_wasi_constructor_call(arg(0)),

        // ── net module legacy/internal helpers ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_zlib(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("zlib", _) => {
            let ptr =
                crate::value::JS_NATIVE_ZLIB_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
