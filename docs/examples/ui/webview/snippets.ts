// demonstrates: per-API WebView snippets shown in docs/src/ui/webview.md
// docs: docs/src/ui/webview.md
// platforms: macos, linux, windows
// run: false

// `run: false` because the WebView FFI spawns a real browser engine
// (WKWebView / WebView2 / WebKitGTK / android.webkit.WebView) and
// performs live network I/O — neither is safe inside the doc-tests
// harness. Compile-link is enough to certify the codegen surface; this
// file pins every name in the WebView API so a future rename / drop
// trips a link error in CI.

// ANCHOR: imports
import {
    WebView,
    webviewLoadUrl,
    webviewReload,
    webviewGoBack,
    webviewGoForward,
    webviewCanGoBack,
    webviewEvaluateJs,
    webviewClearCookies,
} from "perry/ui"
// ANCHOR_END: imports
import { App, VStack, HStack, Button, Text, State } from "perry/ui"

// ANCHOR: basic
const wv = WebView({
    url: "https://example.com",
    width: 800,
    height: 600,
})

App({
    title: "WebView Demo",
    width: 820,
    height: 640,
    body: wv,
})
// ANCHOR_END: basic

// ANCHOR: oauth
const authCode = State("")

const auth = WebView({
    url: "https://accounts.google.com/o/oauth2/auth?client_id=...&redirect_uri=https://myapp.com/oauth/callback&response_type=code&scope=email",
    // Hard host-level allowlist — blocked at the native delegate
    // without round-tripping into TS. Exact match or subdomain match
    // (so "google.com" allows "accounts.google.com").
    allowedDomains: ["accounts.google.com", "google.com", "myapp.com"],
    onShouldNavigate: (url) => {
        if (url.startsWith("https://myapp.com/oauth/callback?")) {
            const code = new URL(url).searchParams.get("code") ?? ""
            authCode.set(code)
            return false  // cancel — we already have what we need
        }
        return true
    },
    onLoaded: (url) => {
        // Fires after every successful page load.
    },
    onError: (code, message) => {
        // DNS / TLS / HTTP / cancellation all flow here.
    },
})
// ANCHOR_END: oauth

// ANCHOR: imperative
// Navigate the WebView from outside (e.g. from a toolbar button).
webviewLoadUrl(wv, "https://perryts.com")
webviewReload(wv)
webviewGoBack(wv)
webviewGoForward(wv)
const hasHistory = webviewCanGoBack(wv)  // 1 or 0
// ANCHOR_END: imperative

// ANCHOR: evaluate-js
// Read state out of the loaded page after `onLoaded` fires. The
// callback gets the stringified return value (empty string on null /
// undefined / error). Plain string returns are JSON-unwrapped for
// ergonomic `document.cookie` reads.
const reader = WebView({
    url: "https://example.com/auth/callback",
    onLoaded: (_url) => {
        webviewEvaluateJs(reader, "document.cookie", (cookies) => {
            // parseCookies(cookies)
        })
    },
})
// ANCHOR_END: evaluate-js

// ANCHOR: clear-cookies
// Wipe the WebView's cookies / localStorage / IndexedDB. Useful at
// logout, or between user accounts in a multi-tenant flow. No-op when
// `ephemeral: true` (the default), since there's nothing persisted to
// clear.
webviewClearCookies(wv)
// ANCHOR_END: clear-cookies

// ANCHOR: persistent
// Opt out of ephemeral cookies so the user's session survives app
// restarts (like a regular browser profile).
const browser = WebView({
    url: "https://news.ycombinator.com",
    ephemeral: false,
    userAgent: "MyApp/1.0",
})
// ANCHOR_END: persistent
