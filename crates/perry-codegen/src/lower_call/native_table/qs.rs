use super::*;

pub(super) const QS_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "qs",
        has_receiver: false,
        method: "stringify",
        class_filter: None,
        runtime: "js_qs_stringify",
        args: &[NA_F64, NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "qs",
        has_receiver: false,
        method: "parse",
        class_filter: None,
        runtime: "js_qs_parse",
        args: &[NA_STR, NA_F64],
        ret: NR_OBJ_FROM_JSON_STR,
    },
];
