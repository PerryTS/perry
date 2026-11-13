//! The runtime families, in one place (DESIGN-family-reach §2.2).
//!
//! A family is one install-hook group of perry-runtime
//! (`feature_hooks.rs`, `js_runtime_install_<id>`): code the always-linked
//! runtime reaches only through slots the install fills. The whole-program
//! fact `family_reach` (perry-wpfacts) says, per family, whether the program
//! may ever create or observe a value of it; this table says which realm
//! entries, modules and other families belong to a family. Everything that
//! is in no row is `core`: always linked, always live.
//!
//! Membership is by the NAME of the realm declaration (`RealmEntry::name`):
//! a holder's root name for a whole holder, a full entry name for a member
//! of another family on a holder (`String.prototype.normalize`). The realm
//! table stays the authority for what exists; this table only groups it.
//! `perry-wpfacts` checks that every root of the realm table is classified
//! here (a family holder or [`CORE_HOLDERS`]), so a new holder fails a test
//! until it is placed.

/// One install-hook group.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeFamily {
    /// The group's name: `js_runtime_install_<id>`.
    pub id: &'static str,
    /// Families whose install this family's code needs: the calls between
    /// installs (`feature_hooks.rs`) and the implications the feature
    /// detector applies outside them (`optimized_libs/features.rs`).
    pub requires: &'static [&'static str],
    /// Realm holders (root names) whose every entry is of this family.
    pub holders: &'static [&'static str],
    /// Realm entries of this family on a holder of another family or of
    /// core. An entry named here is of this family together with every
    /// entry below it (`process.report` covers `process.report.getReport`).
    pub members: &'static [&'static str],
    /// Node / Perry module specifiers (without `node:`) whose namespace is
    /// of this family.
    pub modules: &'static [&'static str],
}

/// A set of families: bit `i` is `RUNTIME_FAMILIES[i]`.
pub type FamilySet = u64;

const BYTE_HOLDERS: &[&str] = &[
    "%BufferSpeciesConstructor%",
    "%TypedArray%",
    "ArrayBuffer",
    "BigInt64Array",
    "BigUint64Array",
    "Buffer",
    "DataView",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "SharedArrayBuffer",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
];

/// The families, in the order of `RUNTIME_INSTALLS` (perry's
/// `stdlib_installs.rs`; a test there keeps the two lists equal).
pub const RUNTIME_FAMILIES: &[RuntimeFamily] = &[
    RuntimeFamily {
        id: "dyn_eval",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &["vm"],
    },
    RuntimeFamily {
        id: "diagnostics",
        requires: &[],
        holders: &[],
        members: &["process.report"],
        modules: &["bun", "inspector", "inspector/promises", "v8"],
    },
    RuntimeFamily {
        id: "bun_cli_utils",
        requires: &["bigint"],
        holders: &[],
        members: &[],
        modules: &["bun"],
    },
    RuntimeFamily {
        id: "intl_namespace",
        requires: &["intl_datetime", "intl_locale", "intl_segmenter"],
        holders: &["%CollatorCompare%", "Intl"],
        members: &[
            "Array.prototype.toLocaleString",
            "BigInt.prototype.toLocaleString",
            "Date.prototype.toLocaleDateString",
            "Date.prototype.toLocaleString",
            "Date.prototype.toLocaleTimeString",
            "Number.prototype.toLocaleString",
            "String.prototype.toLocaleLowerCase",
            "String.prototype.toLocaleUpperCase",
        ],
        modules: &[],
    },
    RuntimeFamily {
        id: "temporal",
        requires: &["bigint"],
        holders: &["Temporal"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "intl_datetime",
        requires: &[],
        holders: &[],
        members: &[
            "Date.prototype.toLocaleDateString",
            "Date.prototype.toLocaleString",
            "Date.prototype.toLocaleTimeString",
        ],
        modules: &[],
    },
    RuntimeFamily {
        id: "regex_engine",
        requires: &[],
        holders: &["RegExp"],
        // `match`, `matchAll` and `search` build a RegExp from a string
        // argument; `replace` and `split` need one handed to them.
        members: &[
            "String.prototype.match",
            "String.prototype.matchAll",
            "String.prototype.search",
        ],
        modules: &["util.types", "util/types", "v8", "worker_threads"],
    },
    RuntimeFamily {
        id: "url_engine",
        requires: &[],
        holders: &["URL", "URLPattern", "URLSearchParams"],
        members: &[],
        modules: &["module", "url"],
    },
    RuntimeFamily {
        id: "string_normalize",
        requires: &[],
        holders: &[],
        // `localeCompare` and `Intl.Collator` use the normalization tables.
        members: &[
            "Intl.Collator",
            "String.prototype.localeCompare",
            "String.prototype.normalize",
        ],
        modules: &[],
    },
    RuntimeFamily {
        id: "intl_segmenter",
        requires: &[],
        holders: &[],
        members: &["Intl.Segmenter"],
        modules: &[],
    },
    RuntimeFamily {
        id: "intl_locale",
        requires: &[],
        holders: &[],
        members: &["Intl.Locale", "Intl.getCanonicalLocales"],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_math",
        requires: &[],
        holders: &["Math"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_json",
        requires: &["json_engine"],
        holders: &["JSON"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "json_format",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "json_engine",
        requires: &["json_format"],
        holders: &[],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "turnloop",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &[
            "child_process",
            "crypto",
            "dgram",
            "dns",
            "dns/promises",
            "fs",
            "fs/promises",
            "http",
            "http2",
            "https",
            "net",
            "node-pty",
            "perry/thread",
            "readline",
            "readline/promises",
            "tls",
            "worker_threads",
            "zlib",
        ],
    },
    RuntimeFamily {
        id: "bigint",
        requires: &[],
        holders: &["BigInt", "BigInt64Array", "BigUint64Array", "Buffer"],
        members: &["process.hrtime"],
        modules: &[
            "bun",
            "bun:ffi",
            "bun:sqlite",
            "child_process",
            "crypto",
            "fs",
            "fs/promises",
            "os",
            "perf_hooks",
            "sqlite",
            "util.types",
            "util/types",
            "v8",
            "worker_threads",
        ],
    },
    RuntimeFamily {
        id: "proxy",
        requires: &[],
        holders: &["Proxy"],
        members: &[],
        modules: &["util.types", "util/types"],
    },
    RuntimeFamily {
        id: "async_hooks",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &["async_hooks", "events"],
    },
    RuntimeFamily {
        id: "thread",
        requires: &["event_target", "typedarray"],
        holders: &["structuredClone"],
        members: &["Atomics.waitAsync"],
        modules: &[
            "child_process",
            "diagnostics_channel",
            "module",
            "perf_hooks",
            "perry/thread",
            "v8",
            "worker_threads",
        ],
    },
    RuntimeFamily {
        id: "date",
        requires: &[],
        holders: &["Date"],
        members: &[],
        modules: &[
            "bun",
            "bun:ffi",
            "child_process",
            "crypto",
            "fs",
            "fs/promises",
            "util.types",
            "util/types",
            "v8",
            "worker_threads",
        ],
    },
    RuntimeFamily {
        id: "perf_hooks",
        requires: &["event_target", "thread"],
        holders: &[
            "Performance",
            "PerformanceEntry",
            "PerformanceMark",
            "PerformanceMeasure",
            "PerformanceObserver",
            "PerformanceObserverEntryList",
            "PerformanceResourceTiming",
            "performance",
        ],
        members: &[],
        modules: &["perf_hooks"],
    },
    RuntimeFamily {
        id: "os",
        requires: &[],
        holders: &[],
        members: &[
            "process.emit",
            "process.finalization",
            "process.kill",
            "process.off",
            "process.on",
            "process.once",
            "process.stderr",
            "process.stdin",
            "process.stdout",
        ],
        modules: &[
            "@lydell/node-pty",
            "bun-pty",
            "node-pty",
            "os",
            "process",
            "readline",
            "readline/promises",
            "repl",
            "tty",
        ],
    },
    RuntimeFamily {
        id: "messaging",
        requires: &["event_target", "thread"],
        holders: &["BroadcastChannel", "MessageChannel", "MessagePort"],
        members: &[],
        modules: &["worker_threads"],
    },
    RuntimeFamily {
        id: "event_target",
        requires: &[],
        holders: &[
            "AbortController",
            "AbortSignal",
            "CustomEvent",
            "DOMException",
            "Event",
            "EventTarget",
            "MessageEvent",
        ],
        members: &[],
        modules: &[
            "bun",
            "crypto",
            "perf_hooks",
            "perry/thread",
            "undici",
            "worker_threads",
        ],
    },
    RuntimeFamily {
        id: "navigator",
        requires: &["event_target"],
        holders: &[
            "Navigator",
            "Storage",
            "localStorage",
            "navigator",
            "sessionStorage",
        ],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "typedarray",
        requires: &[],
        holders: BYTE_HOLDERS,
        // `BigInt.asIntN` / `asUintN` convert their argument with the
        // group's ToBigInt (an empty hook answers 0n). `process.stdin` hands
        // its chunks to listeners as Buffers.
        members: &["BigInt.asIntN", "BigInt.asUintN", "process.stdin"],
        modules: &[
            "better-sqlite3",
            "buffer",
            "bun",
            "bun:ffi",
            "bun:jsc",
            "bun:sqlite",
            "child_process",
            "crypto",
            "dgram",
            "fs",
            "fs/promises",
            "http",
            "http2",
            "https",
            "iroh",
            "mysql2",
            "mysql2/promise",
            "net",
            "os",
            "perry/thread",
            "pg",
            "process",
            // readline and repl write to and read from the streams they are
            // given; `sea.getAsset` answers an ArrayBuffer.
            "readline",
            "readline/promises",
            "repl",
            "sea",
            "sqlite",
            "stream",
            "stream/consumers",
            "stream/promises",
            "stream/web",
            "string_decoder",
            // The test runner's event stream and the reporters are streams.
            "test",
            "test/reporters",
            "tls",
            "tty",
            "tursodb",
            // `url.fileURLToPathBuffer` answers a Buffer.
            "url",
            "util.types",
            "util/types",
            "v8",
            // `vm.Script` and `vm.compileFunction` produce cached data.
            "vm",
            "worker_threads",
            "ws",
            "zlib",
        ],
    },
    RuntimeFamily {
        id: "global_reflect",
        requires: &[],
        holders: &["Reflect"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_atomics",
        requires: &["thread", "typedarray"],
        holders: &["Atomics"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_url",
        requires: &["url_engine"],
        holders: &["URL", "URLPattern", "URLSearchParams"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_text",
        requires: &["typedarray"],
        holders: &[
            "TextDecoder",
            "TextDecoderStream",
            "TextEncoder",
            "TextEncoderStream",
        ],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_websocket",
        requires: &["event_target", "turnloop", "typedarray"],
        holders: &["WebSocket"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_webcrypto",
        requires: &["event_target", "typedarray"],
        holders: &["Crypto", "CryptoKey", "SubtleCrypto", "crypto"],
        members: &[],
        modules: &[],
    },
    RuntimeFamily {
        id: "global_webfetch",
        requires: &["event_target", "turnloop", "typedarray"],
        holders: &[
            "Blob",
            "CompressionStream",
            "DecompressionStream",
            "File",
            "FormData",
            "Headers",
            "ReadableByteStreamController",
            "ReadableStream",
            "ReadableStreamBYOBReader",
            "ReadableStreamBYOBRequest",
            "ReadableStreamDefaultController",
            "ReadableStreamDefaultReader",
            "Request",
            "Response",
            "TransformStream",
            "WritableStream",
            "fetch",
        ],
        members: &[],
        modules: &["node-fetch", "undici"],
    },
    RuntimeFamily {
        id: "proc_ipc",
        requires: &[
            "bigint",
            "date",
            "os",
            "regex_engine",
            "thread",
            "turnloop",
            "typedarray",
        ],
        holders: &[],
        members: &[
            "process.channel",
            "process.connected",
            "process.disconnect",
            "process.send",
        ],
        modules: &["child_process", "cluster"],
    },
    RuntimeFamily {
        id: "mod_dgram",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &["dgram"],
    },
    RuntimeFamily {
        id: "mod_http2_constants",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &["http2"],
    },
    RuntimeFamily {
        id: "mod_node_test",
        requires: &[],
        holders: &[],
        members: &[],
        modules: &["test", "test/reporters"],
    },
    RuntimeFamily {
        id: "node_api_host",
        requires: &["bigint", "os", "turnloop"],
        holders: &[],
        members: &["process.dlopen"],
        modules: &[],
    },
    RuntimeFamily {
        id: "wasm_host",
        requires: &["bigint", "typedarray"],
        holders: &["WebAssembly"],
        members: &[],
        modules: &["wasi"],
    },
];

/// Realm holders (root names) of core: in no family, always linked.
pub const CORE_HOLDERS: &[&str] = &[
    "%ArrayIteratorPrototype%",
    "%AsyncFunctionPrototype%",
    "%AsyncGeneratorFunctionPrototype%",
    "%AsyncGeneratorPrototype%",
    "%AsyncIteratorPrototype%",
    "%FsStatsPredicate%",
    "%FsWatcherClose%",
    "%FsWatcherOff%",
    "%FsWatcherOn%",
    "%FsWatcherOnce%",
    "%FsWatcherRef%",
    "%FsWatcherUnref%",
    "%GeneratorFunctionPrototype%",
    "%GeneratorPrototype%",
    "%Global%",
    "%IteratorPrototype%",
    "%MapIteratorPrototype%",
    "%SetIteratorPrototype%",
    "%StatWatcherOff%",
    "%StatWatcherOn%",
    "%StatWatcherOnce%",
    "%StatWatcherRef%",
    "%StatWatcherUnref%",
    "%StringIteratorPrototype%",
    "AggregateError",
    "Array",
    "AsyncDisposableStack",
    "Boolean",
    "DisposableStack",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Function",
    "Infinity",
    "Map",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "RangeError",
    "ReferenceError",
    "Set",
    "String",
    "SuppressedError",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "atob",
    "btoa",
    "clearImmediate",
    "clearInterval",
    "clearTimeout",
    "console",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "eval",
    "gc",
    "global",
    "globalThis",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "process",
    "queueMicrotask",
    "setImmediate",
    "setInterval",
    "setTimeout",
    "undefined",
    "unescape",
];

/// (`util/types` is in no list of core: its predicates name the families
/// they test, and lowering routes `ArrayBuffer.isView` through it.)
///
/// Module specifiers (without `node:`) whose namespace is core: no family
/// row names them and their realm namespace declares no family birth that a
/// row would have to carry. A specifier in neither list is an unknown
/// module: every family is live.
pub const CORE_MODULES: &[&str] = &[
    "__disposable__",
    "assert",
    "assert/strict",
    "console",
    "constants",
    "domain",
    "path",
    "path.posix",
    "path.win32",
    "path/posix",
    "path/win32",
    "perry",
    "perry/gc",
    "perry/i18n",
    "perry/native",
    "perry/system",
    "punycode",
    "querystring",
    "sys",
    "timers",
    "timers/promises",
    "trace_events",
    "util",
];

/// Module specifiers (without `node:`) the realm declares whose values no
/// row describes (native libraries outside the runtime, UI toolkits):
/// every family is live in a program that names one.
pub const OPEN_MODULES: &[&str] = &[
    "@parcel/watcher",
    "@parcel/watcher-darwin-arm64",
    "@parcel/watcher-darwin-x64",
    "@parcel/watcher-linux-arm64-glibc",
    "@parcel/watcher-linux-arm64-musl",
    "@parcel/watcher-linux-x64-glibc",
    "@parcel/watcher-linux-x64-musl",
    "@parcel/watcher-win32-arm64",
    "@parcel/watcher-win32-x64",
    "@perryts/pdf",
    "argon2",
    "bcrypt",
    "cheerio",
    "ethers",
    "ffi",
    "lodash",
    "nodemailer",
    "perry/ads",
    "perry/audio",
    "perry/background",
    "perry/component",
    "perry/compose",
    "perry/container",
    "perry/container-compose",
    "perry/ios",
    "perry/media",
    "perry/plugin",
    "perry/tui",
    "perry/ui",
    "perry/updater",
    "perry/widget",
    "perry/workloads",
    "perry/yoga",
    "sharp",
    "streams",
    "typescript",
];

/// The index of family `id` in [`RUNTIME_FAMILIES`].
pub fn family_index(id: &str) -> Option<usize> {
    RUNTIME_FAMILIES.iter().position(|f| f.id == id)
}

/// The set holding family `id` (empty for a name that is no family).
pub fn family_bit(id: &str) -> FamilySet {
    family_index(id).map_or(0, |i| 1 << i)
}

/// Every family.
pub fn all_families() -> FamilySet {
    (1u64 << RUNTIME_FAMILIES.len()) - 1
}

/// The install symbol of a family.
pub fn install_symbol(family: &RuntimeFamily) -> String {
    format!("js_runtime_install_{}", family.id)
}

/// The holder an entry name is rooted at: `%TypedArray%.prototype.set` is
/// rooted at `%TypedArray%`, `node:fs.readFileSync` at `node:fs`,
/// `Buffer["@@species"].<get>` at `Buffer`.
pub fn entry_root(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix('%') {
        if let Some(end) = rest.find('%') {
            // `%NativeNamespace:buffer.Buffer%` names module `buffer`.
            return &name[..end + 2];
        }
    }
    let end = name.find(['.', '[']).unwrap_or(name.len());
    &name[..end]
}

/// The module a realm entry name belongs to (`node:fs.readFileSync`,
/// `%NativeNamespace:fs%`): its specifier without `node:`.
pub fn entry_module(name: &str) -> Option<&str> {
    if let Some(rest) = name.strip_prefix("node:") {
        // Module specifiers contain `/`, `:`, `-`, `@` and `_`, and a `.`
        // only in the dotted aliases the realm lists whole
        // (`path.posix`, `util.types`).
        for dotted in ["path.posix", "path.win32", "util.types"] {
            if rest == dotted || rest.starts_with(&format!("{dotted}.")) {
                return Some(&rest[..dotted.len()]);
            }
        }
        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    let rest = name.strip_prefix("%NativeNamespace:")?;
    let end = rest.find(['.', '%']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The families of module specifier `spec` (with or without `node:`);
/// every family for one of [`OPEN_MODULES`]. `None`: a module no list here
/// names.
pub fn module_families(spec: &str) -> Option<FamilySet> {
    let spec = spec.strip_prefix("node:").unwrap_or(spec);
    if OPEN_MODULES.contains(&spec) {
        return Some(all_families());
    }
    let mut set = 0;
    for (i, f) in RUNTIME_FAMILIES.iter().enumerate() {
        if f.modules.contains(&spec) {
            set |= 1 << i;
        }
    }
    (set != 0 || CORE_MODULES.contains(&spec)).then_some(set)
}

/// The families realm entry `name` is of; empty: core. `None`: the name is
/// rooted at a holder or module this table does not classify.
pub fn entry_families(name: &str) -> Option<FamilySet> {
    if let Some(module) = entry_module(name) {
        return module_families(module);
    }
    let root = entry_root(name);
    let mut set = 0;
    let mut known = CORE_HOLDERS.contains(&root) || root.starts_with("%Layout:");
    for (i, f) in RUNTIME_FAMILIES.iter().enumerate() {
        if f.holders.contains(&root) {
            set |= 1 << i;
            known = true;
        }
        if f.members.iter().any(|m| {
            name == *m
                || name
                    .strip_prefix(m)
                    .is_some_and(|rest| rest.starts_with(['.', '[']))
        }) {
            set |= 1 << i;
        }
    }
    known.then_some(set)
}

/// `set` closed over `requires`.
pub fn close_requires(mut set: FamilySet) -> FamilySet {
    loop {
        let mut next = set;
        for (i, f) in RUNTIME_FAMILIES.iter().enumerate() {
            if set & (1 << i) != 0 {
                for r in f.requires {
                    next |= family_bit(r);
                }
            }
        }
        if next == set {
            return set;
        }
        set = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_well_formed() {
        assert!(RUNTIME_FAMILIES.len() <= 64);
        for (i, f) in RUNTIME_FAMILIES.iter().enumerate() {
            assert_eq!(
                family_index(f.id),
                Some(i),
                "{}: a family id is unique",
                f.id
            );
            for r in f.requires {
                assert!(family_index(r).is_some(), "{} requires unknown {r}", f.id);
                assert_ne!(*r, f.id);
            }
            for h in f.holders {
                assert!(
                    !CORE_HOLDERS.contains(h),
                    "{h} is a holder of {} and of core",
                    f.id
                );
                assert_eq!(entry_root(h), *h, "{h}: a holder is a root name");
            }
            for m in f.modules {
                assert!(
                    !CORE_MODULES.contains(m),
                    "{m} is a module of {} and of core",
                    f.id
                );
            }
            for lists in [f.holders, f.members, f.modules, f.requires] {
                assert!(
                    lists.windows(2).all(|w| w[0] < w[1]),
                    "{}: lists are sorted",
                    f.id
                );
            }
        }
        assert!(CORE_HOLDERS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn entries_map_to_their_families() {
        let of = |name: &str| entry_families(name).unwrap();
        assert_eq!(of("Uint8Array.prototype.set"), family_bit("typedarray"));
        assert_eq!(of("%TypedArray%.prototype"), family_bit("typedarray"));
        assert_eq!(
            of("Buffer.alloc"),
            family_bit("typedarray") | family_bit("bigint")
        );
        assert_eq!(of("Array.prototype.map"), 0);
        assert_eq!(
            of("String.prototype.normalize"),
            family_bit("string_normalize")
        );
        assert_eq!(of("String.prototype.normalizeX"), 0);
        assert_eq!(of("String.prototype.split"), 0);
        assert_eq!(of("node:lodash.chunk"), all_families());
        assert_eq!(of("process.argv"), 0);
        assert_eq!(of("process.report.getReport"), family_bit("diagnostics"));
        assert_eq!(of("%Layout:ITERATOR_RESULT%"), 0);
        assert_eq!(of("node:path.join"), 0);
        assert_ne!(of("node:fs.readFileSync") & family_bit("typedarray"), 0);
        assert_ne!(
            of("%NativeNamespace:buffer.Buffer%") & family_bit("typedarray"),
            0
        );
        assert_eq!(of("node:path.posix.join"), 0);
        assert_eq!(entry_families("NoSuchHolder.x"), None);
        assert_eq!(entry_families("node:no-such-module.x"), None);
        assert_eq!(entry_root("Buffer[\"@@species\"].<get>"), "Buffer");
    }

    #[test]
    fn requires_close_transitively() {
        let closed = close_requires(family_bit("perf_hooks"));
        for id in ["perf_hooks", "event_target", "thread", "typedarray"] {
            assert_ne!(closed & family_bit(id), 0, "{id}");
        }
        assert_eq!(closed & family_bit("regex_engine"), 0);
        assert_eq!(close_requires(0), 0);
    }
}
