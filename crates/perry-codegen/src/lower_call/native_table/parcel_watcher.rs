use super::*;

/// The low-level binding object consumed by `@parcel/watcher/wrapper`.
/// Platform package names are canonicalized to `@parcel/watcher` by HIR
/// lowering, so one table covers the root plus every native-addon sidecar.
pub(super) const PARCEL_WATCHER_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "@parcel/watcher",
        has_receiver: false,
        method: "subscribe",
        class_filter: None,
        runtime: "js_parcel_watcher_subscribe",
        args: &[NA_STR, NA_PTR, NA_F64],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "@parcel/watcher",
        has_receiver: false,
        method: "unsubscribe",
        class_filter: None,
        runtime: "js_parcel_watcher_unsubscribe",
        args: &[NA_STR, NA_PTR, NA_F64],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "@parcel/watcher",
        has_receiver: false,
        method: "writeSnapshot",
        class_filter: None,
        runtime: "js_parcel_watcher_write_snapshot",
        args: &[NA_STR, NA_STR, NA_F64],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "@parcel/watcher",
        has_receiver: false,
        method: "getEventsSince",
        class_filter: None,
        runtime: "js_parcel_watcher_get_events_since",
        args: &[NA_STR, NA_STR, NA_F64],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "@parcel/watcher",
        has_receiver: false,
        method: "__nativeEventCount",
        class_filter: None,
        runtime: "js_parcel_watcher_native_event_count",
        args: &[],
        ret: NR_F64,
    },
];
