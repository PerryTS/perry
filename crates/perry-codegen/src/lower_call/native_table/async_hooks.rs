use super::*;

pub(super) const ASYNC_HOOKS_ROWS: &[NativeModSig] = &[
    // ========== async_hooks.AsyncLocalStorage ==========
    // Instances are ordinary objects; every receiver method resolves from the
    // prototype. Only constructor statics and module functions stay here.
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "bind",
        class_filter: Some("AsyncLocalStorage"),
        runtime: "js_async_local_storage_static_bind_direct",
        args: &[NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "snapshot",
        class_filter: Some("AsyncLocalStorage"),
        runtime: "js_async_local_storage_static_snapshot_direct",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "bind",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_static_bind_direct",
        args: &[NA_F64, NA_F64, NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "createHook",
        class_filter: None,
        runtime: "js_async_hooks_create_hook",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "executionAsyncId",
        class_filter: None,
        runtime: "js_async_hooks_execution_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "triggerAsyncId",
        class_filter: None,
        runtime: "js_async_hooks_trigger_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "executionAsyncResource",
        class_filter: None,
        runtime: "js_async_hooks_execution_async_resource",
        args: &[],
        ret: NR_F64,
    },
];
