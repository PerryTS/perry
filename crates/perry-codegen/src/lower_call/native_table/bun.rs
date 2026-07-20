use super::*;

/// `"bun"` module / `Bun.*` globals shim pack (issue #6560, Tier 0 of
/// Bun-app support — driver: opencode).
///
/// Runtime implementations live in perry-runtime `bun_compat` except the
/// url aliases, which reuse the `node:url` FFI directly.
/// `Bun.stdin` / `Bun.stdout` / `Bun.stderr` are property reads (handled by
/// `js_native_module_property_by_name`), not rows here.
pub(crate) const BUN_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "stringWidth",
        class_filter: None,
        runtime: "js_bun_string_width",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "hash",
        class_filter: None,
        // Returns an already-NaN-boxed BigInt value (not a raw
        // BigIntHeader pointer), so NR_F64 passes it through.
        runtime: "js_bun_hash",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "file",
        class_filter: None,
        runtime: "js_bun_file",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "write",
        class_filter: None,
        // Returns an already-settled Promise as a NaN-boxed value (same
        // convention as the fs.promises thunks).
        runtime: "js_bun_write",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    // Module-level aliases with node:url semantics (issue table:
    // `import { pathToFileURL, fileURLToPath } from "bun"`).
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "pathToFileURL",
        class_filter: None,
        runtime: "js_url_path_to_file_url",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun",
        has_receiver: false,
        method: "fileURLToPath",
        class_filter: None,
        runtime: "js_url_file_url_to_path",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
];
