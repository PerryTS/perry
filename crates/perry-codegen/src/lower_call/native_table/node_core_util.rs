use super::*;

/// node:util MIME + legacy helper dispatch rows.
///
/// Split out of `node_core.rs` to keep that file under the 2,000-line
/// limit. These back the `MIMEType`/`MIMEParams` constructors and the
/// legacy `_extend`/`_errnoException`/`_exceptionWithHostPort` helpers
/// exposed on `node:util` (and its `node:sys` alias).
pub(super) const NODE_CORE_UTIL_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "_extend",
        class_filter: None,
        runtime: "js_util_extend",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "_errnoException",
        class_filter: None,
        runtime: "js_util_errno_exception",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "_exceptionWithHostPort",
        class_filter: None,
        runtime: "js_util_exception_with_host_port",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "MIMEType",
        class_filter: None,
        runtime: "js_util_mime_type_new",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "MIMEParams",
        class_filter: None,
        runtime: "js_util_mime_params_new",
        args: &[],
        ret: NR_F64,
    },
];
