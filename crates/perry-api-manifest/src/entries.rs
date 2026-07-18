//! The actual manifest data — the source of truth.
//!
//! Two categories of entry feed this table:
//!
//! 1. **Methods dispatched through `NATIVE_MODULE_TABLE`** in
//!    `crates/perry-codegen/src/lower_call.rs`. These are extracted
//!    mechanically and a CI test in `perry-codegen` asserts that every
//!    `NATIVE_MODULE_TABLE` entry has a counterpart here so drift can't
//!    ship.
//! 2. **Methods/properties dispatched via custom `Expr::*` variants**
//!    in `perry-hir`'s lowering — `crypto.randomUUID` lowers to
//!    `Expr::CryptoRandomUUID` directly, never touching
//!    `NATIVE_MODULE_TABLE`. Same for `os.platform` → `Expr::OsPlatform`,
//!    `path.join` → `Expr::PathJoin`, etc. These are listed manually
//!    below; coverage of a module is what promotes it to "strict mode"
//!    in the unimplemented-API check (#463) — modules with at least
//!    one entry have all references gated against the manifest, modules
//!    with zero entries fall through to existing permissive behavior.
//!
//! Adding a new method/property to a module here automatically lifts
//! the corresponding compile error.

use crate::{ApiEntry, ApiKind, ApiSource, ParamSpec, TypeSpec};

/// Module specifiers Perry recognizes as native (i.e. resolvable
/// without going through the V8 fallback). Migrated from
/// `crates/perry-hir/src/ir.rs::NATIVE_MODULES` so the manifest can
/// answer module-resolution questions without depending on
/// `perry-hir`. Order matches the original list to keep diffs minimal.
pub const NATIVE_MODULES: &[&str] = &[
    "mysql2",
    "mysql2/promise",
    "pg",
    "uuid",
    "bcrypt",
    "argon2",
    "ioredis",
    "axios",
    "node-fetch",
    "ws",
    "zlib",
    "crypto",
    "dotenv",
    "dotenv/config",
    "jsonwebtoken",
    "nanoid",
    "slugify",
    "validator",
    "ethers",
    "mongodb",
    "better-sqlite3",
    "sqlite",
    "tursodb",
    "iroh",
    "node-cron",
    "nodemailer",
    "http",
    "https",
    "http2",
    "inspector",
    "inspector/promises",
    "events",
    "domain",
    "os",
    "buffer",
    "assert",
    "assert/strict",
    "test",
    "child_process",
    "dns",
    "dns/promises",
    "dgram",
    "net",
    "tls",
    "stream",
    "streams",
    "fs",
    "module",
    "path",
    "path/posix",
    "path/win32",
    "console",
    "constants",
    "util",
    "util/types",
    "dns",
    "dns/promises",
    "url",
    "lru-cache",
    "commander",
    "decimal.js",
    "bignumber.js",
    "exponential-backoff",
    "lodash",
    "dayjs",
    "date-fns",
    "moment",
    "sharp",
    "cheerio",
    "cron",
    "fastify",
    "async_hooks",
    // #2875: internal module backing DisposableStack/AsyncDisposableStack
    // instance-method dispatch (no JS import surface).
    "__disposable__",
    "readline",
    "repl",
    "sea",
    "string_decoder",
    "querystring",
    "cluster",
    "tty",
    "wasi",
    "perf_hooks",
    "v8",
    "vm",
    "process",
    // Bare `perry` builtin — embedded-asset introspection (#5731):
    // `embeddedFiles`, `readEmbedded`, `isStandaloneExecutable`.
    "perry",
    "perry/tui",
    "perry/yoga",
    "perry/ui",
    "perry/system",
    "perry/plugin",
    "perry/widget",
    "perry/i18n",
    "worker_threads",
    "perry/thread",
    // `perry/gc` — explicit GC control (collect / minor / idleHint).
    // Served entirely by perry-runtime; a no-op-style Perry-native
    // surface like `perry/thread` (doesn't resolve under Node/Bun).
    "perry/gc",
    "perry/updater",
    "perry/container",
    "perry/container-compose",
    "perry/compose",
    "perry/workloads",
    "perry/media",
    "perry/audio",
    "perry/background",
    "redis",
    "rate-limiter-flexible",
    "fetch",
    // `@perryts/pdf` — official PDF creation package (#516).
    // Bundled wrapper lives in `crates/perry-ext-pdf`; the producer
    // side companion to the existing PdfView widget. d.ts at
    // `types/perry/pdf/index.d.ts`.
    "@perryts/pdf",
    // `perry/ads` — official in-app advertising package (#867).
    // MVP scaffold: bundled wrapper at `crates/perry-ext-ads`
    // returns structured `{ error: "no-sdk-linked" }` placeholders
    // until real Google Mobile Ads SDK integration lands. d.ts at
    // `types/perry/ads/index.d.ts`.
    "perry/ads",
    // #2513: deprecated Punycode/IDNA conversion module.
    "punycode",
    // #6560 — Bun compatibility: the `"bun"` module specifier (named
    // aliases `pathToFileURL` / `fileURLToPath` + type-only exports).
    // The `Bun.*` globals dispatch through the same "bun" module tag.
    "bun",
    // #6563: runtime-native pty under the node-pty JS shape. Both the
    // canonical package name (kimi-code's dynamic `import("node-pty")`) and
    // the API-identical @lydell fork (opencode's static import) resolve to
    // the one perry-runtime implementation — no N-API addon involved.
    "node-pty",
    "@lydell/node-pty",
];

/// Node built-in submodules that Perry routes through the
/// `node_submodules` runtime table rather than `NATIVE_MODULES`.
/// Keeping these separate preserves the compiler's submodule import
/// lowering while still allowing manifest/docs entries for the subpath.
pub const NODE_SUBMODULES: &[&str] = &[
    "diagnostics_channel",
    "fs/promises",
    "stream/promises",
    "stream/consumers",
    "stream/web",
    "readline/promises",
    "sys",
    "test",
    "test/reporters",
    // #2682: node:timers namespace + node:timers/promises subpath. Routed
    // through the runtime's `node_submodules` table; manifest entries cover
    // the export-shape (setTimeout/.../promises and the timers/promises
    // helpers) so the unimplemented-API gate and docs recognize the modules.
    "timers",
    "timers/promises",
];

/// Internal manifest keys used by dispatch/property gates but not importable
/// module specifiers.
#[cfg(test)]
pub(crate) const INTERNAL_MODULE_KEYS: &[&str] = &["inspector.Network", "punycode.ucs2"];

/// Modules handled entirely by `perry-runtime` — the linker doesn't
/// need to pull in `perry-stdlib` for these. Migrated from
/// `crates/perry-hir/src/ir.rs::RUNTIME_ONLY_MODULES`.
pub const RUNTIME_ONLY_MODULES: &[&str] = &[
    "fs",
    "path",
    "path/posix",
    "path/win32",
    "os",
    "buffer",
    "assert",
    "assert/strict",
    "test",
    "child_process",
    "dns",
    "dns/promises",
    "dgram",
    "inspector",
    "inspector/promises",
    "sea",
    "stream",
    "module",
    "url",
    "console",
    "util",
    "util/types",
    "dns",
    "dns/promises",
    "process",
    // #5731 — `perry` embed API is served entirely from perry-runtime
    // (registry + fs interception); no perry-stdlib surface needed.
    "perry",
    "perry/ui",
    "perry/system",
    "perry/widget",
    "perry/i18n",
    "perry/thread",
    "perry/gc",
    "perry/media",
    "perry/audio",
    "perry/tui",
    "perry/yoga",
    "perry/background",
    "tty",
    "wasi",
    "perf_hooks",
    "v8",
    "repl",
    // #6560 — Bun globals shim pack lives in perry-runtime `bun_compat`.
    "bun",
    // #6563: the pty lives in perry-runtime (child_process-style reactor).
    "node-pty",
    "@lydell/node-pty",
];

const fn method(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
) -> ApiEntry {
    method_entry(module, name, has_receiver, class_filter, true)
}

const fn internal_method(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
) -> ApiEntry {
    method_entry(module, name, has_receiver, class_filter, false)
}

const fn method_entry(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
    module_export: bool,
) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Method {
            has_receiver,
            class_filter,
        },
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: module_export && !has_receiver && class_filter.is_none(),
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

/// Method entry with declared `params` and `returns`. Used to backfill
/// auto-derivable rows from the codegen dispatch table so the
/// generated `.d.ts` carries real signatures (#512).
const fn method_sig(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
    params: &'static [ParamSpec],
    returns: TypeSpec,
) -> ApiEntry {
    method_sig_entry(
        module,
        name,
        has_receiver,
        class_filter,
        params,
        returns,
        true,
    )
}

const fn internal_method_sig(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
    params: &'static [ParamSpec],
    returns: TypeSpec,
) -> ApiEntry {
    method_sig_entry(
        module,
        name,
        has_receiver,
        class_filter,
        params,
        returns,
        false,
    )
}

const fn method_sig_entry(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
    params: &'static [ParamSpec],
    returns: TypeSpec,
    module_export: bool,
) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Method {
            has_receiver,
            class_filter,
        },
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: module_export && !has_receiver && class_filter.is_none(),
        abi_version: None,
        params,
        returns,
    }
}

const fn property(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Property,
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: true,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

const fn internal_property(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Property,
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: false,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

const fn class(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Class,
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: true,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

const fn internal_class(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Class,
        source: ApiSource::Stdlib,
        stub: false,
        stub_note: None,
        module_export: false,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

// -----------------------------------------------------------------------------
// Param shorthand consts. Auto-derived rows cite these to keep the
// table compact. Names are `p0`/`p1`/... — the codegen dispatch table
// doesn't carry user-facing names, and the manifest-v1 spec doesn't
// require them.
// -----------------------------------------------------------------------------

const fn p_str(name: &'static str) -> ParamSpec {
    ParamSpec::Named {
        name,
        ty: TypeSpec::String,
        optional: false,
    }
}
const fn p_any(name: &'static str) -> ParamSpec {
    ParamSpec::Named {
        name,
        ty: TypeSpec::Any,
        optional: false,
    }
}

/// #1843 — every `zlib.create*` Transform-stream factory shares the same
/// shape: an optional `options` object in, a stream handle (`Any`) out.
const ZLIB_STREAM_OPTS: &[ParamSpec] = &[ParamSpec::Named {
    name: "options",
    ty: TypeSpec::Any,
    optional: true,
}];
const ZLIB_CALLBACK_ARGS: &[ParamSpec] = &[p_any("buffer"), p_any("callback")];
/// #2935 — optional `{ level, ... }` options object for one-shot codecs.
const ZLIB_OPTIONS_PARAM: ParamSpec = ParamSpec::Named {
    name: "options",
    ty: TypeSpec::Any,
    optional: true,
};
const fn zlib_stream_factory(name: &'static str) -> ApiEntry {
    method_sig("zlib", name, false, None, ZLIB_STREAM_OPTS, TypeSpec::Any)
}
/// Deflate-family compressor factory: `level` is honored (#4917);
/// `strategy`/`memLevel` are validated but not applied, and a supplied
/// `dictionary` warns once instead of silently mis-compressing.
const fn zlib_compressor_factory(name: &'static str) -> ApiEntry {
    zlib_stream_factory(name)
        .stub_note("level honored; strategy/memLevel validated but not applied (#4917)")
}
/// Brotli/zstd factory: their `params` option shape is not wired up; a
/// passed options object warns once (#4917).
const fn zlib_params_factory(name: &'static str) -> ApiEntry {
    zlib_stream_factory(name)
        .stub_note("params/quality options accepted but ignored, warns once (#4917)")
}

mod part_1;
mod part_2;
mod part_3;
mod part_4;

use part_1::API_MANIFEST_PART_1;
use part_2::API_MANIFEST_PART_2;
use part_3::API_MANIFEST_PART_3;
use part_4::API_MANIFEST_PART_4;

const API_MANIFEST_LEN: usize = API_MANIFEST_PART_1.len()
    + API_MANIFEST_PART_2.len()
    + API_MANIFEST_PART_3.len()
    + API_MANIFEST_PART_4.len();

const fn build_api_manifest() -> [ApiEntry; API_MANIFEST_LEN] {
    // ApiEntry is Copy; seed with the first entry then overwrite every slot.
    let mut out = [API_MANIFEST_PART_1[0]; API_MANIFEST_LEN];
    let mut i = 0;
    let parts: [&[ApiEntry]; 4] = [
        API_MANIFEST_PART_1,
        API_MANIFEST_PART_2,
        API_MANIFEST_PART_3,
        API_MANIFEST_PART_4,
    ];
    let mut p = 0;
    while p < parts.len() {
        let part = parts[p];
        let mut j = 0;
        while j < part.len() {
            out[i] = part[j];
            i += 1;
            j += 1;
        }
        p += 1;
    }
    out
}

static API_MANIFEST_ARR: [ApiEntry; API_MANIFEST_LEN] = build_api_manifest();

/// Source-of-truth manifest. See module-level docs for what feeds it. The
/// entry data is split across `entries/part_{1..4}.rs` to keep each file under
/// the 2000-line CI gate and concatenated at compile time here, so
/// `API_MANIFEST` stays a `&'static [ApiEntry]` for every consumer.
pub static API_MANIFEST: &[ApiEntry] = &API_MANIFEST_ARR;
