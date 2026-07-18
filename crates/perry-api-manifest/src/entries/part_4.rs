//! `API_MANIFEST` entries, part 4. Split out of entries.rs to satisfy the
//! 2000-line file-size gate; concatenated at compile time by the parent.
//!
//! `use super::*` pulls in the parent's type imports and the const-fn entry
//! builders (`method`/`property`/`class`/…) — children can name an ancestor's
//! private items, so the builders need no visibility change.

use super::*;

pub(crate) const API_MANIFEST_PART_4: &[ApiEntry] = &[
    method("v8", "cachedDataVersionTag", false, None),
    class("v8", "GCProfiler"),
    method("v8", "start", true, Some("GCProfiler")),
    method("v8", "stop", true, Some("GCProfiler"))
        .stub_note("report has the Node shape but the statistics array is always empty (#4916)"),
    // #3680: class-based serialization. Serializer / Deserializer plus the
    // Default* subclasses, with their write*/read* instance methods.
    class("v8", "Serializer"),
    class("v8", "DefaultSerializer"),
    class("v8", "Deserializer"),
    class("v8", "DefaultDeserializer"),
    method("v8", "writeHeader", true, Some("Serializer")),
    method("v8", "writeValue", true, Some("Serializer")),
    method("v8", "writeUint32", true, Some("Serializer")),
    method("v8", "writeUint64", true, Some("Serializer")),
    method("v8", "writeDouble", true, Some("Serializer")),
    method("v8", "writeRawBytes", true, Some("Serializer")),
    method("v8", "releaseBuffer", true, Some("Serializer")),
    method("v8", "readHeader", true, Some("Deserializer")),
    method("v8", "readValue", true, Some("Deserializer")),
    method("v8", "readUint32", true, Some("Deserializer")),
    method("v8", "readUint64", true, Some("Deserializer")),
    method("v8", "readDouble", true, Some("Deserializer")),
    method("v8", "readRawBytes", true, Some("Deserializer")),
    // #3679: lifecycle namespaces + diagnostic-control helpers.
    property("v8", "startupSnapshot"),
    property("v8", "promiseHooks"),
    method("v8", "setFlagsFromString", false, None),
    method("v8", "takeCoverage", false, None),
    method("v8", "stopCoverage", false, None),
    method("v8", "setHeapSnapshotNearHeapLimit", false, None),
    // #3904: modern V8 diagnostics/profiler named exports (function-valued in
    // Node's ESM namespace). `getHeapSnapshot`/`writeHeapSnapshot` deeper
    // behavior is tracked by #3140; here they're added to the export surface.
    method("v8", "getCppHeapStatistics", false, None),
    method("v8", "getHeapSnapshot", false, None),
    method("v8", "isStringOneByteRepresentation", false, None),
    method("v8", "queryObjects", false, None),
    method("v8", "startCpuProfile", false, None),
    method("v8", "writeHeapSnapshot", false, None),
    method("v8", "isBuildingSnapshot", true, Some("startupSnapshot")),
    method("v8", "addSerializeCallback", true, Some("startupSnapshot")),
    method(
        "v8",
        "addDeserializeCallback",
        true,
        Some("startupSnapshot"),
    ),
    method(
        "v8",
        "setDeserializeMainFunction",
        true,
        Some("startupSnapshot"),
    ),
    method("v8", "onInit", true, Some("promiseHooks")),
    method("v8", "onBefore", true, Some("promiseHooks")),
    method("v8", "onAfter", true, Some("promiseHooks")),
    method("v8", "onSettled", true, Some("promiseHooks")),
    method("v8", "createHook", true, Some("promiseHooks")),
    // --- node:vm scaffold (#3127/#3128/#3130/#3284/#3321/#3323) ---
    // Perry exposes the no-flag Node import/require shape here: Script,
    // callable top-level helpers, and vm.constants. VM module classes
    // (Module/SourceTextModule/SyntheticModule) stay out of the default
    // public manifest because Node only exposes them with
    // --experimental-vm-modules.
    class("vm", "Script"),
    // createContext is registered above via method_sig (#4050).
    method("vm", "createScript", false, None),
    method("vm", "runInContext", false, None),
    method("vm", "runInNewContext", false, None),
    method("vm", "runInThisContext", false, None),
    method("vm", "isContext", false, None),
    method("vm", "compileFunction", false, None),
    method("vm", "measureMemory", false, None),
    property("vm", "constants"),
    property("vm", "default"),
    // Experimental VM module rows are gated at runtime and are not public
    // no-flag named exports, but the codegen dispatch table still needs
    // manifest counterparts for the lifecycle/cached-data methods.
    internal_method("vm", "Module", false, None),
    internal_method("vm", "SourceTextModule", false, None),
    internal_method("vm", "SyntheticModule", false, None),
    internal_method("vm", "status", true, None),
    internal_method("vm", "identifier", true, None),
    internal_method("vm", "error", true, None),
    internal_method("vm", "namespace", true, None),
    internal_method("vm", "dependencySpecifiers", true, None),
    internal_method("vm", "moduleRequests", true, None),
    internal_method("vm", "link", true, None),
    internal_method("vm", "evaluate", true, None),
    internal_method("vm", "createCachedData", true, None),
    internal_method("vm", "linkRequests", true, None),
    internal_method("vm", "instantiate", true, None),
    internal_method("vm", "hasTopLevelAwait", true, None),
    internal_method("vm", "hasAsyncGraph", true, None),
    internal_method("vm", "setExport", true, None),
    // --- buffer (module-level helpers in addition to the Buffer class
    //     already registered above) ---
    internal_method("buffer", "alloc", false, None),
    internal_method("buffer", "allocUnsafe", false, None),
    internal_method("buffer", "allocUnsafeSlow", false, None),
    internal_method("buffer", "from", false, None),
    internal_method("buffer", "of", false, None),
    internal_method("buffer", "concat", false, None),
    internal_method("buffer", "copyBytesFrom", false, None),
    // #2901: TC39 `Uint8Array.fromBase64` / `fromHex` static factories,
    // routed through the buffer module (Uint8Array ≡ Buffer in Perry).
    internal_method("buffer", "fromBase64", false, None),
    internal_method("buffer", "fromHex", false, None),
    internal_method("buffer", "isBuffer", false, None),
    internal_method("buffer", "isEncoding", false, None),
    internal_method("buffer", "byteLength", false, None),
    // Issue #800: WHATWG base64 aliases exposed from node:buffer.
    method("buffer", "atob", false, None),
    method("buffer", "btoa", false, None),
    // Buffer module-level encoding probes added in PR #1257.
    method("buffer", "isAscii", false, None),
    method("buffer", "isUtf8", false, None),
    // Issue #1210: re-encode bytes between supported encodings.
    method("buffer", "transcode", false, None),
    // Issue #1211: Blob / File constructors + object-URL helpers
    // exposed from node:buffer.  Blob/File constructors are recognized
    // by the codegen builtin path, so they only need to appear here
    // as class exports.
    class("buffer", "Blob"),
    class("buffer", "File"),
    method("buffer", "resolveObjectURL", false, None),
    property("buffer", "constants"),
    property("buffer", "INSPECT_MAX_BYTES"),
    property("buffer", "kMaxLength"),
    property("buffer", "kStringMaxLength"),
    // --- url (additional helpers) ---
    property("url", "default"),
    method("url", "fileURLToPath", false, None),
    method("url", "fileURLToPathBuffer", false, None),
    method("url", "pathToFileURL", false, None),
    method("url", "domainToASCII", false, None),
    method("url", "domainToUnicode", false, None),
    method("url", "urlToHttpOptions", false, None),
    class("url", "Url"),
    method("url", "Url", false, None),
    method("url", "format", false, None),
    method("url", "parse", false, None),
    method("url", "resolve", false, None),
    method("url", "resolveObject", false, None),
    // Issue #1211: Blob/File object-URL registry — paired with the
    // `resolveObjectURL` export on `node:buffer`.
    internal_method("url", "createObjectURL", false, None),
    internal_method("url", "revokeObjectURL", false, None),
    // --- punycode (deprecated module, #2513). Top-level string helpers,
    //     Node's CJS default export, and the `version` property. ---
    property("punycode", "default"),
    method("punycode", "decode", false, None),
    method("punycode", "encode", false, None),
    method("punycode", "toASCII", false, None),
    method("punycode", "toUnicode", false, None),
    property("punycode", "version"),
    // #2607: the `ucs2` code-point helper sub-namespace. The sub-namespace
    // object is a `property` on `punycode`; its `decode`/`encode` methods carry
    // the internal `punycode.ucs2` dispatch key. Node does not expose
    // `node:punycode.ucs2` as an importable builtin module.
    property("punycode", "ucs2"),
    internal_method("punycode.ucs2", "decode", false, None),
    internal_method("punycode.ucs2", "encode", false, None),
    // --- http (perry-ext-http surface + classes the framework spec
    //     exposes). Both http and https route through the same crate. ---
    method("http", "createServer", false, None),
    // `http.Server(handler)` is Node's callable-constructor alias for
    // `createServer` (works with or without `new`). #2132.
    method("http", "Server", false, None),
    method("http", "request", false, None),
    method("http", "get", false, None),
    property("http", "METHODS"),
    property("http", "STATUS_CODES"),
    // #3712 — module-level helper/export tail. `maxHeaderSize` is the 16 KiB
    // default constant; `globalAgent` is the shared http.Agent; the four
    // helpers validate header tokens/values or are deterministic no-ops.
    property("http", "maxHeaderSize"),
    property("http", "globalAgent"),
    // #4974 — `require('_http_server').kConnectionsCheckingInterval`
    // (Perry aliases `_http_server` to `http`). Node exports a Symbol
    // tests use as `server[k]._destroyed`; Perry resolves it to the
    // sentinel key the server handle dispatch recognizes.
    property("http", "kConnectionsCheckingInterval"),
    method("http", "validateHeaderName", false, None),
    method("http", "validateHeaderValue", false, None),
    method("http", "setMaxIdleHTTPParsers", false, None),
    method("http", "setGlobalProxyFromEnv", false, None),
    method("http", "_connectionListener", false, None),
    class("http", "Server"),
    class("http", "WebSocket"),
    class("http", "ClientRequest"),
    class("http", "IncomingMessage"),
    class("http", "OutgoingMessage"),
    class("http", "ServerResponse"),
    // #2129 — `new http.Agent(options?)`. Construction is unconditional;
    // method dispatch flows through ("http", "Agent") rows below.
    class("http", "Agent"),
    method("http", "Agent", false, None),
    method("http", "getName", true, Some("Agent")),
    // #4917 — `destroy()` really drops the per-agent reqwest client (=
    // releases its keep-alive pool) and flips `destroyed`; not a stub.
    method("http", "destroy", true, Some("Agent")),
    method("http", "close", true, Some("Agent")),
    method("http", "keepSocketAlive", true, Some("Agent")).stub_note(
        "reqwest owns the keep-alive pool; per-socket hooks are no-ops, warns once (#4917)",
    ),
    method("http", "reuseSocket", true, Some("Agent")).stub_note(
        "reqwest owns the keep-alive pool; per-socket hooks are no-ops, warns once (#4917)",
    ),
    // Synthetic `__get_<name>` / `__set_<name>` accessor methods (HIR
    // rewrites bare `agent.maxSockets` reads to `__get_maxSockets()`
    // when the receiver is class-tagged) + their bare-name twins for
    // sites where the rewrite doesn't fire. Keep parity with the rows
    // in `crates/perry-codegen/src/lower_call/native_table/http.rs`
    // (drift caught by perry-codegen/tests/manifest_consistency.rs).
    method("http", "__get_maxSockets", true, Some("Agent")),
    method("http", "__get_maxFreeSockets", true, Some("Agent")),
    method("http", "__get_maxTotalSockets", true, Some("Agent")),
    method("http", "__get_keepAliveMsecs", true, Some("Agent")),
    method("http", "__get_keepAlive", true, Some("Agent")),
    method("http", "__get_protocol", true, Some("Agent")),
    method("http", "__get_defaultPort", true, Some("Agent")),
    method("http", "__set_protocol", true, Some("Agent")),
    method("http", "maxSockets", true, Some("Agent")),
    method("http", "maxFreeSockets", true, Some("Agent")),
    method("http", "maxTotalSockets", true, Some("Agent")),
    method("http", "keepAliveMsecs", true, Some("Agent")),
    method("http", "keepAlive", true, Some("Agent")),
    method("http", "protocol", true, Some("Agent")),
    method("http", "defaultPort", true, Some("Agent")),
    // #2154 — sockets/freeSockets/requests accessors return `{}` for an
    // idle agent; destroyed reflects whether `.destroy()` has been
    // called; the `__set_*` rows enforce ERR_OUT_OF_RANGE on invalid
    // writes (matches Node's `_http_agent.js` setter behavior);
    // createConnection / createSocket closure pointers round-trip.
    method("http", "__get_sockets", true, Some("Agent")),
    method("http", "sockets", true, Some("Agent")),
    method("http", "__get_freeSockets", true, Some("Agent")),
    method("http", "freeSockets", true, Some("Agent")),
    method("http", "__get_requests", true, Some("Agent")),
    method("http", "requests", true, Some("Agent")),
    method("http", "__get_destroyed", true, Some("Agent")),
    method("http", "destroyed", true, Some("Agent")),
    method("http", "__set_maxSockets", true, Some("Agent")),
    method("http", "__set_maxFreeSockets", true, Some("Agent")),
    method("http", "__set_maxTotalSockets", true, Some("Agent")),
    method("http", "__set_keepAlive", true, Some("Agent")),
    method("http", "__set_keepAliveMsecs", true, Some("Agent")),
    method("http", "__set_createConnection", true, Some("Agent")),
    method("http", "__set_createSocket", true, Some("Agent")),
    method("http", "__get_createConnection", true, Some("Agent")),
    method("http", "__get_createSocket", true, Some("Agent")),
    method("https", "createServer", false, None),
    // `https.Server(options, handler)` is Node's callable-constructor
    // alias for `createServer` (works with or without `new`). #2132.
    method("https", "Server", false, None),
    method("https", "request", false, None),
    method("https", "get", false, None),
    property("https", "globalAgent"),
    class("https", "Server"),
    internal_class("https", "ClientRequest"),
    internal_class("https", "IncomingMessage"),
    internal_class("https", "ServerResponse"),
    // #2129 — `new https.Agent(options?)`. The instance is tagged as
    // ("http", "Agent") in destructuring/var_decl.rs so it shares the
    // method surface; only the constructor's default protocol differs.
    class("https", "Agent"),
    method("https", "Agent", false, None),
    // --- axios (perry-ext-axios) — the npm `axios` HTTP client surface.
    //     The default export is callable (`axios(config)`); both flow
    //     through perry-ext-axios's `js_axios_*` symbols. ---
    method("axios", "default", false, None),
    method("axios", "get", false, None),
    method("axios", "post", false, None),
    method("axios", "put", false, None),
    method("axios", "delete", false, None),
    method("axios", "patch", false, None),
    method("axios", "head", false, None),
    method("axios", "options", false, None),
    method("axios", "request", false, None),
    method("axios", "create", false, None),
    method("axios", "all", false, None),
    // --- node-fetch (perry-ext-fetch) — also exposes the Web Fetch
    //     API classes (Headers, Request, Response, Blob, FormData). ---
    method("node-fetch", "default", false, None),
    class("node-fetch", "Headers"),
    class("node-fetch", "Request"),
    class("node-fetch", "Response"),
    class("node-fetch", "Blob"),
    class("node-fetch", "FormData"),
    // --- bignumber.js — alias surface for decimal.js. The wrapper
    //     dispatches to the same perry-ext-decimal implementation. ---
    class("bignumber.js", "BigNumber"),
    // --- node-cron — alias for the cron wrapper.
    method("node-cron", "schedule", false, None),
    method("node-cron", "validate", false, None),
    // --- perry/ui constructors + setters. Auto-derivable from
    //     PERRY_UI_TABLE in crates/perry-dispatch/src/lib.rs. The
    //     reverse drift test enforces parity in both directions. ---
    method("perry/ui", "App", false, None),
    method("perry/ui", "Window", false, None),
    method("perry/ui", "VStack", false, None),
    method("perry/ui", "HStack", false, None),
    method("perry/ui", "ZStack", false, None),
    method("perry/ui", "Section", false, None),
    method("perry/ui", "Spacer", false, None),
    method("perry/ui", "Divider", false, None),
    method("perry/ui", "ScrollView", false, None),
    method("perry/ui", "Text", false, None),
    // Issue #710 — AttributedText (per-range styling)
    method("perry/ui", "AttributedText", false, None),
    method("perry/ui", "attributedTextAppend", false, None),
    method("perry/ui", "attributedTextClear", false, None),
    method("perry/ui", "TextField", false, None),
    method("perry/ui", "TextArea", false, None),
    method("perry/ui", "SecureField", false, None),
    method("perry/ui", "Button", false, None),
    method("perry/ui", "Toggle", false, None),
    method("perry/ui", "Slider", false, None),
    method("perry/ui", "ProgressView", false, None),
    method("perry/ui", "Picker", false, None),
    method("perry/ui", "ImageFile", false, None),
    method("perry/ui", "ImageSymbol", false, None),
    method("perry/ui", "loadImage", false, None),
    method("perry/ui", "Image", false, None),
    method("perry/ui", "LazyVStack", false, None),
    method("perry/ui", "NavStack", false, None),
    method("perry/ui", "TabBar", false, None),
    // Issue #553 — production-mobile widgets
    method("perry/ui", "BottomNavigation", false, None),
    method("perry/ui", "bottomNavAddItem", false, None),
    method("perry/ui", "bottomNavSetBadge", false, None),
    method("perry/ui", "bottomNavSetSelected", false, None),
    method("perry/ui", "bottomNavSetTintColor", false, None),
    method("perry/ui", "bottomNavSetUnselectedTintColor", false, None),
    method("perry/ui", "ImageGallery", false, None),
    method("perry/ui", "imageGalleryAddImage", false, None),
    method("perry/ui", "imageGallerySetIndex", false, None),
    // Issue #658 — WebView (auth flows / payments / embedded HTML)
    method("perry/ui", "WebView", false, None),
    method("perry/ui", "webviewLoadUrl", false, None),
    method("perry/ui", "webviewReload", false, None),
    method("perry/ui", "webviewGoBack", false, None),
    method("perry/ui", "webviewGoForward", false, None),
    method("perry/ui", "webviewCanGoBack", false, None),
    method("perry/ui", "webviewEvaluateJs", false, None),
    method("perry/ui", "webviewClearCookies", false, None),
    method("perry/ui", "scrollviewSetScrollEndCallback", false, None),
    method("perry/ui", "scrollViewSetScrollEndCallback", false, None),
    method("perry/ui", "lazyvstackSetRefreshControl", false, None),
    method("perry/ui", "lazyvstackEndRefreshing", false, None),
    method("perry/ui", "lazyvstackSetScrollEndCallback", false, None),
    method("perry/ui", "Table", false, None),
    method("perry/ui", "Canvas", false, None),
    // Issue #2395 / #5519 — BloomView (embed an external GPU renderer / Bloom engine)
    method("perry/ui", "BloomView", false, None),
    method("perry/ui", "bloomViewGetNativeHandle", false, None),
    // Deprecated alias for bloomViewGetNativeHandle (#5519).
    method("perry/ui", "bloomViewGetHwnd", false, None),
    method("perry/ui", "CameraView", false, None),
    method("perry/ui", "cameraStart", false, None),
    method("perry/ui", "cameraStop", false, None),
    method("perry/ui", "cameraFreeze", false, None),
    method("perry/ui", "cameraUnfreeze", false, None),
    method("perry/ui", "cameraSampleColor", false, None),
    method("perry/ui", "cameraSetOnTap", false, None),
    method("perry/ui", "cameraRegisterFrameCallback", false, None),
    method("perry/ui", "cameraUnregisterFrameCallback", false, None),
    method("perry/ui", "SplitView", false, None),
    method("perry/ui", "ForEach", false, None),
    method("perry/ui", "State", false, None),
    method("perry/ui", "VStackWithInsets", false, None),
    method("perry/ui", "HStackWithInsets", false, None),
    method("perry/ui", "showToast", false, None),
    method("perry/ui", "setText", false, None),
    method("perry/ui", "alert", false, None),
    method("perry/ui", "alertWithButtons", false, None),
    method("perry/ui", "menuCreate", false, None),
    method("perry/ui", "menuAddItem", false, None),
    method("perry/ui", "menuAddSeparator", false, None),
    method("perry/ui", "menuAddSubmenu", false, None),
    method("perry/ui", "menuAddStandardAction", false, None),
    method("perry/ui", "menuAddItemWithShortcut", false, None),
    method("perry/ui", "menuClear", false, None),
    method("perry/ui", "menuBarCreate", false, None),
    method("perry/ui", "menuBarAddMenu", false, None),
    method("perry/ui", "menuBarAttach", false, None),
    method("perry/ui", "trayCreate", false, None),
    method("perry/ui", "traySetIcon", false, None),
    method("perry/ui", "traySetTooltip", false, None),
    method("perry/ui", "trayAttachMenu", false, None),
    method("perry/ui", "trayOnClick", false, None),
    method("perry/ui", "trayDestroy", false, None),
    method("perry/ui", "toolbarCreate", false, None),
    method("perry/ui", "toolbarAddItem", false, None),
    method("perry/ui", "toolbarAttach", false, None),
    method("perry/ui", "openFileDialog", false, None),
    method("perry/ui", "openFolderDialog", false, None),
    method("perry/ui", "saveFileDialog", false, None),
    method("perry/ui", "pollOpenFile", false, None),
    method("perry/ui", "clipboardRead", false, None),
    method("perry/ui", "clipboardWrite", false, None),
    method("perry/ui", "addKeyboardShortcut", false, None),
    method("perry/ui", "registerGlobalHotkey", false, None),
    // Continuous keyboard events (issue #1864).
    method("perry/ui", "onKeyDown", false, None),
    method("perry/ui", "onKeyUp", false, None),
    method("perry/ui", "onAppKeyDown", false, None),
    method("perry/ui", "onAppKeyUp", false, None),
    method("perry/ui", "focus", false, None),
    method("perry/ui", "blur", false, None),
    method("perry/ui", "isKeyDown", false, None),
    method("perry/ui", "currentModifiers", false, None),
    method("perry/ui", "onTerminate", false, None),
    method("perry/ui", "onActivate", false, None),
    method("perry/ui", "appSetTimer", false, None),
    method("perry/ui", "appSetMinSize", false, None),
    method("perry/ui", "appSetMaxSize", false, None),
    method("perry/ui", "appSetActivationPolicy", false, None),
    method("perry/ui", "embedNSView", false, None),
    method("perry/ui", "sheetCreate", false, None),
    method("perry/ui", "sheetPresent", false, None),
    method("perry/ui", "sheetDismiss", false, None),
    method("perry/ui", "frameSplitCreate", false, None),
    method("perry/ui", "frameSplitAddChild", false, None),
    // --- perry/system — auto-derivable from PERRY_SYSTEM_TABLE. ---
    method("perry/system", "isDarkMode", false, None),
    method("perry/system", "getDeviceIdiom", false, None),
    method("perry/system", "getSafeAreaInsets", false, None),
    method("perry/system", "getDeviceModel", false, None),
    // Bug-report-flow utility: stable OS-version string per
    // platform (e.g. `"15.2"`, `"macOS 14.5"`, `"Android 14"`).
    // Common need for crash reports and telemetry; pairs with
    // getDeviceModel / getAppVersion.
    method("perry/system", "getOSVersion", false, None),
    method("perry/system", "getLocale", false, None),
    method("perry/system", "getAppVersion", false, None),
    method("perry/system", "getAppBuildNumber", false, None),
    method("perry/system", "getBundleId", false, None),
    method("perry/system", "getAppIcon", false, None),
    method("perry/system", "openURL", false, None),
    // #917 — system share sheet (UIActivityViewController on iOS,
    // NSSharingServicePicker on macOS, Intent.ACTION_SEND on
    // Android). Two convenience entry points cover the common
    // shapes: plain text + URL.
    method("perry/system", "shareText", false, None),
    method("perry/system", "shareUrl", false, None),
    // #675 — App Group / cross-process shared storage. Widget
    // extensions, share extensions, watchOS targets, etc. all need
    // a way to share key/value data with the host app. macOS/iOS:
    // `UserDefaults(suiteName:)`. Android: scoped SharedPreferences
    // (follow-up). Every other platform: an in-process HashMap
    // fallback so the API surface is exercisable in dev/tests; not
    // actually cross-process there. Follow-up tracker: #675.
    method("perry/system", "appGroupSet", false, None),
    method("perry/system", "appGroupGet", false, None),
    method("perry/system", "appGroupDelete", false, None),
    method("perry/system", "keychainSave", false, None),
    method("perry/system", "keychainGet", false, None),
    method("perry/system", "keychainDelete", false, None),
    method("perry/system", "preferencesGet", false, None),
    method("perry/system", "preferencesSet", false, None),
    // Haptic feedback: hapticPlay("success" | "warning" | "error" |
    // "light" | "medium" | "heavy" | "click" | "selection" |
    // "directionUp" | "directionDown" | "start" | "stop"). Maps to
    // WKInterfaceDevice playHaptic: (watchOS), UIFeedbackGenerator
    // (iOS), VibrationEffect (Android), NSHapticFeedbackManager
    // (macOS), navigator.vibrate (Web); no-op on platforms without a
    // haptic engine (tvOS, visionOS, GTK4, Windows).
    method_sig(
        "perry/system",
        "hapticPlay",
        false,
        None,
        &[p_str("type")],
        TypeSpec::Void,
    ),
    method("perry/system", "notificationSend", false, None),
    method("perry/system", "notificationCancel", false, None),
    method("perry/system", "notificationOnTap", false, None),
    method("perry/system", "notificationOnReceive", false, None),
    method(
        "perry/system",
        "notificationOnBackgroundReceive",
        false,
        None,
    ),
    method("perry/system", "notificationRegisterRemote", false, None),
    method("perry/system", "audioStart", false, None),
    method("perry/system", "audioStop", false, None),
    method("perry/system", "audioGetLevel", false, None),
    method("perry/system", "audioGetPeak", false, None),
    method("perry/system", "audioGetWaveform", false, None),
    method("perry/system", "audioSetOutputFilename", false, None),
    method("perry/system", "audioRegisterCallback", false, None),
    method("perry/system", "audioUnregisterCallback", false, None),
    method("perry/system", "audioStartRecording", false, None),
    method("perry/system", "audioStopRecording", false, None),
    // --- perry/system geolocation + image picker (issue #552). ---
    method("perry/system", "geolocationGetCurrent", false, None),
    method("perry/system", "geolocationWatch", false, None),
    method("perry/system", "geolocationStopWatch", false, None),
    method("perry/system", "geolocationRequestPermission", false, None),
    method("perry/system", "imagePickerPick", false, None),
    // --- perry/system in-app screen capture (issue #918). ---
    method("perry/system", "takeScreenshot", false, None),
    // --- perry/system network reachability (issue #582). ---
    method("perry/system", "networkGetStatus", false, None),
    method("perry/system", "networkOnChange", false, None),
    method("perry/system", "networkStopOnChange", false, None),
    // --- perry/system deep links (issue #583). ---
    method("perry/system", "appOnOpenUrl", false, None),
    method("perry/system", "appGetLaunchUrl", false, None),
    // --- perry/background (issue #538) — BGTaskScheduler / WorkManager. ---
    method("perry/background", "registerTask", false, None),
    method("perry/background", "schedule", false, None),
    method("perry/background", "cancel", false, None),
    // --- perry/i18n — auto-derivable from PERRY_I18N_TABLE. ---
    method("perry/i18n", "t", false, None),
    method("perry/i18n", "Currency", false, None),
    method("perry/i18n", "Percent", false, None),
    method("perry/i18n", "FormatNumber", false, None),
    method("perry/i18n", "FormatTime", false, None),
    method("perry/i18n", "ShortDate", false, None),
    method("perry/i18n", "LongDate", false, None),
    method("perry/i18n", "Raw", false, None),
    // --- perry/updater — auto-derivable from PERRY_UPDATER_TABLE. ---
    method("perry/updater", "compareVersions", false, None),
    method("perry/updater", "verifyHash", false, None),
    method("perry/updater", "verifySignature", false, None),
    method("perry/updater", "verifySignatureV2", false, None),
    method("perry/updater", "computeFileSha256", false, None),
    method("perry/updater", "writeSentinel", false, None),
    method("perry/updater", "readSentinel", false, None),
    method("perry/updater", "clearSentinel", false, None),
    method("perry/updater", "getExePath", false, None),
    method("perry/updater", "getBackupPath", false, None),
    method("perry/updater", "getSentinelPath", false, None),
    method("perry/updater", "installUpdate", false, None),
    method("perry/updater", "performRollback", false, None),
    method("perry/updater", "relaunch", false, None),
    // --- perry/media — auto-derivable from PERRY_MEDIA_TABLE. ---
    method("perry/media", "createPlayer", false, None),
    method("perry/media", "play", false, None),
    method("perry/media", "pause", false, None),
    method("perry/media", "stop", false, None),
    method("perry/media", "seek", false, None),
    method("perry/media", "setVolume", false, None),
    method("perry/media", "setRate", false, None),
    method("perry/media", "getCurrentTime", false, None),
    method("perry/media", "getDuration", false, None),
    method("perry/media", "getState", false, None),
    method("perry/media", "isPlaying", false, None),
    method("perry/media", "onStateChange", false, None),
    method("perry/media", "onTimeUpdate", false, None),
    method("perry/media", "setNowPlaying", false, None),
    method("perry/media", "destroy", false, None),
    // --- perry/audio (issue #1867) — auto-derivable from PERRY_AUDIO_TABLE. ---
    method("perry/audio", "loadSound", false, None),
    method("perry/audio", "unload", false, None),
    method("perry/audio", "play", false, None),
    method("perry/audio", "stop", false, None),
    method("perry/audio", "pause", false, None),
    method("perry/audio", "resume", false, None),
    method("perry/audio", "setVolume", false, None),
    method("perry/audio", "setRate", false, None),
    method("perry/audio", "setPan", false, None),
    method("perry/audio", "fadeIn", false, None),
    method("perry/audio", "fadeOut", false, None),
    method("perry/audio", "crossfade", false, None),
    method("perry/audio", "createBus", false, None),
    method("perry/audio", "destroyBus", false, None),
    method("perry/audio", "muteBus", false, None),
    method("perry/audio", "soloBus", false, None),
    method("perry/audio", "setMasterVolume", false, None),
    method("perry/audio", "suspend", false, None),
    method("perry/audio", "resumeAll", false, None),
    method("perry/audio", "isPlaying", false, None),
    method("perry/audio", "getDuration", false, None),
    method("perry/audio", "getPosition", false, None),
    method("perry/audio", "onEnded", false, None),
    method("perry/audio", "onLoaded", false, None),
    // --- perry/container — OCI single-container + image lifecycle.
    //     Backed by the perry-container-compose crate's FFI exports
    //     (js_container_*). Auto-namespace module: signatures stay loose
    //     ((...args): any) — codegen NaN-boxes whatever is passed.
    //     Surface mirrors types/perry/container/index.d.ts. The entries
    //     flip strict mode (#463) on for the module so the
    //     unimplemented-API gate fires (#513). ---
    method("perry/container", "run", false, None),
    method("perry/container", "create", false, None),
    method("perry/container", "start", false, None),
    method("perry/container", "stop", false, None),
    method("perry/container", "remove", false, None),
    method("perry/container", "list", false, None),
    method("perry/container", "inspect", false, None),
    method("perry/container", "logs", false, None),
    method("perry/container", "exec", false, None),
    method("perry/container", "pullImage", false, None),
    method("perry/container", "listImages", false, None),
    method("perry/container", "removeImage", false, None),
    method("perry/container", "composeUp", false, None),
    method("perry/container", "downByProject", false, None),
    method("perry/container", "downAll", false, None),
    method("perry/container", "removeIfExists", false, None),
    method("perry/container", "getBackend", false, None),
    method("perry/container", "detectBackend", false, None),
    method("perry/container", "getAvailableBackends", false, None),
    method("perry/container", "setBackend", false, None),
    method("perry/container", "setBackends", false, None),
    method("perry/container", "getBackendPriority", false, None),
    method("perry/container", "selectBackendFor", false, None),
    // --- perry/compose — multi-service Compose orchestration. Same
    //     backend crate; surface mirrors types/perry/compose/index.d.ts. ---
    method("perry/compose", "up", false, None),
    method("perry/compose", "down", false, None),
    method("perry/compose", "ps", false, None),
    method("perry/compose", "logs", false, None),
    method("perry/compose", "exec", false, None),
    method("perry/compose", "config", false, None),
    method("perry/compose", "start", false, None),
    method("perry/compose", "stop", false, None),
    method("perry/compose", "restart", false, None),
    // --- perry/container-compose — internal specifier for the unified
    //     compose subsystem (crate perry-container-compose). Feature-mapped
    //     in stdlib_features.rs alongside the public perry/compose surface;
    //     entries mirror perry/compose so the unimplemented-API gate (#463)
    //     flips strict mode on for the module too. ---
    method("perry/container-compose", "up", false, None),
    method("perry/container-compose", "down", false, None),
    method("perry/container-compose", "ps", false, None),
    method("perry/container-compose", "logs", false, None),
    method("perry/container-compose", "exec", false, None),
    method("perry/container-compose", "config", false, None),
    method("perry/container-compose", "start", false, None),
    method("perry/container-compose", "stop", false, None),
    method("perry/container-compose", "restart", false, None),
    // --- perry/workloads — workload-graph orchestration. Surface mirrors
    //     types/perry/workloads/index.d.ts; `runtime` and `policy` are
    //     const helper-constructor objects (Property rows). ---
    method("perry/workloads", "graph", false, None),
    method("perry/workloads", "node", false, None),
    method("perry/workloads", "runGraph", false, None),
    method("perry/workloads", "inspectGraph", false, None),
    property("perry/workloads", "runtime"),
    property("perry/workloads", "policy"),
    // --- perry/plugin — host-side functions (PERRY_PLUGIN_TABLE in
    //     lower_call.rs). Instance methods on PluginApi are tracked on
    //     class_filter rows — see perry/plugin's PluginApi class. ---
    method("perry/plugin", "loadPlugin", false, None),
    method("perry/plugin", "unloadPlugin", false, None),
    method("perry/plugin", "emitHook", false, None),
    method("perry/plugin", "emitEvent", false, None),
    method("perry/plugin", "invokeTool", false, None),
    method("perry/plugin", "setPluginConfig", false, None),
    method("perry/plugin", "discoverPlugins", false, None),
    method("perry/plugin", "listPlugins", false, None),
    method("perry/plugin", "listHooks", false, None),
    method("perry/plugin", "listTools", false, None),
    method("perry/plugin", "pluginCount", false, None),
    method("perry/plugin", "initPlugins", false, None),
    class("perry/plugin", "PluginApi"),
    // --- perry/widget — declarative widget-extension entrypoint
    //     (iOS WidgetKit / Android home-screen widgets). One callable
    //     export `Widget(config)` produces a WidgetDecl in HIR; see
    //     try_lower_widget_decl in perry-hir/src/lower.rs. ---
    method("perry/widget", "Widget", false, None),
    // --- redis — alias for ioredis (well-known table routes both to
    //     perry-ext-ioredis). The Redis class instance methods come
    //     from the ioredis class entries. ---
    class("redis", "Redis"),
    method("redis", "createClient", false, None),
    // --- date-fns — alias for dayjs (well-known routes both to
    //     perry-ext-dayjs). Surface methods are the date-fns
    //     functional API exposed by the wrapper. ---
    method("date-fns", "format", false, None),
    method("date-fns", "parseISO", false, None),
    method("date-fns", "addDays", false, None),
    method("date-fns", "addMonths", false, None),
    method("date-fns", "addYears", false, None),
    method("date-fns", "differenceInDays", false, None),
    method("date-fns", "differenceInHours", false, None),
    method("date-fns", "differenceInMinutes", false, None),
    method("date-fns", "isAfter", false, None),
    method("date-fns", "isBefore", false, None),
    method("date-fns", "startOfDay", false, None),
    method("date-fns", "endOfDay", false, None),
    // --- rate-limiter-flexible — perry-ext-ratelimit. Surface mirrors
    //     the npm package's RateLimiterMemory class. Construction is a
    //     lower_builtin_new arm (js_ratelimit_new_from_options); the
    //     instance methods dispatch via the NATIVE_MODULE_TABLE rows in
    //     lower_call/native_table/extras.rs. ---
    class("rate-limiter-flexible", "RateLimiterMemory"),
    class("rate-limiter-flexible", "RateLimiterAbstract"),
    method("rate-limiter-flexible", "consume", true, None),
    method("rate-limiter-flexible", "get", true, None),
    method("rate-limiter-flexible", "delete", true, None),
    method("rate-limiter-flexible", "block", true, None),
    method("rate-limiter-flexible", "penalty", true, None),
    method("rate-limiter-flexible", "reward", true, None),
    // --- fetch — well-known alias for perry-ext-fetch. Same surface
    //     as node-fetch (the more common alias above). ---
    method("fetch", "default", false, None),
    class("fetch", "Headers"),
    class("fetch", "Request"),
    class("fetch", "Response"),
    class("fetch", "Blob"),
    class("fetch", "FormData"),
    // --- streams — Web Streams API umbrella (perry-ext-streams). ---
    class("streams", "ReadableStream"),
    class("streams", "WritableStream"),
    class("streams", "TransformStream"),
    class("streams", "TextEncoder"),
    class("streams", "TextDecoder"),
    class("streams", "DecompressionStream"),
    // node:stream/web QueuingStrategy classes (#1545). #4915: the
    // constructor lowers through the same stdlib builtin arm as the
    // node:stream/web form, with real byteLength desiredSize accounting.
    class("streams", "ByteLengthQueuingStrategy"),
    class("streams", "CountQueuingStrategy"),
    // --- node:http server (issue #577) ---
    method("http", "createServer", false, None),
    method("http", "listen", true, Some("HttpServer")),
    method("http", "close", true, Some("HttpServer")),
    method("http", "closeAllConnections", true, Some("HttpServer")),
    method("http", "closeIdleConnections", true, Some("HttpServer")),
    method("http", "on", true, Some("HttpServer")),
    method("http", "addListener", true, Some("HttpServer")),
    // #2153 — `.address()` was stubbed in the runtime
    // (`js_node_http_server_address_json`) but missing from both
    // `NATIVE_MODULE_TABLE` and the manifest.
    method("http", "address", true, Some("HttpServer")),
    // Issue #2210 — `server.<name>` timeout/socket-option accessors,
    // plus the canonical `server.setTimeout(ms, cb)` method. Each
    // accessor has two manifest entries (`__get_<name>` HIR-rewrite +
    // bare-name fallback for receivers that escape the rewrite).
    method("http", "__get_listening", true, Some("HttpServer")),
    method("http", "listening", true, Some("HttpServer")),
    method("http", "__get_headersTimeout", true, Some("HttpServer")),
    method("http", "__set_headersTimeout", true, Some("HttpServer")),
    method("http", "headersTimeout", true, Some("HttpServer")),
    method("http", "__get_keepAliveTimeout", true, Some("HttpServer")),
    method("http", "__set_keepAliveTimeout", true, Some("HttpServer")),
    method("http", "keepAliveTimeout", true, Some("HttpServer")),
    method(
        "http",
        "__get_keepAliveTimeoutBuffer",
        true,
        Some("HttpServer"),
    ),
    method(
        "http",
        "__set_keepAliveTimeoutBuffer",
        true,
        Some("HttpServer"),
    ),
    method("http", "keepAliveTimeoutBuffer", true, Some("HttpServer")),
    method("http", "__get_requestTimeout", true, Some("HttpServer")),
    method("http", "__set_requestTimeout", true, Some("HttpServer")),
    method("http", "requestTimeout", true, Some("HttpServer")),
    method("http", "__get_timeout", true, Some("HttpServer")),
    method("http", "__set_timeout", true, Some("HttpServer")),
    method("http", "timeout", true, Some("HttpServer")),
    method("http", "__get_maxHeadersCount", true, Some("HttpServer")),
    method("http", "__set_maxHeadersCount", true, Some("HttpServer")),
    method("http", "maxHeadersCount", true, Some("HttpServer")),
    method(
        "http",
        "__get_maxRequestsPerSocket",
        true,
        Some("HttpServer"),
    ),
    method(
        "http",
        "__set_maxRequestsPerSocket",
        true,
        Some("HttpServer"),
    ),
    method("http", "maxRequestsPerSocket", true, Some("HttpServer")),
    method("http", "setTimeout", true, Some("HttpServer")),
    // #5011 — `server.ref()` / `server.unref()` return the server (`this`)
    // for chaining; `unref()` also drops the server out of the event-loop
    // keepalive set so the process can exit while still bound.
    method("http", "ref", true, Some("HttpServer")),
    method("http", "unref", true, Some("HttpServer")),
    method("http", "on", true, Some("IncomingMessage")),
    method("http", "addListener", true, Some("IncomingMessage")),
    method("http", "pause", true, Some("IncomingMessage")),
    method("http", "resume", true, Some("IncomingMessage")),
    method("http", "destroy", true, Some("IncomingMessage")),
    method("http", "read", true, Some("IncomingMessage")),
    method("http", "setEncoding", true, Some("IncomingMessage")),
    method("http", "setTimeout", true, Some("IncomingMessage")),
    // Issue #769 — `ClientRequest.setTimeout(ms)` for `http.request` /
    // `http.get` returns. Class filter differs from any existing http
    // method, so the manifest-consistency drift guard requires a row
    // here even though the test collapses class_filter variants.
    method("http", "setTimeout", true, Some("ClientRequest")),
    method("http", "listenerCount", true, Some("ClientRequest")),
    method("http", "setHeader", true, Some("ClientRequest")),
    method("http", "getHeader", true, Some("ClientRequest")),
    method("http", "hasHeader", true, Some("ClientRequest")),
    method("http", "removeHeader", true, Some("ClientRequest")),
    method("http", "getHeaderNames", true, Some("ClientRequest")),
    method("http", "getHeaders", true, Some("ClientRequest")),
    method("http", "getRawHeaderNames", true, Some("ClientRequest")),
    method("http", "abort", true, Some("ClientRequest")),
    method("http", "destroy", true, Some("ClientRequest")),
    method("http", "flushHeaders", true, Some("ClientRequest")),
    method("http", "cork", true, Some("ClientRequest")),
    method("http", "uncork", true, Some("ClientRequest")),
    method("http", "setNoDelay", true, Some("ClientRequest")),
    method("http", "setSocketKeepAlive", true, Some("ClientRequest")),
    method("http", "__get_method", true, Some("ClientRequest")),
    method("http", "__get_protocol", true, Some("ClientRequest")),
    method("http", "__get_host", true, Some("ClientRequest")),
    method("http", "__get_path", true, Some("ClientRequest")),
    method("http", "__get_aborted", true, Some("ClientRequest")),
    method("http", "__get_connection", true, Some("ClientRequest")),
    method("http", "__get_destroyed", true, Some("ClientRequest")),
    method("http", "__get_finished", true, Some("ClientRequest")),
    method("http", "__get_maxHeadersCount", true, Some("ClientRequest")),
    method("http", "__get_reusedSocket", true, Some("ClientRequest")),
    method("http", "__get_socket", true, Some("ClientRequest")),
    method("http", "__get_writableEnded", true, Some("ClientRequest")),
    method(
        "http",
        "__get_writableFinished",
        true,
        Some("ClientRequest"),
    ),
    method("http", "setHeader", true, Some("ServerResponse")),
    method("http", "getHeader", true, Some("ServerResponse")),
    method("http", "removeHeader", true, Some("ServerResponse")),
    method("http", "hasHeader", true, Some("ServerResponse")),
    method("http", "getHeaders", true, Some("ServerResponse")),
    method("http", "getHeaderNames", true, Some("ServerResponse")),
    method("http", "appendHeader", true, Some("ServerResponse")),
    method("http", "setHeaders", true, Some("ServerResponse")),
    method("http", "writeHead", true, Some("ServerResponse")),
    method("http", "write", true, Some("ServerResponse")),
    method("http", "addTrailers", true, Some("ServerResponse")),
    method("http", "end", true, Some("ServerResponse")),
    method("http", "flushHeaders", true, Some("ServerResponse")),
    method("http", "cork", true, Some("ServerResponse")),
    method("http", "uncork", true, Some("ServerResponse")),
    method("http", "setTimeout", true, Some("ServerResponse")),
    method("http", "writeEarlyHints", true, Some("ServerResponse")),
    method("http", "writeContinue", true, Some("ServerResponse")),
    method("http", "writeProcessing", true, Some("ServerResponse")),
    method("http", "on", true, Some("ServerResponse")),
    method("http", "addListener", true, Some("ServerResponse")),
    method("http", "method", true, Some("IncomingMessage")),
    method("http", "url", true, Some("IncomingMessage")),
    method("http", "httpVersion", true, Some("IncomingMessage")),
    method("http", "statusCode", true, Some("IncomingMessage")),
    method("http", "statusMessage", true, Some("IncomingMessage")),
    method("http", "headers", true, Some("IncomingMessage")),
    method("http", "trailers", true, Some("IncomingMessage")),
    method("http", "setStatus", true, Some("ServerResponse")),
    method("http", "getStatus", true, Some("ServerResponse")),
    method("http", "__get_method", true, Some("IncomingMessage")),
    method("http", "__get_url", true, Some("IncomingMessage")),
    method("http", "__get_httpVersion", true, Some("IncomingMessage")),
    method(
        "http",
        "__get_httpVersionMajor",
        true,
        Some("IncomingMessage"),
    ),
    method(
        "http",
        "__get_httpVersionMinor",
        true,
        Some("IncomingMessage"),
    ),
    method("http", "__get_complete", true, Some("IncomingMessage")),
    method("http", "__get_aborted", true, Some("IncomingMessage")),
    method("http", "__get_destroyed", true, Some("IncomingMessage")),
    method("http", "__get_statusCode", true, Some("IncomingMessage")),
    method("http", "__get_statusMessage", true, Some("IncomingMessage")),
    method("http", "__get_headers", true, Some("IncomingMessage")),
    method("http", "__get_trailers", true, Some("IncomingMessage")),
    method("http", "__get_statusCode", true, Some("ServerResponse")),
    method("http", "__set_statusCode", true, Some("ServerResponse")),
    method("http", "__set_statusMessage", true, Some("ServerResponse")),
    method("http", "__set_sendDate", true, Some("ServerResponse")),
    method(
        "http",
        "__set_strictContentLength",
        true,
        Some("ServerResponse"),
    ),
    method("http", "__get_headersSent", true, Some("ServerResponse")),
    method("http", "__get_writableEnded", true, Some("ServerResponse")),
    method(
        "http",
        "__get_writableFinished",
        true,
        Some("ServerResponse"),
    ),
    class("http", "Server"),
    class("http", "IncomingMessage"),
    class("http", "OutgoingMessage"),
    class("http", "ServerResponse"),
    // --- node:https server (issue #577 Phase 2) ---
    method("https", "createServer", false, None),
    method("https", "listen", true, Some("HttpsServer")),
    method("https", "close", true, Some("HttpsServer")),
    method("https", "closeAllConnections", true, Some("HttpsServer")),
    method("https", "closeIdleConnections", true, Some("HttpsServer")),
    method("https", "on", true, Some("HttpsServer")),
    method("https", "addListener", true, Some("HttpsServer")),
    method("https", "address", true, Some("HttpsServer")),
    method("https", "__get_listening", true, Some("HttpsServer")),
    method("https", "listening", true, Some("HttpsServer")),
    method("https", "__get_headersTimeout", true, Some("HttpsServer")),
    method("https", "__set_headersTimeout", true, Some("HttpsServer")),
    method("https", "headersTimeout", true, Some("HttpsServer")),
    method("https", "__get_keepAliveTimeout", true, Some("HttpsServer")),
    method("https", "__set_keepAliveTimeout", true, Some("HttpsServer")),
    method("https", "keepAliveTimeout", true, Some("HttpsServer")),
    method(
        "https",
        "__get_keepAliveTimeoutBuffer",
        true,
        Some("HttpsServer"),
    ),
    method(
        "https",
        "__set_keepAliveTimeoutBuffer",
        true,
        Some("HttpsServer"),
    ),
    method("https", "keepAliveTimeoutBuffer", true, Some("HttpsServer")),
    method("https", "__get_requestTimeout", true, Some("HttpsServer")),
    method("https", "__set_requestTimeout", true, Some("HttpsServer")),
    method("https", "requestTimeout", true, Some("HttpsServer")),
    method("https", "__get_timeout", true, Some("HttpsServer")),
    method("https", "__set_timeout", true, Some("HttpsServer")),
    method("https", "timeout", true, Some("HttpsServer")),
    method("https", "__get_maxHeadersCount", true, Some("HttpsServer")),
    method("https", "__set_maxHeadersCount", true, Some("HttpsServer")),
    method("https", "maxHeadersCount", true, Some("HttpsServer")),
    method(
        "https",
        "__get_maxRequestsPerSocket",
        true,
        Some("HttpsServer"),
    ),
    method(
        "https",
        "__set_maxRequestsPerSocket",
        true,
        Some("HttpsServer"),
    ),
    method("https", "maxRequestsPerSocket", true, Some("HttpsServer")),
    method("https", "setTimeout", true, Some("HttpsServer")),
    // #5011 — see the http HttpServer `ref`/`unref` rows.
    method("https", "ref", true, Some("HttpsServer")),
    method("https", "unref", true, Some("HttpsServer")),
    class("https", "Server"),
    // --- node:http2 server (issue #577 Phase 3) ---
    method("http2", "createSecureServer", false, None),
    method("http2", "listen", true, Some("Http2SecureServer")),
    method("http2", "close", true, Some("Http2SecureServer")),
    method("http2", "on", true, Some("Http2SecureServer")),
    method("http2", "address", true, Some("Http2SecureServer")),
    // --- node:http2 settings helpers (issue #3168) ---
    method("http2", "getDefaultSettings", false, None),
    method("http2", "getPackedSettings", false, None),
    method("http2", "getUnpackedSettings", false, None),
    // `http2.performServerHandshake(socket[, options])` — Node's module-level
    // helper for adopting an already-connected socket as an HTTP/2 server
    // session (#3720). Exposed as a callable export (length 1) so the value
    // read matches Node's `typeof` / `name` / `length` shape; wired through
    // `is_native_module_callable_export` / `native_callable_export_arity`.
    method("http2", "performServerHandshake", false, None),
    // #3905: remaining public ESM export surface — the non-secure server
    // factory, the client-session factory, and the module default (namespace
    // object). `createServer` is already runtime-callable; these unblock the
    // named/default imports that Node accepts.
    method("http2", "createServer", false, None),
    method("http2", "connect", false, None),
    property("http2", "default"),
    internal_class("http2", "Http2SecureServer"),
    class("http2", "Http2ServerRequest"),
    class("http2", "Http2ServerResponse"),
    // `http2.constants` — the object of HTTP2_HEADER_* / NGHTTP2_* /
    // HTTP_STATUS_* values. `@hono/node-server` imports it by name (#1651).
    property("http2", "constants"),
    property("http2", "sensitiveHeaders"),
    // `@perryts/google-auth` no longer ships in the bundled manifest —
    // since v0.5.1015 it lives at https://github.com/PerryTS/google-auth
    // and is installed via `npm install @perryts/google-auth`. The
    // package's own `perry.nativeLibrary.functions` declares the FFI
    // surface; the manifest's unimplemented-API check resolves the
    // import via the standard external-nativeLibrary lookup.
    // --- @perryts/pdf (issue #516) ---
    // Minimal PDF creation API. The five FFI entry points exported
    // by crates/perry-ext-pdf. Param shapes intentionally loose
    // here (mostly `p_any`) — codegen's NATIVE_MODULE_TABLE rows
    // tighten them. createPdf takes a single options object and
    // returns a numeric handle; pdfAddText/pdfAddLine accept
    // positional args.
    method_sig(
        "@perryts/pdf",
        "createPdf",
        false,
        None,
        &[p_any("opts")],
        TypeSpec::Number,
    ),
    method("@perryts/pdf", "pdfAddText", false, None),
    method("@perryts/pdf", "pdfAddLine", false, None),
    method("@perryts/pdf", "pdfNewPage", false, None),
    method("@perryts/pdf", "pdfSave", false, None),
    // --- perry/ads (issue #867) ---
    // Six FFI entry points exported by crates/perry-ext-ads.
    // Promise-returning load / show pairs for interstitial and
    // rewarded ads; sync handle-returning create + destroy pair
    // for the banner widget. Listed here so the manifest's
    // unimplemented-API check (#463) accepts them when a user
    // writes `import { js_ads_interstitial_show } from "perry/ads"`.
    // The MVP returns structured `{ error: "no-sdk-linked" }`
    // placeholders; real Google Mobile Ads SDK integration is
    // tracked under the same issue.
    method("perry/ads", "js_ads_interstitial_load", false, None),
    method("perry/ads", "js_ads_interstitial_show", false, None),
    method("perry/ads", "js_ads_rewarded_load", false, None),
    method("perry/ads", "js_ads_rewarded_show", false, None),
    method("perry/ads", "js_ads_banner_create", false, None),
    method("perry/ads", "js_ads_banner_destroy", false, None),
    method("perry/ads", "js_ads_request_consent", false, None),
    // --- "bun" module / Bun globals shim pack (issue #6560) ---
    // Tier 0 of Bun-app support (driver: opencode). `Bun.stringWidth`,
    // `Bun.file` / `Bun.write`, `Bun.stdin/stdout/stderr`, `Bun.hash`
    // (Zig-std wyhash, BigInt result), and the module-level
    // `pathToFileURL` / `fileURLToPath` aliases of node:url.
    // Implementation: perry-runtime `bun_compat`.
    method("bun", "stringWidth", false, None),
    method("bun", "hash", false, None),
    method("bun", "file", false, None),
    method("bun", "write", false, None),
    method("bun", "pathToFileURL", false, None),
    method("bun", "fileURLToPath", false, None),
    property("bun", "stdin"),
    property("bun", "stdout"),
    property("bun", "stderr"),
];
