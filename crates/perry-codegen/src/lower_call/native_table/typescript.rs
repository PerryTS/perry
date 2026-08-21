use super::*;

/// Native TypeScript runtime-transpilation subset used by OpenCode Code Mode
/// (#8511). The compiler enums are folded by HIR and therefore need no rows.
pub(super) const TYPESCRIPT_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "typescript",
        has_receiver: false,
        method: "transpileModule",
        class_filter: None,
        runtime: "js_typescript_transpile_module",
        args: &[NA_STR, NA_F64],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    NativeModSig {
        module: "typescript",
        has_receiver: false,
        method: "flattenDiagnosticMessageText",
        class_filter: None,
        runtime: "js_typescript_flatten_diagnostic_message_text",
        args: &[NA_F64, NA_STR, NA_F64],
        ret: NR_STR,
    },
];
