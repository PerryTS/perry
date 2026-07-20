//! `API_MANIFEST` entries, part 2. Split out of entries.rs to satisfy the
//! 2000-line file-size gate; concatenated at compile time by the parent.
//!
//! `use super::*` pulls in the parent's type imports and the const-fn entry
//! builders (`method`/`property`/`class`/…) — children can name an ancestor's
//! private items, so the builders need no visibility change.

use super::*;

pub(crate) const API_MANIFEST_PART_2: &[ApiEntry] = &[
    method_sig(
        "lodash",
        "drop",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "first",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "head", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "last", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "flatten",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "uniq", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "reverse",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "take",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "camelCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "kebabCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "snakeCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "clamp",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "range",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "times",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "size", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "sum",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "mean",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "sumBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "meanBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    method_sig("lodash", "tail", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "max", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "min", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "maxBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "minBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "clamp",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "inRange",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Bool,
    ),
    method_sig(
        "lodash",
        "random",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    // Factory takes an optional input (string | number | undefined) —
    // matches the 1-slot NA_JSV dispatch row (js_dayjs_factory).
    method_sig(
        "dayjs",
        "default",
        false,
        None,
        &[ParamSpec::Named {
            name: "input",
            ty: TypeSpec::Any,
            optional: true,
        }],
        TypeSpec::Any,
    ),
    method_sig(
        "dayjs",
        "dayjs",
        false,
        None,
        &[ParamSpec::Named {
            name: "input",
            ty: TypeSpec::Any,
            optional: true,
        }],
        TypeSpec::Any,
    ),
    method("dayjs", "format", true, None),
    method("dayjs", "year", true, None),
    method("dayjs", "month", true, None),
    method("dayjs", "date", true, None),
    method("dayjs", "day", true, None),
    method("dayjs", "hour", true, None),
    method("dayjs", "minute", true, None),
    method("dayjs", "second", true, None),
    method("dayjs", "millisecond", true, None),
    method("dayjs", "valueOf", true, None),
    method("dayjs", "unix", true, None),
    method("dayjs", "toISOString", true, None),
    method("dayjs", "add", true, None),
    method("dayjs", "subtract", true, None),
    method("dayjs", "startOf", true, None),
    method("dayjs", "endOf", true, None),
    method("dayjs", "isBefore", true, None),
    method("dayjs", "isAfter", true, None),
    method("dayjs", "isSame", true, None),
    method("dayjs", "isValid", true, None),
    method("dayjs", "diff", true, None),
    method("dayjs", "clone", true, None),
    // moment factory + instance methods (wired to the same handle-based
    // date runtime as dayjs; see native_table/dates.rs moment rows).
    method_sig(
        "moment",
        "default",
        false,
        None,
        &[ParamSpec::Named {
            name: "input",
            ty: TypeSpec::Any,
            optional: true,
        }],
        TypeSpec::Any,
    ),
    method_sig(
        "moment",
        "moment",
        false,
        None,
        &[ParamSpec::Named {
            name: "input",
            ty: TypeSpec::Any,
            optional: true,
        }],
        TypeSpec::Any,
    ),
    method("moment", "format", true, None),
    method("moment", "toISOString", true, None),
    method("moment", "valueOf", true, None),
    method("moment", "unix", true, None),
    method("moment", "year", true, None),
    method("moment", "month", true, None),
    method("moment", "date", true, None),
    method("moment", "day", true, None),
    method("moment", "hour", true, None),
    method("moment", "minute", true, None),
    method("moment", "second", true, None),
    method("moment", "millisecond", true, None),
    method("moment", "add", true, None),
    method("moment", "subtract", true, None),
    method("moment", "startOf", true, None),
    method("moment", "endOf", true, None),
    method("moment", "diff", true, None),
    method("moment", "isBefore", true, None),
    method("moment", "isAfter", true, None),
    method("moment", "isSame", true, None),
    method("moment", "isBetween", true, None),
    method("moment", "isValid", true, None),
    method("moment", "clone", true, None),
    method("moment", "fromNow", true, None),
    method("moment", "toDate", true, None),
    method_sig(
        "sharp",
        "default",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig("sharp", "sharp", false, None, &[p_str("p0")], TypeSpec::Any),
    method("sharp", "resize", true, None),
    method("sharp", "rotate", true, None),
    method("sharp", "flip", true, None),
    method("sharp", "flop", true, None),
    method("sharp", "grayscale", true, None),
    method("sharp", "blur", true, None),
    method("sharp", "sharpen", true, None),
    method("sharp", "extract", true, None),
    method("sharp", "autoOrient", true, None),
    method("sharp", "extend", true, None),
    method("sharp", "trim", true, None),
    method("sharp", "composite", true, None),
    method("sharp", "jpeg", true, None),
    method("sharp", "png", true, None),
    method("sharp", "webp", true, None),
    method("sharp", "avif", true, None),
    method("sharp", "toFile", true, None),
    method("sharp", "toBuffer", true, None),
    method("sharp", "metadata", true, None),
    method("sharp", "width", true, None),
    method("sharp", "height", true, None),
    method_sig(
        "cheerio",
        "load",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method("cheerio", "select", true, None),
    method("cheerio", "text", true, None),
    method("cheerio", "html", true, None),
    method("cheerio", "attr", true, None),
    method("cheerio", "length", true, None),
    method("cheerio", "first", true, None),
    method("cheerio", "last", true, None),
    method("cheerio", "eq", true, None),
    method("cheerio", "find", true, None),
    method("cheerio", "children", true, None),
    method("cheerio", "parent", true, None),
    method("cheerio", "hasClass", true, None),
    // #2935: gzipSync/deflateSync accept an optional `{ level }` options
    // object as the 2nd argument (dispatch is NA_JSV, so the data slot
    // accepts a string or Buffer alike).
    method_sig(
        "zlib",
        "gzipSync",
        false,
        None,
        &[p_any("p0"), ZLIB_OPTIONS_PARAM],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "gunzipSync",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "deflateSync",
        false,
        None,
        &[p_any("p0"), ZLIB_OPTIONS_PARAM],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "inflateSync",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "gzip",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "gunzip",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    // One-shot sync codecs that round out the #1843 set: raw deflate/inflate
    // (no zlib wrapper), auto-detect unzip, and CRC32.
    method_sig(
        "zlib",
        "deflateRawSync",
        false,
        None,
        &[p_any("p0"), ZLIB_OPTIONS_PARAM],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "inflateRawSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "unzipSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Buffer,
    ),
    // `crc32(data, seed?)` — `seed` is the running CRC from a prior chunk
    // so callers can stream a long input. Dispatch declares 2 args; mirror
    // that arity here so manifest_consistency stays green.
    method_sig(
        "zlib",
        "crc32",
        false,
        None,
        &[
            p_str("p0"),
            ParamSpec::Named {
                name: "seed",
                ty: TypeSpec::Number,
                optional: true,
            },
        ],
        TypeSpec::Number,
    ),
    // Callback-form one-shot codecs. Direct calls return `undefined`; promise
    // wrappers are provided by `util.promisify(...)`.
    method_sig(
        "zlib",
        "deflate",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "deflateRaw",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "inflate",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "inflateRaw",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "unzip",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    // Stream classes — registered as classes so `typeof zlib.Gzip` reads
    // "function". #1843 exposed the `create*` factories but not the
    // constructor names themselves.
    class("zlib", "Deflate"),
    class("zlib", "DeflateRaw"),
    class("zlib", "Gzip"),
    class("zlib", "Gunzip"),
    class("zlib", "Inflate"),
    class("zlib", "InflateRaw"),
    class("zlib", "Unzip"),
    class("zlib", "BrotliCompress"),
    class("zlib", "BrotliDecompress"),
    // `zlib.constants` — the ~50 Z_*/DEFLATE/INFLATE/GZIP/BROTLI_*/ZSTD_*
    // constants Node exposes on `require('node:zlib').constants`. Required
    // by axios for stream wiring. Values are resolved at runtime by
    // `get_native_module_constant` in `perry-runtime/src/object.rs`.
    property("zlib", "constants"),
    property("zlib", "codes"),
    class("zlib", "Deflate"),
    class("zlib", "DeflateRaw"),
    class("zlib", "Gzip"),
    class("zlib", "Gunzip"),
    class("zlib", "Inflate"),
    class("zlib", "InflateRaw"),
    class("zlib", "Unzip"),
    class("zlib", "BrotliCompress"),
    class("zlib", "BrotliDecompress"),
    class("zlib", "ZstdCompress"),
    class("zlib", "ZstdDecompress"),
    // #1843 — Brotli one-shot compress/decompress (sync + callback-form).
    method_sig(
        "zlib",
        "brotliCompressSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "brotliDecompressSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "brotliCompress",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "brotliDecompress",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    // #2510 — Zstd one-shot compress/decompress (sync + callback-form).
    method_sig(
        "zlib",
        "zstdCompressSync",
        false,
        None,
        &[p_any("p0"), ZLIB_OPTIONS_PARAM],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "zstdDecompressSync",
        false,
        None,
        &[p_any("p0"), ZLIB_OPTIONS_PARAM],
        TypeSpec::Buffer,
    ),
    method_sig(
        "zlib",
        "zstdCompress",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    method_sig(
        "zlib",
        "zstdDecompress",
        false,
        None,
        ZLIB_CALLBACK_ARGS,
        TypeSpec::Void,
    ),
    // #1843 — Transform-stream factories. Each returns a stream handle
    // supporting `.write`/`.end`/`.on('data'|'end'|'error')`/`.pipe`.
    // #4917 — deflate-family factories honor `options.level`; a supplied
    // `dictionary` warns once (decompressors fail loudly without it, so
    // the plain factories are no longer flagged).
    zlib_compressor_factory("createGzip"),
    zlib_stream_factory("createGunzip"),
    zlib_compressor_factory("createDeflate"),
    zlib_stream_factory("createInflate"),
    zlib_compressor_factory("createDeflateRaw"),
    zlib_stream_factory("createInflateRaw"),
    zlib_stream_factory("createUnzip"),
    zlib_params_factory("createBrotliCompress"),
    // `zlib.createBrotliDecompress(options?)` — now a real Transform stream
    // (still passes axios's `typeof === 'function'` module-init gate).
    zlib_params_factory("createBrotliDecompress"),
    zlib_params_factory("createZstdCompress"),
    zlib_params_factory("createZstdDecompress"),
    method_sig(
        "cron",
        "validate",
        false,
        None,
        &[ParamSpec::Named {
            name: "expr",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "cron",
        "schedule",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "expr",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "handler",
                ty: TypeSpec::Any,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "cron",
        "describe",
        false,
        None,
        &[ParamSpec::Named {
            name: "expr",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::String,
    ),
    method("cron", "start", true, None),
    method("cron", "stop", true, None),
    method("cron", "isRunning", true, None),
    method("cron", "nextDate", true, None),
    // npm `cron` package class form: `new CronJob(cronTime, onTick,
    // onComplete?, start?)` — constructed by the lower_builtin_new arm
    // (js_cron_job_new; no auto-start, matching the npm package). The
    // instance methods reuse the ("cron", true, …) rows above.
    class("cron", "CronJob"),
    method_sig(
        "perry/tui",
        "Text",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "Box", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "render",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig("perry/tui", "enter", false, None, &[], TypeSpec::Void),
    method_sig(
        "perry/tui",
        "state",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("perry/tui", "get", true, Some("State")),
    method("perry/tui", "set", true, Some("State")),
    method_sig(
        "perry/tui",
        "useInput",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "run",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig("perry/tui", "exit", false, None, &[], TypeSpec::Void),
    // `perry/yoga` — native taffy-backed flexbox primitives consumed by the
    // `yoga-layout` TS shim (see crates/perry-runtime/src/yoga.rs and
    // codegen's native_table/yoga.rs). All free functions taking numeric
    // handle/value args; the `(...args: any[]): any` .d.ts fallback is fine
    // since only the internal shim calls them. These rows mirror the dispatch
    // table so the manifest-consistency check (#513) stays satisfied.
    method("perry/yoga", "nodeNew", false, None),
    method("perry/yoga", "nodeFree", false, None),
    method("perry/yoga", "insertChild", false, None),
    method("perry/yoga", "removeChild", false, None),
    method("perry/yoga", "childCount", false, None),
    method("perry/yoga", "setMeasureFunc", false, None),
    method("perry/yoga", "unsetMeasureFunc", false, None),
    method("perry/yoga", "setNumber", false, None),
    method("perry/yoga", "setEdge", false, None),
    method("perry/yoga", "setGap", false, None),
    method("perry/yoga", "setEnum", false, None),
    method("perry/yoga", "calculateLayout", false, None),
    method("perry/yoga", "getComputed", false, None),
    method("perry/yoga", "getComputedEdge", false, None),
    method_sig(
        "perry/tui",
        "boxSetFlexDirection",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetJustifyContent",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetAlignItems",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetGap",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetPadding",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetWidth",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetHeight",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexGrow",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    // Manifest-consistency catch-up (release-sweep gate, v0.5.823):
    // NATIVE_MODULE_TABLE accumulated 12 perry/tui entries during the
    // #679 ink-API ergonomics work (v0.5.810) and follow-ups that
    // weren't mirrored here. Restoring drift-free state.
    method_sig(
        "perry/tui",
        "boxSetPaddingEach",
        false,
        None,
        &[
            p_any("p0"),
            p_any("p1"),
            p_any("p2"),
            p_any("p3"),
            p_any("p4"),
        ],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexShrink",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexBasis",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexBasisPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetWidthPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetHeightPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "TextStyled",
        false,
        None,
        &[p_str("p0"), p_str("p1"), p_str("p2"), p_any("p3")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Table",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Tabs",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "InputAt",
        false,
        None,
        &[p_str("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "AnimatedSpinner",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useStateTuple",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "Spacer", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "ProgressBar",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Spinner",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Input",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "List",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Select",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "TextArea",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    // ---- perry/tui ink-shape hooks (#679 Phase 1) ----
    method_sig(
        "perry/tui",
        "useState",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useStateSet",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useEffect",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useMemo",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useRef",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "useApp", false, None, &[], TypeSpec::Any),
    method_sig("perry/tui", "useStdout", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "waitUntilExit",
        false,
        None,
        &[],
        TypeSpec::Void,
    ),
    method("perry/tui", "exit", true, Some("TuiApp")),
    method("perry/tui", "waitUntilExit", true, Some("TuiApp")),
    method("perry/tui", "write", true, Some("TuiStdout")),
    method("perry/tui", "columns", true, Some("TuiStdout")),
    method("perry/tui", "rows", true, Some("TuiStdout")),
    method("perry/tui", "get", true, Some("RefBox")),
    method("perry/tui", "set", true, Some("RefBox")),
    // ---- perry/tui Phase 3 — focus management (#679) ----
    method_sig(
        "perry/tui",
        "useFocus",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "focusNext", false, None, &[], TypeSpec::Void),
    method_sig(
        "perry/tui",
        "focusPrevious",
        false,
        None,
        &[],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "focus",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useFocusManager",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method("perry/tui", "focusNext", true, Some("FocusManager")),
    method("perry/tui", "focusPrevious", true, Some("FocusManager")),
    method("perry/tui", "focus", true, Some("FocusManager")),
    method_sig(
        "readline",
        "createInterface",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("readline", "clearLine", false, None),
    method("readline", "clearScreenDown", false, None),
    method("readline", "cursorTo", false, None),
    method("readline", "moveCursor", false, None),
    method("readline", "emitKeypressEvents", false, None),
    method("readline", "question", true, None),
    method("readline", "on", true, None),
    method("readline", "close", true, None),
    method("readline", "iterator", true, None),
    method("readline", "pause", true, None),
    method("readline", "resume", true, None),
    method("readline", "prompt", true, None),
    method("readline", "setPrompt", true, None),
    method("readline", "getPrompt", true, None),
    method("readline", "write", true, None),
    method("readline", "getCursorPos", true, None),
    method("readline", "line", true, None),
    method("readline", "terminal", true, None),
    method_sig(
        "readline/promises",
        "createInterface",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("readline/promises", "question", true, None),
    method("readline/promises", "close", true, None),
    class("readline/promises", "Interface"),
    class("readline/promises", "Readline"),
    method_sig(
        "worker_threads",
        "getEnvironmentData",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "setEnvironmentData",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "worker_threads",
        "markAsUntransferable",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "worker_threads",
        "isMarkedAsUntransferable",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Bool,
    ),
    method_sig(
        "worker_threads",
        "markAsUncloneable",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "worker_threads",
        "moveMessagePortToContext",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "receiveMessageOnPort",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "postMessageToThread",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2"), p_any("p3")],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "MessageChannel",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "BroadcastChannel",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    // #3899: `workerData` is a value-only export (resolved to the worker's data,
    // or `null` on the main thread, by the value-shaped property arm in
    // `native_module.rs`). The old `internal_method_sig` row made
    // `module_has_symbol("worker_threads", "workerData")` return a `Method`, so
    // codegen's `typeof <module>.<member>` fold reported `"function"` (parentPort,
    // which has only a property row, correctly read `"object"`). Dropping the
    // method row lets workerData read through `property("worker_threads",
    // "workerData")` below, and `workerData()` throws a normal TypeError —
    // matching Node. (`getWorkerData` is kept for now: it is not a public named
    // export, but removing it entirely makes `worker_threads.getWorkerData()`
    // trip the #463 compile gate instead of Node's runtime TypeError — that
    // absent-member-read boundary is tracked by #3896.)
    internal_method_sig(
        "worker_threads",
        "getWorkerData",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    // Internal dispatch hooks for `worker_threads.locks.request/query`
    // (#3328). These are reached through the value-shaped `locks`
    // export rather than public top-level worker_threads named exports.
    internal_method_sig(
        "worker_threads",
        "request",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    internal_method_sig("worker_threads", "query", false, None, &[], TypeSpec::Any),
    internal_method("worker_threads", "postMessage", true, None),
    // Web-style EventTarget methods on `parentPort` / the `Worker` handle.
    // Like `postMessage`, these are reached through the value-shaped namespace
    // member path and dispatch dynamically on the real runtime object (which
    // installs `addEventListener`/`removeEventListener`); registering them here
    // keeps the #463 unimplemented-API gate from firing for the value-shaped
    // `parentPort.addEventListener(...)` form.
    internal_method("worker_threads", "addEventListener", true, None),
    internal_method("worker_threads", "removeEventListener", true, None),
    method("worker_threads", "on", true, Some("Worker")),
    method("worker_threads", "once", true, Some("Worker")),
    method("worker_threads", "off", true, Some("Worker")),
    method("worker_threads", "terminate", true, Some("Worker")),
    // #4917 — real: `ref()`/`unref()` flip `WorkerRecord.refed`, which
    // `js_worker_threads_has_pending` checks to keep the event loop alive
    // (a live refed worker holds the process; `unref()` releases it).
    method("worker_threads", "ref", true, Some("Worker")),
    method("worker_threads", "unref", true, Some("Worker")),
    method("worker_threads", "getHeapStatistics", true, Some("Worker")),
    method("worker_threads", "cpuUsage", true, Some("Worker")),
    method("worker_threads", "getHeapSnapshot", true, Some("Worker")),
    method("worker_threads", "startCpuProfile", true, Some("Worker")),
    method("worker_threads", "startHeapProfile", true, Some("Worker")),
    // node:worker_threads — value-shaped exports (#2135). Perry doesn't
    // spawn JS workers, so the main thread is the only thread: isMainThread
    // is always true, threadId is 0, resourceLimits is an empty object.
    // The values themselves are returned by `js_native_module_property_by_name`
    // (see `crates/perry-runtime/src/object/native_module.rs`).
    class("worker_threads", "Worker"),
    class("worker_threads", "MessageChannel"),
    class("worker_threads", "MessagePort"),
    class("worker_threads", "BroadcastChannel"),
    property("worker_threads", "isMainThread"),
    property("worker_threads", "isInternalThread"),
    property("worker_threads", "parentPort"),
    property("worker_threads", "threadId"),
    property("worker_threads", "threadName"),
    property("worker_threads", "workerData"),
    property("worker_threads", "resourceLimits"),
    property("worker_threads", "SHARE_ENV"),
    property("worker_threads", "locks"),
    method_sig(
        "ethers",
        "getAddress",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "formatEther",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "formatUnits",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "parseEther",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::BigInt,
    ),
    method_sig(
        "ethers",
        "parseUnits",
        false,
        None,
        &[p_str("p0"), p_any("p1")],
        TypeSpec::BigInt,
    ),
    method("ethers", "createRandom", false, Some("Wallet")),
    // ===========================================================
    // Methods dispatched via custom Expr::* variants
    // (perry-hir/src/lower/expr_call.rs and expr_member.rs)
    // ===========================================================

    // crypto — issue #463 calls out crypto.subtle.encrypt as the
    // motivating example. Some entries below are dispatched via
    // codegen-level chain pattern matching (createHash/createHmac via
    // expr.rs:8475+, pbkdf2Sync via expr.rs:8677+) rather than through
    // NATIVE_MODULE_TABLE — they do work, even though they don't show
    // up in the dispatch-table extraction.
    method("crypto", "randomBytes", false, None),
    method("crypto", "randomUUID", false, None),
    internal_method("crypto", "randomUUIDv7", false, None),
    method("crypto", "randomInt", false, None),
    method("crypto", "hash", false, None),
    internal_method("crypto", "sha256", false, None),
    internal_method("crypto", "md5", false, None),
    method("crypto", "getRandomValues", false, None),
    // crypto.randomFill(buffer[, offset][, size], callback) /
    // randomFillSync(buffer, offset?, size?) — fills the
    // typed-array / Buffer with cryptographically strong random
    // bytes in-place and returns the same object. Required by
    // axios (Uint32Array) for ID generation.
    method("crypto", "randomFill", false, None),
    method("crypto", "randomFillSync", false, None),
    method("crypto", "createHash", false, None),
    method("crypto", "createSign", false, None),
    method("crypto", "createVerify", false, None),
    // #3955: the Hash/Hmac/Sign/Verify constructor classes are public
    // `node:crypto` named exports in Node. The HIR call-lowering in
    // `lower/expr_call/crypto.rs` already routes `Hash(...)`/`Hmac(...)`/
    // `Sign(...)`/`Verify(...)` through the same path as their `create*`
    // factories, so these entries just expose them on the ESM/named-import
    // surface — `import { Hash } from "node:crypto"` previously failed `check`
    // with "does not provide an export named 'Hash'".
    method("crypto", "Hash", false, None),
    method("crypto", "Hmac", false, None),
    method("crypto", "Sign", false, None),
    method("crypto", "Verify", false, None),
    class("crypto", "ECDH"),
    // #1367: X509Certificate — `new X509Certificate(pem|der)` + read-only
    // subject/issuer/validFrom/validTo/serialNumber/fingerprint/ca props.
    class("crypto", "X509Certificate"),
    // #2565: public `KeyObject` constructor export. Runtime exposes the
    // class-like function and the supported secret-key `KeyObject.from`.
    class("crypto", "KeyObject"),
    // Legacy Netscape SPKAC helper namespace:
    // crypto.Certificate.{verifySpkac,exportPublicKey,exportChallenge}.
    property("crypto", "Certificate"),
    method("crypto", "createECDH", false, None),
    method("crypto", "createDiffieHellman", false, None),
    method("crypto", "createDiffieHellmanGroup", false, None),
    method("crypto", "getDiffieHellman", false, None),
    // #2706/#2716: Node also exposes the legacy DH factories as
    // constructor-named exports and exposes the one-shot `diffieHellman`
    // helper. Runtime/codegen routes these to the same classic-DH and X25519
    // helpers as the existing factory forms.
    class("crypto", "DiffieHellman"),
    class("crypto", "DiffieHellmanGroup"),
    method("crypto", "diffieHellman", false, None),
    method("crypto", "encapsulate", false, None),
    method("crypto", "decapsulate", false, None),
    method("crypto", "createPrivateKey", false, None),
    method("crypto", "createPublicKey", false, None),
    method("crypto", "generateKeyPairSync", false, None),
    method("crypto", "generateKeyPair", false, None),
    // #3927: `crypto.generateKeySync("aes"|"hmac", { length })` — the codegen
    // dispatch (expr/calls.rs → js_crypto_generate_key_sync) and the secret-key
    // KeyObject metadata (type/symmetricKeySize/export, fixed for 192/256 by
    // #3930) were already complete; only this manifest row was missing, so the
    // #463 unimplemented-API gate rejected the call before codegen ran.
    method("crypto", "generateKeySync", false, None),
    method("crypto", "generateKey", false, None),
    method("crypto", "createHmac", false, None),
    // `crypto.createCipheriv(alg, key, iv)` / `createDecipheriv(...)` —
    // issue #1075. Registers a CipherHandle dispatched via the
    // small-pointer-handle method route. Supports aes-128-cbc,
    // aes-256-cbc, aes-128-gcm, aes-256-gcm. Wired in `expr.rs`
    // (no NATIVE_MODULE_TABLE entry — direct dispatch like createHash).
    method("crypto", "createCipheriv", false, None),
    method("crypto", "createDecipheriv", false, None),
    // `crypto.Cipheriv` / `crypto.Decipheriv` — the constructor exports
    // behind the `createCipheriv()` / `createDecipheriv()` factories
    // (#3726). Node exposes them as enumerable constructor functions
    // (length 4). Perry reads them as callable handles via
    // `is_native_module_callable_export` / `native_callable_export_arity`;
    // the actual cipher behavior continues to flow through the
    // factory-helper codegen path.
    class("crypto", "Cipheriv"),
    class("crypto", "Decipheriv"),
    // `crypto.createSign(alg)` / `createVerify(alg)` — RSA PKCS#1 v1.5 sign /
    // verify over the SHA family (#1364). SignHandle dispatched like createHash
    // (no NATIVE_MODULE_TABLE entry — direct codegen dispatch in expr/calls.rs).
    method("crypto", "createSign", false, None),
    method("crypto", "createVerify", false, None),
    // `crypto.createSecretKey(key, encoding?)` — required by jose for the
    // JWT signing path; returns a Uint8Array-marked Buffer of the key
    // bytes that `instanceof Uint8Array` accepts on both sides of the
    // V8 boundary. Wired through codegen in `expr.rs` (no NATIVE_MODULE_TABLE
    // entry — direct dispatch matches the createHash/createHmac pattern).
    method("crypto", "createSecretKey", false, None),
    method("crypto", "pbkdf2Sync", false, None),
    method("crypto", "pbkdf2", false, None),
    method("crypto", "argon2Sync", false, None),
    method("crypto", "argon2", false, None),
    // crypto.scryptSync(password, salt, keylen, options?) -> Buffer. Wired in
    // codegen `expr/calls.rs`; HIR types the result as Uint8Array.
    method("crypto", "scryptSync", false, None),
    method("crypto", "scrypt", false, None),
    // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
    method("crypto", "hkdfSync", false, None),
    method("crypto", "hkdf", false, None),
    // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }
    // PEM strings (RSA / EC P-256). Wired in codegen `expr/calls.rs`.
    method("crypto", "generateKeyPairSync", false, None),
    // crypto.randomInt([min,] max) — uniform integer in [min, max).
    // crypto.timingSafeEqual(a, b) — constant-time byte comparison.
    // crypto.getHashes() / getCiphers() / getCurves() — supported-algorithm name lists.
    // crypto.getFips() — FIPS mode flag.
    // crypto.sign/verify/publicEncrypt/privateDecrypt/privateEncrypt/publicDecrypt —
    // asymmetric one-shot helpers. All wired in codegen `expr/calls.rs`
    // (direct dispatch, like createHash).
    method("crypto", "randomInt", false, None),
    method("crypto", "timingSafeEqual", false, None),
    method("crypto", "sign", false, None),
    method("crypto", "verify", false, None),
    method("crypto", "publicEncrypt", false, None),
    method("crypto", "privateDecrypt", false, None),
    method("crypto", "privateEncrypt", false, None),
    method("crypto", "publicDecrypt", false, None),
    method("crypto", "getHashes", false, None),
    method("crypto", "getCiphers", false, None),
    // #4033-adjacent: `crypto.getCipherInfo(nameOrNid[, options])` — the runtime
    // (`js_crypto_get_cipher_info`) + native-module dispatch already exist; only
    // the manifest row was missing, so the #463 gate rejected the call.
    method("crypto", "getCipherInfo", false, None),
    method("crypto", "getCurves", false, None),
    method("crypto", "getFips", false, None),
    method("crypto", "setFips", false, None),
    method("crypto", "secureHeapUsed", false, None),
    method("crypto", "generatePrime", false, None),
    method("crypto", "generatePrimeSync", false, None),
    method("crypto", "checkPrime", false, None),
    method("crypto", "checkPrimeSync", false, None),
    // Web Crypto API (issue #561) — `crypto.subtle.*`. The HIR
    // lowering at `crates/perry-hir/src/lower/expr_call.rs` recognizes
    // the `crypto.subtle.<method>(args)` chain and emits a
    // `WebCrypto*` HIR variant. Listing `subtle` here flips the strict
    // strict-API gate (#463) so unimported `crypto.subtle` reads inside
    // an import-style binding don't silently return undefined.
    property("crypto", "webcrypto"),
    property("crypto", "subtle"),
    // os — methods mapped to Expr::Os* in expr_call.rs.
    property("os", "default"),
    method("os", "platform", false, None),
    method("os", "availableParallelism", false, None),
    method("os", "arch", false, None),
    method("os", "endianness", false, None),
    method("os", "hostname", false, None),
    method("os", "homedir", false, None),
    method("os", "loadavg", false, None),
    method("os", "machine", false, None),
    method("os", "tmpdir", false, None),
    method("os", "totalmem", false, None),
    method("os", "freemem", false, None),
    method("os", "uptime", false, None),
    method("os", "type", false, None),
    method("os", "release", false, None),
    method("os", "cpus", false, None),
    method("os", "networkInterfaces", false, None),
    method("os", "userInfo", false, None),
    method("os", "version", false, None),
    method_sig(
        "os",
        "getPriority",
        false,
        None,
        &[ParamSpec::Named {
            name: "pid",
            ty: TypeSpec::Number,
            optional: true,
        }],
        TypeSpec::Number,
    ),
    method_sig(
        "os",
        "setPriority",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "pidOrPriority",
                ty: TypeSpec::Number,
                optional: false,
            },
            ParamSpec::Named {
                name: "priority",
                ty: TypeSpec::Number,
                optional: true,
            },
        ],
        TypeSpec::Void,
    ),
    property("os", "EOL"),
    property("os", "devNull"),
    // Issue #649: os/crypto.constants tables — see
    // get_native_module_constant in perry-runtime/src/object.rs.
    property("os", "constants"),
    property("crypto", "constants"),
    // Deprecated `node:constants` flat alias. It mirrors the fs/os/crypto
    // constants that Perry already exposes under module-specific
    // `*.constants` namespaces.
    property("constants", "default"),
    property("constants", "F_OK"),
    property("constants", "R_OK"),
    property("constants", "W_OK"),
    property("constants", "X_OK"),
    property("constants", "O_RDONLY"),
    property("constants", "O_WRONLY"),
    property("constants", "O_RDWR"),
    property("constants", "O_NOFOLLOW"),
    property("constants", "O_NOCTTY"),
    property("constants", "O_DIRECTORY"),
    property("constants", "O_DIRECT"),
    property("constants", "O_NOATIME"),
    property("constants", "O_NONBLOCK"),
    property("constants", "O_SYNC"),
    property("constants", "O_DSYNC"),
];
