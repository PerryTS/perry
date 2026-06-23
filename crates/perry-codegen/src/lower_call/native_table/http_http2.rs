use super::*;

pub(super) const HTTP_HTTP2_ROWS: &[NativeModSig] = &[
    // ========== node:http2 server (issue #577 Phase 3) ==========
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "createServer",
        class_filter: None,
        runtime: "js_node_http2_create_server",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "createSecureServer",
        class_filter: None,
        runtime: "js_node_http2_create_secure_server",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_node_http2_connect",
        args: &[NA_F64, NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "listen",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_listen",
        // Variadic listen() overloads — see the http `listen` row. Issue #2041.
        // Returns the server handle for chainability (#2129).
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "close",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "on",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "address",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_address_json",
        args: &[],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // ========== node:http2 settings helpers (issue #3168) ==========
    // Pure pack/unpack functions implemented in perry-ext-http-server (so
    // their Buffer alloc/recognition shares the program's runtime copy).
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "getDefaultSettings",
        class_filter: None,
        runtime: "js_node_http2_get_default_settings",
        args: &[],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "getPackedSettings",
        class_filter: None,
        runtime: "js_node_http2_get_packed_settings",
        // NA_JSV: pass the settings object's raw NaN-boxed bits (the runtime
        // JSON-stringifies it); NR_PTR: return value is a Buffer pointer.
        args: &[NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "getUnpackedSettings",
        class_filter: None,
        runtime: "js_node_http2_get_unpacked_settings",
        // NA_JSV: pass the Buffer's raw bits (runtime reads it via the
        // buffer-registry shim); NR_OBJ_FROM_JSON_STR: returns a settings
        // object reparsed from JSON.
        args: &[NA_JSV],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // `@perryts/google-auth` is no longer bundled in perry-stdlib —
    // since v0.5.1015 the package is published as a standalone npm
    // module (https://github.com/PerryTS/google-auth). Codegen
    // dispatches `js_google_auth_*` symbols through `ffi_signatures`
    // built from the installed package's
    // `perry.nativeLibrary.functions`, same as any other external
    // nativeLibrary crate.
    // ========== perry/ads (issue #867) ==========
    // Six FFI entry points exported by `crates/perry-ext-ads`:
    //   - 4 promise-returning load/show pairs for interstitial +
    //     rewarded (NR_PROMISE — runtime sees a `*mut perry_ffi::Promise`
    //     and NaN-boxes via an explicit promise-boundary transition).
    //   - 2 synchronous banner create/destroy (NR_F64 / NR_VOID —
    //     banner_create returns a handle as a `number`, destroy is
    //     fire-and-forget).
    // MVP returns structured `{ error: "no-sdk-linked" }`
    // placeholders; real Google Mobile Ads SDK integration follows.
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_interstitial_load",
        class_filter: None,
        runtime: "js_ads_interstitial_load",
        args: &[NA_STR],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_interstitial_show",
        class_filter: None,
        runtime: "js_ads_interstitial_show",
        args: &[],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_rewarded_load",
        class_filter: None,
        runtime: "js_ads_rewarded_load",
        args: &[NA_STR],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_rewarded_show",
        class_filter: None,
        runtime: "js_ads_rewarded_show",
        args: &[],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_banner_create",
        class_filter: None,
        runtime: "js_ads_banner_create",
        args: &[NA_STR, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_banner_destroy",
        class_filter: None,
        runtime: "js_ads_banner_destroy",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_request_consent",
        class_filter: None,
        runtime: "js_ads_request_consent",
        args: &[],
        ret: NR_PROMISE,
    },
];
