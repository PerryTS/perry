//! Source-of-truth manifest of stdlib / native APIs Perry implements.
//!
//! Three consumers:
//!
//! - **perry-hir** consults [`module_has_symbol`] during HIR lowering to
//!   reject references to unimplemented APIs at compile time (#463), and
//!   [`module_has_public_named_export`] when checking Node-compatible named imports.
//! - **perry-codegen** keeps its native dispatch table aligned with this
//!   manifest via a CI test (`tests/manifest_consistency.rs`) — the
//!   manifest is the entry list, codegen owns the dispatch metadata.
//! - **perry's docs / .d.ts emit** iterates entries to produce an
//!   external view of the supported surface (#465).
//!
//! The schema is also the foundation for #466 Phase 2's external
//! `perry.nativeLibrary` manifest spec — third-party native bindings
//! will declare entries with the same shape, just `source: External`
//! instead of `Stdlib`.

#![deny(missing_docs)]

mod emit;
mod entries;
mod native_abi;

pub use emit::{emit_dts, emit_markdown};
pub use entries::{API_MANIFEST, NATIVE_MODULES, NODE_SUBMODULES, RUNTIME_ONLY_MODULES};
pub use native_abi::{
    native_handle_type_id, NativeAbiParseError, NativeAbiType, NativeHandleAbi,
    NativeHandleOwnership, NativeHandleThreadAffinity, NativePodAbi, NativePodFieldAbi,
    NativePromiseAbi, NativePromiseCompletion, NativePromiseThread,
};

/// One entry in the manifest. Identifies a single named symbol on a
/// known module — a method, a property, or a class.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ApiEntry {
    /// Module specifier (without the `node:` prefix).
    /// Example: `"crypto"`, `"fs"`, `"perry/ui"`, `"@perry/iroh"`.
    pub module: &'static str,
    /// Symbol name on the module.
    /// For methods and properties this is the bare identifier.
    /// For classes it's the class name.
    pub name: &'static str,
    /// What kind of symbol this is.
    pub kind: ApiKind,
    /// Where the implementation lives. Today nearly everything is
    /// [`ApiSource::Stdlib`]; #466 Phase 5 migrates some entries to
    /// [`ApiSource::WellKnown`] without changing user-visible behavior.
    pub source: ApiSource,
    /// Intentional no-op stubs (platform-gated UI symbols, etc.) are
    /// flagged so the unimplemented-API check (#463) does NOT error on
    /// them — the runtime first-call warning from #464 surfaces those.
    pub stub: bool,
    /// True when this row is a real top-level export of the module.
    ///
    /// Some rows exist only so the runtime dispatch table and strict
    /// property gate can recognize object members (`Buffer.from`,
    /// `performance.mark`, `process.on`, receiver methods, etc.). Those
    /// rows must remain in `API_MANIFEST`, but they are not ESM named
    /// exports and should not be accepted by named-import validation or
    /// emitted as top-level declarations.
    pub module_export: bool,
    /// ABI version this entry was published against. `None` for
    /// `Stdlib` source — the bundled stdlib is built and shipped
    /// together with the compiler, so its ABI moves in lockstep.
    /// Required for `External` source under #466 Phase 2.
    pub abi_version: Option<&'static str>,
    /// Declared parameter list for this method (#512). Empty for
    /// properties, classes, and methods whose param shape hasn't been
    /// backfilled — emit code falls back to `(...args: any[])` in that
    /// case so editors don't reject working calls. Auto-derived from
    /// `NATIVE_MODULE_TABLE` for module-level (no-receiver, no
    /// class_filter) rows; instance methods stay loose for now since
    /// the receiver-binding shape varies per dispatch path.
    pub params: &'static [ParamSpec],
    /// Declared return type. [`TypeSpec::Any`] means "fall back to the
    /// loose `any` rendering"; concrete values let the .d.ts emitter
    /// produce a typed signature. (#512)
    pub returns: TypeSpec,
}

/// One parameter slot on a method entry. Mirrors the param-type
/// vocabulary in `docs/src/native-libraries/manifest-v1.md` so the
/// in-tree manifest and external `perry.nativeLibrary` manifests share
/// one type model.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ParamSpec {
    /// A named, typed positional parameter (`name: ty`).
    Named {
        /// Parameter name. Auto-derived params use `p0`/`p1`/... — the
        /// dispatch table doesn't carry user-facing names.
        name: &'static str,
        /// Parameter type.
        ty: TypeSpec,
        /// True when the param is optional (`name?: ty`).
        optional: bool,
    },
    /// A trailing rest parameter (`...name: ty[]`).
    Rest {
        /// Parameter name.
        name: &'static str,
        /// Element type.
        ty: TypeSpec,
    },
}

/// Reduced type vocabulary the manifest uses for parameter and return
/// types. Mirrors the param/return type strings in
/// `docs/src/native-libraries/manifest-v1.md` so the in-tree manifest
/// and external `perry.nativeLibrary` manifests share one model.
///
/// Deliberately small. The dispatch table doesn't carry per-arg
/// TypeScript types (it's a Rust-ABI table); the manifest's job is to
/// say "this slot is a string vs. a number vs. opaque" with enough
/// fidelity that the `.d.ts` catches obvious mismatches like
/// `bcrypt.hash(123, "salt")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "camelCase"))]
pub enum TypeSpec {
    /// `string`.
    String,
    /// `number`.
    Number,
    /// `boolean`.
    Bool,
    /// `bigint`.
    BigInt,
    /// `Buffer` (Node Buffer / `Uint8Array`-shaped opaque).
    Buffer,
    /// `any` — opaque handle; the runtime shape varies and is not
    /// expressible in a useful TypeScript type today.
    Handle,
    /// `void`. Used for return slots when the runtime ignores the
    /// return value (`I32Void`/`Void`).
    Void,
    /// `any`. Default when no specific type fits or the dispatch path
    /// returns a NaN-boxed JSValue whose user-side type isn't fixed
    /// (`F64` returns can be `Promise<T>`, mixed strings/numbers, etc.).
    Any,
    /// `Promise<any>`. The call returns a runtime Promise whose resolved
    /// value isn't statically known (e.g. `perry/thread`'s `spawn`, whose
    /// resolution is the background closure's return value). Distinguishes
    /// "this is awaitable" from the opaque [`TypeSpec::Any`] fallback.
    Promise,
}

/// What shape of symbol this entry describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind"))]
pub enum ApiKind {
    /// A function/method on the module.
    Method {
        /// True for instance methods (`db.query(...)`); false for
        /// receiver-less calls (`crypto.randomUUID()`).
        has_receiver: bool,
        /// Optional class filter — when `Some("Pool")`, only matches
        /// instance methods whose receiver was constructed via that
        /// class. Mirrors `NativeModSig::class_filter` in codegen.
        class_filter: Option<&'static str>,
    },
    /// A constant or accessor property (`os.EOL`, `path.sep`).
    Property,
    /// A class exported by the module (`Buffer` on `"buffer"`,
    /// `Pool` on `"mysql2/promise"`).
    Class,
}

/// Where the implementation backing an entry lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum ApiSource {
    /// `perry-stdlib` or `perry-runtime` ships the implementation.
    Stdlib,
    /// A bundled wrapper crate registered in the well-known bindings
    /// table (#466 Phase 4). User imports the bare name (`mysql2`),
    /// resolution lands here. Same observable behavior as `Stdlib`.
    WellKnown,
    /// A third-party `node_modules/<pkg>/package.json` declares
    /// `perry.nativeLibrary` and provides the implementation (#466).
    External,
    /// Compiler-emitted symbol — no Rust function backs it. Currently
    /// unused; reserved for things like `import.meta.{url,dirname}`
    /// which the HIR lowers to literals at parse time.
    Intrinsic,
}

/// Look up `name` on `module` in the manifest. Strips a leading
/// `node:` prefix so callers don't have to. Returns the entry on hit.
///
/// Used by HIR lowering to gate property access against
/// `Expr::NativeModuleRef` — unknown lookups become a compile error
/// (#463).
pub fn module_has_symbol(module: &str, name: &str) -> Option<&'static ApiEntry> {
    let module = module.strip_prefix("node:").unwrap_or(module);
    // Match either:
    //  - a top-level export by name (`ethers.parseEther` → entry.name = parseEther)
    //  - any method whose class_filter is the requested name (`ethers.Wallet`
    //    → some entry has Method { class_filter: Some("Wallet") }). Without
    //    this branch, `ethers.Wallet.createRandom()` failed the #463
    //    unimplemented gate even though `createRandom` was registered with
    //    class_filter=Wallet.
    API_MANIFEST.iter().find(|e| {
        if e.module != module {
            return false;
        }
        if e.name == name {
            return true;
        }
        matches!(
            e.kind,
            ApiKind::Method { class_filter: Some(c), .. } if c == name
        )
    })
}

/// True if a module exposes a public ESM named export.
///
/// This is deliberately narrower than [`module_has_symbol`]. The manifest
/// also stores receiver methods, class-filtered dispatch helpers, and a few
/// object/static member shims so Perry can lower valid member access such as
/// `Buffer.alloc()` or `performance.mark()`. Those rows must not make
/// `import { alloc } from "node:buffer"` compile, because Node rejects that
/// at module instantiation.
pub fn module_has_public_named_export(module: &str, name: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    if name == "default" && is_node_core_module(module) {
        return true;
    }
    if is_platform_unavailable_named_export(module, name) {
        return false;
    }
    API_MANIFEST.iter().any(|entry| {
        entry.module == module && entry.name == name && entry_is_public_named_export(entry)
    })
}

/// #3902: a handful of `node:constants` values are Linux-only — `O_DIRECT` /
/// `O_NOATIME` (open(2) flags), `SIGPOLL` / `SIGPWR` / `SIGSTKFLT` (signals),
/// and `RTLD_DEEPBIND` (a glibc-only dlopen(3) flag). Node's `node:constants`
/// ESM namespace omits them on other platforms, so importing them there fails
/// Node's module instantiation.
///
/// These entries stay in the static `API_MANIFEST` unconditionally so the
/// generated docs (`docs/api/perry.d.ts`, `docs/src/api/reference.md`) and
/// `--print-api-manifest` are byte-identical regardless of the host OS the
/// generator runs on — otherwise `api-docs-drift` would fail on every PR (CI
/// runs on Linux; the invariant is asserted by
/// `deprecated_constants_alias_has_manifest_entries`). But the *import gate*
/// must still match the host: on a non-Linux host these names are not valid
/// named exports, so `perry check`/compile rejects them with `U006` instead of
/// silently binding `undefined` and diverging from Node.
fn is_platform_unavailable_named_export(module: &str, name: &str) -> bool {
    let linux_only = module == "constants"
        && matches!(
            name,
            "O_DIRECT" | "O_NOATIME" | "RTLD_DEEPBIND" | "SIGPOLL" | "SIGPWR" | "SIGSTKFLT"
        );
    linux_only && !cfg!(target_os = "linux")
}

/// True for Node.js built-in module specifiers that should use Node's public
/// named-export surface when validating static imports.
pub fn is_node_core_module(module: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    matches!(
        module,
        "assert"
            | "assert/strict"
            | "async_hooks"
            | "buffer"
            | "child_process"
            | "cluster"
            | "console"
            | "constants"
            | "crypto"
            | "dgram"
            | "diagnostics_channel"
            | "dns"
            | "dns/promises"
            | "events"
            | "fs"
            | "fs/promises"
            | "http"
            | "http2"
            | "https"
            | "module"
            | "net"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "perf_hooks"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "readline/promises"
            | "stream"
            | "stream/consumers"
            | "stream/promises"
            | "stream/web"
            | "string_decoder"
            | "sys"
            | "test"
            | "test/reporters"
            | "timers"
            | "timers/promises"
            | "tls"
            | "tty"
            | "url"
            | "util"
            | "util/types"
            | "v8"
            | "vm"
            | "wasi"
            | "worker_threads"
            | "zlib"
    )
}

/// Public named-export filter shared by the import gate and docs emitters.
pub fn entry_is_public_named_export(entry: &ApiEntry) -> bool {
    if !entry.module_export {
        return false;
    }
    if is_node_core_private_named_export(entry.module, entry.name) {
        return false;
    }
    match entry.kind {
        ApiKind::Method {
            has_receiver: false,
            class_filter: None,
        }
        | ApiKind::Property
        | ApiKind::Class => true,
        ApiKind::Method { .. } => false,
    }
}

fn is_node_core_private_named_export(module: &str, name: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    if !is_node_core_module(module) {
        return false;
    }
    match module {
        "buffer" => matches!(
            name,
            "alloc"
                | "allocUnsafe"
                | "allocUnsafeSlow"
                | "byteLength"
                | "concat"
                | "copyBytesFrom"
                | "from"
                | "fromBase64"
                | "fromHex"
                | "isBuffer"
                | "isEncoding"
                | "of"
        ),
        "crypto" => matches!(name, "md5" | "randomUUIDv7" | "sha256"),
        "perf_hooks" => matches!(
            name,
            "clearMarks"
                | "clearMeasures"
                | "clearResourceTimings"
                | "getEntries"
                | "getEntriesByName"
                | "getEntriesByType"
                | "mark"
                | "markResourceTiming"
                | "measure"
                | "nodeTiming"
                | "now"
                | "setResourceTimingBufferSize"
                | "supportedEntryTypes"
                | "timeOrigin"
                | "toJSON"
        ),
        "string_decoder" => matches!(name, "encoding" | "lastChar" | "lastNeed" | "lastTotal"),
        "process" => matches!(
            name,
            "addListener"
                | "emit"
                | "eventNames"
                | "getMaxListeners"
                | "listenerCount"
                | "listeners"
                | "off"
                | "on"
                | "once"
                | "prependListener"
                | "prependOnceListener"
                | "rawListeners"
                | "removeAllListeners"
                | "removeListener"
                | "setMaxListeners"
        ),
        "url" => matches!(name, "createObjectURL" | "revokeObjectURL"),
        "module" => matches!(name, "wrap" | "wrapper"),
        "worker_threads" => matches!(name, "getWorkerData" | "postMessage"),
        "https" => matches!(name, "ClientRequest" | "IncomingMessage" | "ServerResponse"),
        "http2" => matches!(
            name,
            "Http2SecureServer" | "listen" | "close" | "on" | "address"
        ),
        "child_process" => name == "Stream",
        "cluster" => matches!(name, "addListener" | "on" | "worker"),
        "stream" => matches!(name, "from" | "fromWeb" | "prototype" | "toWeb"),
        _ => false,
    }
}

/// True if `path` resolves to a Perry-implemented native module.
/// Strips `node:` prefix. This is the migrated home of the
/// `is_native_module` check that previously lived in
/// `perry-hir::ir`.
pub fn is_known_module(path: &str) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized) || NODE_SUBMODULES.contains(&normalized)
}

/// True if `module` is handled entirely by `perry-runtime` (no
/// `perry-stdlib` link required). Used by the linker's auto-feature
/// detection — modules in this list don't enable any
/// `perry-stdlib` cargo feature.
pub fn is_runtime_only_module(module: &str) -> bool {
    let normalized = module.strip_prefix("node:").unwrap_or(module);
    RUNTIME_ONLY_MODULES.contains(&normalized)
}

/// Iterate every entry in the manifest. Stable order: matches the
/// declaration order in `entries.rs`. Useful for the `--print-api-manifest`
/// CLI flag (#465 starter).
pub fn iter_entries() -> impl Iterator<Item = &'static ApiEntry> {
    API_MANIFEST.iter()
}

/// Returns true if the manifest has at least one entry on `module`.
///
/// Used by the unimplemented-API check (#463) to gate strictness:
/// modules with at least one entry have all property accesses
/// validated; modules with zero entries fall through to existing
/// permissive behavior so that incremental coverage doesn't break
/// unrelated working code. Strips `node:` prefix.
pub fn module_has_any_entries(module: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    API_MANIFEST.iter().any(|e| e.module == module)
}

/// Iterate entries on a specific module. Useful for the docs serializer.
pub fn entries_for_module(module: &str) -> impl Iterator<Item = &'static ApiEntry> {
    let module = module.strip_prefix("node:").unwrap_or(module).to_string();
    API_MANIFEST
        .iter()
        .filter(move |e| e.module == module.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS_PROMISES_METHOD_EXPORTS: &[&str] = &[
        "access",
        "appendFile",
        "chmod",
        "chown",
        "copyFile",
        "cp",
        "glob",
        "lchmod",
        "lchown",
        "link",
        "lstat",
        "lutimes",
        "mkdir",
        "mkdtemp",
        "open",
        "opendir",
        "readFile",
        "readdir",
        "readlink",
        "realpath",
        "rename",
        "rm",
        "rmdir",
        "stat",
        "statfs",
        "symlink",
        "truncate",
        "unlink",
        "utimes",
        "watch",
        "writeFile",
    ];

    #[test]
    fn lookup_strips_node_prefix() {
        // Whatever `crypto.randomUUID` resolves to in the real manifest,
        // it must resolve identically under `node:crypto`.
        let bare = module_has_symbol("crypto", "randomUUID");
        let prefixed = module_has_symbol("node:crypto", "randomUUID");
        assert_eq!(bare.is_some(), prefixed.is_some());
    }

    #[test]
    fn buffer_inspect_max_bytes_is_manifest_property() {
        let entry = module_has_symbol("node:buffer", "INSPECT_MAX_BYTES")
            .expect("buffer.INSPECT_MAX_BYTES should be in the manifest");
        assert!(matches!(entry.kind, ApiKind::Property));
    }

    #[test]
    fn assert_strict_self_alias_has_manifest_entries() {
        let method = module_has_symbol("node:assert/strict", "strict")
            .expect("assert/strict.strict should be callable in the manifest");
        assert!(matches!(method.kind, ApiKind::Method { .. }));

        assert!(
            API_MANIFEST.iter().any(|entry| {
                entry.module == "assert/strict"
                    && entry.name == "strict"
                    && matches!(entry.kind, ApiKind::Property)
            }),
            "assert/strict.strict should also be a manifest property"
        );
    }

    #[test]
    fn util_is_array_is_manifest_method() {
        let entry = module_has_symbol("node:util", "isArray")
            .expect("util.isArray should be in the manifest");
        assert!(matches!(
            entry.kind,
            ApiKind::Method {
                has_receiver: false,
                class_filter: None
            }
        ));
        assert_eq!(entry.params.len(), 1);
        assert!(matches!(entry.returns, TypeSpec::Bool));
    }

    #[test]
    fn node_core_named_export_view_rejects_member_only_rows() {
        for (module, name) in [
            ("node:buffer", "alloc"),
            ("node:buffer", "from"),
            ("node:perf_hooks", "mark"),
            ("node:perf_hooks", "supportedEntryTypes"),
            ("node:string_decoder", "encoding"),
            ("node:tty", "clearLine"),
            ("node:process", "on"),
            ("node:process", "emit"),
            ("node:module", "wrap"),
            ("node:module", "wrapper"),
            ("node:url", "createObjectURL"),
            ("node:https", "ClientRequest"),
            ("node:http2", "Http2SecureServer"),
            ("node:child_process", "Stream"),
            ("node:cluster", "worker"),
            ("node:stream", "fromWeb"),
            ("node:crypto", "sha256"),
        ] {
            assert!(
                !module_has_public_named_export(module, name),
                "{module} should not expose invalid named export {name}"
            );
            assert!(
                module_has_symbol(module, name).is_some(),
                "{module}.{name} should remain available to member/dispatch checks"
            );
        }
    }

    #[test]
    fn node_core_named_export_view_keeps_real_exports() {
        for (module, name) in [
            ("node:buffer", "Buffer"),
            ("node:buffer", "atob"),
            ("node:perf_hooks", "performance"),
            ("node:perf_hooks", "timerify"),
            ("node:string_decoder", "StringDecoder"),
            ("node:tty", "ReadStream"),
            ("node:process", "cwd"),
            ("node:process", "env"),
            ("node:module", "builtinModules"),
            ("node:module", "createRequire"),
            ("node:url", "URL"),
            ("node:url", "fileURLToPath"),
            ("node:worker_threads", "parentPort"),
            ("node:worker_threads", "workerData"),
            ("node:path", "default"),
            ("node:https", "Agent"),
            ("node:https", "request"),
            ("node:http2", "Http2ServerRequest"),
            ("node:child_process", "ChildProcess"),
            ("node:cluster", "workers"),
            ("node:stream", "Readable"),
            ("node:stream", "compose"),
            ("node:crypto", "randomUUID"),
        ] {
            assert!(
                module_has_public_named_export(module, name),
                "{module} should expose real named export {name}"
            );
        }
    }

    #[test]
    fn worker_threads_post_message_is_receiver_only() {
        let entry = module_has_symbol("node:worker_threads", "postMessage")
            .expect("worker_threads.postMessage stays registered for receiver dispatch");
        assert!(matches!(
            entry.kind,
            ApiKind::Method {
                has_receiver: true,
                class_filter: None
            }
        ));
        assert!(
            !module_has_public_named_export("node:worker_threads", "postMessage"),
            "worker_threads.postMessage must not be accepted as a named export"
        );
    }

    #[test]
    fn path_make_long_is_manifest_method() {
        let entry = module_has_symbol("node:path", "_makeLong")
            .expect("node:path._makeLong should be in the manifest");
        assert!(matches!(
            entry.kind,
            ApiKind::Method {
                has_receiver: false,
                class_filter: None
            }
        ));
    }

    #[test]
    fn crypto_random_fill_is_manifest_method() {
        let entry = module_has_symbol("node:crypto", "randomFill")
            .expect("crypto.randomFill should be in the manifest");
        assert!(matches!(
            entry.kind,
            ApiKind::Method {
                has_receiver: false,
                class_filter: None
            }
        ));
    }

    #[test]
    fn zlib_codes_is_manifest_named_export_property() {
        let entry =
            module_has_symbol("node:zlib", "codes").expect("zlib.codes should be in the manifest");
        assert!(matches!(entry.kind, ApiKind::Property));
        assert!(
            module_has_public_named_export("node:zlib", "codes"),
            "zlib.codes should be available to named imports"
        );
    }

    #[test]
    fn ws_ready_state_constants_are_manifest_properties() {
        for name in ["CONNECTING", "OPEN", "CLOSING", "CLOSED"] {
            let entry = module_has_symbol("ws", name)
                .unwrap_or_else(|| panic!("ws.{name} should be in the manifest"));
            assert!(matches!(entry.kind, ApiKind::Property));
            assert!(
                module_has_public_named_export("ws", name),
                "ws.{name} should be available to named imports"
            );
        }
    }

    #[test]
    fn fs_promises_manifest_matches_runtime_backed_exports() {
        assert!(is_known_module("fs/promises"));
        assert!(is_known_module("node:fs/promises"));
        assert!(module_has_any_entries("fs/promises"));

        for name in FS_PROMISES_METHOD_EXPORTS {
            let entry = module_has_symbol("node:fs/promises", name).unwrap_or_else(|| {
                panic!("node:fs/promises missing runtime-backed export: {name}")
            });
            assert!(
                matches!(
                    entry.kind,
                    ApiKind::Method {
                        has_receiver: false,
                        class_filter: None
                    }
                ),
                "node:fs/promises::{name} should be a receiver-less method"
            );
        }

        let constants = module_has_symbol("node:fs/promises", "constants")
            .expect("node:fs/promises missing constants export");
        assert_eq!(
            constants.kind,
            ApiKind::Property,
            "node:fs/promises::constants should be an object-valued property"
        );

        for not_implemented in ["FileHandle", "Dir", "Dirent"] {
            assert!(
                module_has_symbol("node:fs/promises", not_implemented).is_none(),
                "node:fs/promises::{not_implemented} should stay out of the manifest until runtime-backed"
            );
        }
    }

    #[test]
    fn deprecated_constants_alias_has_manifest_entries() {
        for name in [
            "F_OK",
            "SIGTERM",
            "SIGINT",
            "EACCES",
            "PRIORITY_NORMAL",
            "RSA_PKCS1_PADDING",
            "SSL_OP_NO_SSLv2",
            "SSL_OP_NO_TLSv1",
            "POINT_CONVERSION_COMPRESSED",
            "POINT_CONVERSION_UNCOMPRESSED",
        ] {
            let entry = module_has_symbol("node:constants", name)
                .expect("node:constants representative property should be in the manifest");
            assert!(matches!(entry.kind, ApiKind::Property));
        }

        // Platform-specific constants are listed in the manifest
        // unconditionally so the generated docs (`docs/api/perry.d.ts`,
        // `docs/src/api/reference.md`) are byte-identical regardless of the
        // host OS the generator runs on — otherwise `api-docs-drift` fails for
        // every PR whenever the committed docs were regenerated on a different
        // OS than CI (macOS vs Linux). RTLD_DEEPBIND is therefore present on
        // all platforms; the runtime `constants` module still only exposes a
        // real value where the OS provides one.
        let rtld_deepbind = module_has_symbol("node:constants", "RTLD_DEEPBIND");
        assert!(
            rtld_deepbind.is_some(),
            "RTLD_DEEPBIND should be in the manifest on every platform"
        );
        assert!(matches!(rtld_deepbind.unwrap().kind, ApiKind::Property));
    }

    #[test]
    fn platform_unavailable_constants_gate_the_import_surface() {
        // #3902: Linux-only `node:constants` values stay in the manifest
        // (for portable docs) but the import gate must follow the host
        // platform so `perry check` matches Node's ESM instantiation.
        let linux_only = [
            "O_DIRECT",
            "O_NOATIME",
            "RTLD_DEEPBIND",
            "SIGPOLL",
            "SIGPWR",
            "SIGSTKFLT",
        ];
        for name in linux_only {
            // Always present in the raw manifest (docs portability).
            assert!(
                module_has_symbol("node:constants", name).is_some(),
                "{name} must remain in the manifest on every platform"
            );
            // Import gate tracks the host platform.
            assert_eq!(
                module_has_public_named_export("node:constants", name),
                cfg!(target_os = "linux"),
                "{name} import availability must match the host platform"
            );
        }
        // Cross-platform constants are always importable.
        for name in ["O_RDONLY", "O_DIRECTORY", "SIGINT"] {
            assert!(
                module_has_public_named_export("node:constants", name),
                "{name} must be importable on every platform"
            );
        }
    }

    #[test]
    fn known_modules_consistent_with_manifest() {
        // Every entry's module must be either importable or an explicit
        // internal dispatch key. Catches typos and entries on unregistered
        // modules without making private namespaces public modules.
        for entry in API_MANIFEST {
            let normalized = entry.module.strip_prefix("node:").unwrap_or(entry.module);
            assert!(
                is_known_module(entry.module)
                    || entries::INTERNAL_MODULE_KEYS.contains(&normalized),
                "manifest entry {}::{} on unknown module",
                entry.module,
                entry.name
            );
        }
    }

    #[test]
    fn sys_alias_mirrors_util_manifest() {
        assert!(is_known_module("sys"));
        assert!(is_known_module("node:sys"));
        assert!(module_has_any_entries("sys"));

        for name in [
            "format",
            "inspect",
            "types",
            "TextEncoder",
            "parseArgs",
            "stripVTControlCharacters",
        ] {
            assert!(
                module_has_symbol("node:sys", name).is_some(),
                "node:sys missing representative util alias export: {name}"
            );
        }

        let util_entries: Vec<&ApiEntry> =
            API_MANIFEST.iter().filter(|e| e.module == "util").collect();
        let sys_entries: Vec<&ApiEntry> =
            API_MANIFEST.iter().filter(|e| e.module == "sys").collect();
        assert_eq!(
            sys_entries.len(),
            util_entries.len(),
            "sys should mirror the public util module manifest surface"
        );

        for util_entry in util_entries {
            let sys_entry = sys_entries
                .iter()
                .copied()
                .find(|e| e.name == util_entry.name && e.kind == util_entry.kind)
                .unwrap_or_else(|| {
                    panic!(
                        "sys missing util alias entry {}::{:?}",
                        util_entry.name, util_entry.kind
                    )
                });
            assert_eq!(sys_entry.source, util_entry.source, "{}", util_entry.name);
            assert_eq!(sys_entry.stub, util_entry.stub, "{}", util_entry.name);
            assert_eq!(
                sys_entry.abi_version, util_entry.abi_version,
                "{}",
                util_entry.name
            );
            assert_eq!(
                sys_entry.params.len(),
                util_entry.params.len(),
                "{}",
                util_entry.name
            );
            assert_eq!(sys_entry.returns, util_entry.returns, "{}", util_entry.name);
        }
    }

    #[test]
    fn path_submodule_manifests_mirror_path() {
        let path_entries: Vec<&ApiEntry> =
            API_MANIFEST.iter().filter(|e| e.module == "path").collect();

        for module in ["path/posix", "path/win32"] {
            assert!(is_known_module(module));
            assert!(is_known_module(&format!("node:{module}")));
            assert!(module_has_any_entries(module));

            for name in ["join", "basename", "sep", "delimiter", "posix", "win32"] {
                assert!(
                    module_has_symbol(module, name).is_some(),
                    "{module} missing representative path export: {name}"
                );
            }

            let submodule_entries: Vec<&ApiEntry> =
                API_MANIFEST.iter().filter(|e| e.module == module).collect();
            assert_eq!(
                submodule_entries.len(),
                path_entries.len(),
                "{module} should mirror the public path module manifest surface"
            );

            for path_entry in &path_entries {
                let submodule_entry = submodule_entries
                    .iter()
                    .copied()
                    .find(|e| e.name == path_entry.name && e.kind == path_entry.kind)
                    .unwrap_or_else(|| {
                        panic!(
                            "{module} missing path alias entry {}::{:?}",
                            path_entry.name, path_entry.kind
                        )
                    });
                assert_eq!(
                    submodule_entry.source, path_entry.source,
                    "{module}::{}",
                    path_entry.name
                );
                assert_eq!(
                    submodule_entry.stub, path_entry.stub,
                    "{module}::{}",
                    path_entry.name
                );
                assert_eq!(
                    submodule_entry.params.len(),
                    path_entry.params.len(),
                    "{module}::{}",
                    path_entry.name
                );
                assert_eq!(
                    submodule_entry.returns, path_entry.returns,
                    "{module}::{}",
                    path_entry.name
                );
            }
        }
    }

    #[test]
    fn stream_consumers_manifest_surface_is_registered() {
        assert!(is_known_module("stream/consumers"));
        assert!(is_known_module("node:stream/consumers"));
        assert!(module_has_any_entries("stream/consumers"));

        for name in ["arrayBuffer", "blob", "buffer", "bytes", "json", "text"] {
            let entry = module_has_symbol("node:stream/consumers", name)
                .unwrap_or_else(|| panic!("stream/consumers missing {name}"));
            assert!(matches!(
                entry.kind,
                ApiKind::Method {
                    has_receiver: false,
                    class_filter: None,
                }
            ));
        }
    }

    #[test]
    fn dispatch_only_node_members_are_not_module_exports() {
        for (module, names) in [
            (
                "buffer",
                &[
                    "alloc",
                    "allocUnsafe",
                    "allocUnsafeSlow",
                    "byteLength",
                    "concat",
                    "copyBytesFrom",
                    "from",
                    "fromBase64",
                    "fromHex",
                    "isBuffer",
                    "isEncoding",
                    "of",
                ][..],
            ),
            ("crypto", &["md5", "randomUUIDv7", "sha256"][..]),
            (
                "perf_hooks",
                &[
                    "clearMarks",
                    "clearMeasures",
                    "clearResourceTimings",
                    "getEntries",
                    "getEntriesByName",
                    "getEntriesByType",
                    "mark",
                    "markResourceTiming",
                    "measure",
                    "nodeTiming",
                    "now",
                    "setResourceTimingBufferSize",
                    "supportedEntryTypes",
                    "timeOrigin",
                    "toJSON",
                ][..],
            ),
            (
                "process",
                &[
                    "addListener",
                    "emit",
                    "eventNames",
                    "getMaxListeners",
                    "listenerCount",
                    "listeners",
                    "off",
                    "on",
                    "once",
                    "prependListener",
                    "prependOnceListener",
                    "rawListeners",
                    "removeAllListeners",
                    "removeListener",
                    "setMaxListeners",
                ][..],
            ),
            (
                "string_decoder",
                &["encoding", "lastChar", "lastNeed", "lastTotal"][..],
            ),
            ("module", &["wrap", "wrapper"][..]),
            (
                "tty",
                &["clearLine", "clearScreenDown", "cursorTo", "moveCursor"][..],
            ),
            ("url", &["createObjectURL", "revokeObjectURL"][..]),
            (
                "https",
                &["ClientRequest", "IncomingMessage", "ServerResponse"][..],
            ),
            (
                "http2",
                &["Http2SecureServer", "listen", "close", "on", "address"][..],
            ),
            ("child_process", &["Stream"][..]),
            ("cluster", &["addListener", "on", "worker"][..]),
            ("stream", &["from", "fromWeb", "prototype", "toWeb"][..]),
            ("punycode.ucs2", &["decode", "encode"][..]),
        ] {
            for name in names {
                assert!(
                    module_has_symbol(module, name).is_some(),
                    "{module}.{name} should remain present for dispatch/member gates"
                );
                assert!(
                    !module_has_public_named_export(module, name),
                    "{module}.{name} must not be accepted as a top-level module export"
                );
            }
        }
    }

    #[test]
    fn punycode_ucs2_is_internal_namespace_not_builtin_submodule() {
        assert!(is_known_module("punycode"));
        assert!(is_known_module("node:punycode"));
        assert!(!is_known_module("punycode.ucs2"));
        assert!(!is_known_module("node:punycode.ucs2"));
        assert!(module_has_public_named_export("punycode", "ucs2"));

        for name in ["decode", "encode"] {
            assert!(
                module_has_symbol("punycode.ucs2", name).is_some(),
                "punycode.ucs2.{name} should remain available for namespace dispatch"
            );
            assert!(
                !module_has_public_named_export("punycode.ucs2", name),
                "punycode.ucs2.{name} must not be advertised as a module export"
            );
        }
    }

    #[test]
    fn representative_node_module_exports_stay_public() {
        for (module, names) in [
            (
                "buffer",
                &["Buffer", "Blob", "File", "atob", "constants"][..],
            ),
            (
                "crypto",
                &["createHash", "getRandomValues", "hash", "randomUUID"][..],
            ),
            (
                "perf_hooks",
                &[
                    "PerformanceObserver",
                    "constants",
                    "createHistogram",
                    "performance",
                ][..],
            ),
            ("process", &["cwd", "env", "pid", "version"][..]),
            (
                "module",
                &["builtinModules", "constants", "createRequire"][..],
            ),
            ("string_decoder", &["StringDecoder"][..]),
            ("tty", &["ReadStream", "WriteStream", "isatty"][..]),
            ("url", &["URL", "URLSearchParams", "fileURLToPath"][..]),
            (
                "worker_threads",
                &["Worker", "parentPort", "workerData"][..],
            ),
            ("https", &["Agent", "Server", "get", "request"][..]),
            (
                "http2",
                &["Http2ServerRequest", "Http2ServerResponse", "constants"][..],
            ),
            ("child_process", &["ChildProcess", "exec", "spawn"][..]),
            ("cluster", &["Worker", "fork", "isPrimary"][..]),
            ("stream", &["Readable", "Stream", "default", "pipeline"][..]),
        ] {
            for name in names {
                assert!(
                    module_has_public_named_export(module, name),
                    "{module}.{name} should remain a top-level module export"
                );
            }
        }
    }

    #[test]
    fn implemented_node_submodule_manifest_surfaces_are_registered() {
        let expected: &[(&str, &[(&str, ApiKind)])] = &[
            (
                "diagnostics_channel",
                &[
                    ("default", ApiKind::Property),
                    ("Channel", ApiKind::Class),
                    (
                        "channel",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "hasSubscribers",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "subscribe",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "tracingChannel",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "unsubscribe",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                ],
            ),
            (
                "fs/promises",
                &[
                    ("default", ApiKind::Property),
                    ("constants", ApiKind::Property),
                ],
            ),
            ("stream/consumers", &[("default", ApiKind::Property)]),
            (
                "stream/web",
                &[
                    ("default", ApiKind::Property),
                    ("ReadableStream", ApiKind::Class),
                    ("WritableStream", ApiKind::Class),
                    ("TransformStream", ApiKind::Class),
                    ("ByteLengthQueuingStrategy", ApiKind::Class),
                    ("CountQueuingStrategy", ApiKind::Class),
                    ("TextEncoderStream", ApiKind::Class),
                    ("TextDecoderStream", ApiKind::Class),
                ],
            ),
            (
                "test/reporters",
                &[
                    ("default", ApiKind::Property),
                    (
                        "spec",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "tap",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "dot",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "junit",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                    (
                        "lcov",
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        },
                    ),
                ],
            ),
        ];

        for (module, symbols) in expected {
            assert!(is_known_module(module), "{module} must be a known module");
            assert!(
                is_known_module(&format!("node:{module}")),
                "node:{module} must be a known module"
            );
            assert!(
                module_has_any_entries(module),
                "{module} must have manifest entries"
            );
            for (name, kind) in *symbols {
                let entry = module_has_symbol(&format!("node:{module}"), name)
                    .unwrap_or_else(|| panic!("{module} missing {name}"));
                assert_eq!(entry.kind, *kind, "{module}.{name}");
            }
        }
    }
}
