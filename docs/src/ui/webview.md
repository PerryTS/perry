# WebView

`WebView` embeds a real browser engine inside the native widget tree —
`WKWebView` on Apple platforms, `WebView2` on Windows, `WebKitGTK 6.0`
on Linux, `android.webkit.WebView` on Android, and a sandboxed
`<iframe>` on the web target. Use it for OAuth / payment flows, embedded
admin pages, help / docs viewers, or any "show this URL as part of my
app" surface. (Tracked under issue #658.)

```ts
{{#include ../../examples/ui/webview/snippets.ts:imports}}
```

> **Scope.** This is a "browser tab embedded in your native widget tree"
> primitive — explicit non-goals: a Tauri / Electron-style native↔JS RPC
> bridge, custom protocol / scheme handlers, DevTools, file downloads,
> WebGL / camera / mic / clipboard permission negotiation, service
> workers, WebRTC. If you need any of those, reach for Tauri or
> Electron; the rest of `perry/ui` still applies.

## Basic Usage

```ts
{{#include ../../examples/ui/webview/snippets.ts:basic}}
```

`WebView({...})` returns a `Widget` you can drop into any layout
container. The widget tree's layout engine controls final size — `width`
and `height` are hints for the initial bounds.

## OAuth / Callback Interception

The load-bearing use case. `onShouldNavigate` is a **synchronous**
intercept invoked before each navigation; return `false` to cancel the
load. Every backend's should-load hook is itself sync on the main
thread (`decidePolicyForNavigationAction`, `NavigationStarting`,
`shouldOverrideUrlLoading`, `decide-policy`), so the contract is the
same everywhere.

```ts
{{#include ../../examples/ui/webview/snippets.ts:oauth}}
```

The `allowedDomains` allowlist is enforced at the **native delegate
layer** — disallowed hosts never reach your `onShouldNavigate`. Treat it
as defense-in-depth against a hijacked OAuth page redirecting the
embedded session somewhere unexpected.

## Imperative Navigation

Drive the WebView from outside (toolbar buttons, deep links, app-state
changes):

```ts
{{#include ../../examples/ui/webview/snippets.ts:imperative}}
```

## Reading Page State

`webviewEvaluateJs(handle, js, callback)` runs a one-shot JS expression
in the WebView's content process and delivers the stringified result.
Use this for "after the redirect lands, read `document.cookie` /
`localStorage.getItem(...)`" — not as a general native↔JS RPC channel.

```ts
{{#include ../../examples/ui/webview/snippets.ts:evaluate-js}}
```

The callback receives an empty string on `null` / `undefined` / error.
Plain string returns are JSON-unwrapped (so `document.cookie` reads
clean, without surrounding quotes).

## Cookie Isolation

`ephemeral: true` is the **default** — auth flows that silently reuse
a user's logged-in browser session are usually a footgun. Each backend
maps this to its native equivalent at construction time:

| Platform | Ephemeral | Persistent |
|----------|-----------|------------|
| **macOS / iOS / visionOS** | `WKWebsiteDataStore.nonPersistent()` | `WKWebsiteDataStore.defaultDataStore()` |
| **Windows** | per-handle temp `userDataFolder` under `%TEMP%\PerryWebView\<pid>-<tag>` | `%LOCALAPPDATA%\PerryWebView\persistent` |
| **Linux / GTK4** | `WebKitNetworkSession::new_ephemeral()` | `~/.local/share/perry-webview` + `~/.cache/perry-webview` (XDG-aware) |
| **Android** | best-effort `CookieManager.removeAllCookies(null)` + `WebStorage.deleteAllData()` at create | shared process-wide storage |
| **Web** | iframe shares parent storage (no true isolation) | same |

To opt out:

```ts
{{#include ../../examples/ui/webview/snippets.ts:persistent}}
```

`webviewClearCookies(handle)` wipes the data store on demand — useful
at logout, or between accounts:

```ts
{{#include ../../examples/ui/webview/snippets.ts:clear-cookies}}
```

## API

| Function | Description |
|----------|-------------|
| `WebView({ url, allowedDomains?, userAgent?, ephemeral?, onShouldNavigate?, onLoaded?, onError?, width?, height? })` | Construct the widget. Returns a `Widget` handle. |
| `webviewLoadUrl(handle, url)` | Replace the current URL and re-paint. |
| `webviewReload(handle)` | Reload the current page. |
| `webviewGoBack(handle)` | Navigate back through session history. |
| `webviewGoForward(handle)` | Navigate forward through session history. |
| `webviewCanGoBack(handle)` | Returns `1` if there's back history, `0` otherwise. |
| `webviewEvaluateJs(handle, js, callback)` | Run JS in the content process; callback receives the stringified result. |
| `webviewClearCookies(handle)` | Wipe cookies / localStorage / IndexedDB for this WebView's data store. |

### Options

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `url` | `string` | — | Initial URL. Required. |
| `allowedDomains` | `string[]` | `[]` | Hard host allowlist (exact OR subdomain). Empty / omitted = no host restriction. |
| `userAgent` | `string` | platform WebKit UA | Custom UA header. |
| `ephemeral` | `boolean` | `true` | Cookie / storage isolation. See the table above. |
| `onShouldNavigate` | `(url) => boolean \| void` | — | **Sync** intercept. Return `false` to cancel. |
| `onLoaded` | `(url) => void` | — | Fires when a page finishes loading. |
| `onError` | `(code, message) => void` | — | DNS / TLS / HTTP / cancellation. |
| `width`, `height` | `number` | layout-engine controlled | Initial pixel bounds; layout engine still has final say. |

## Platform Notes

| Platform | Backend | Notes |
|----------|---------|-------|
| **macOS** | `WKWebView` (AppKit) | Full callback parity. PerryWebViewDelegate (NSObject conforming to `WKNavigationDelegate`) carries the user closures + allowed-domains list. |
| **iOS / visionOS** | `WKWebView` (UIKit) | Same delegate pattern as macOS. |
| **Windows** | `WebView2` via `webview2-com` (pinned to `windows = "0.58"`) | A STATIC host HWND becomes the widget handle; `ICoreWebView2Controller` binds to it. WebView2's two-stage async init is wrapped synchronously by pumping the message queue with a 10s timeout — `WebView({...})` blocks until the widget is live, so the first navigation isn't racing init. `WM_SIZE` is subclassed on the host HWND and forwards bounds to `SetBounds` so the surface tracks layout-engine resizes. Requires the WebView2 runtime, which ships preinstalled on Windows 10+ and Windows Server 2019+. |
| **Linux / GTK4** | `WebKitGTK 6.0` via `webkit6 = "=0.4"` | Real implementation. `decide-policy::navigation-action` is the sync intercept. Build dep: `libwebkitgtk-6.0-dev` (Ubuntu 22.10+ / Debian 12+). |
| **Android** | `android.webkit.WebView` via JNI | `PerryWebViewClient.kt` (deployed alongside the runtime APK) bridges `shouldOverrideUrlLoading` / `onPageFinished` / `onReceivedError` back to native Rust. Full callback parity with the Apple / Windows / GTK4 backends. Ephemeral isolation is best-effort — Android WebView shares storage process-wide; `CookieManager.removeAllCookies(null)` + `WebStorage.deleteAllData()` runs at create when requested. |
| **Web** | sandboxed `<iframe>` | `sandbox="allow-scripts allow-same-origin allow-forms allow-popups"`. `onShouldNavigate` is best-effort (cross-origin URLs the iframe navigates to are unreachable from JS for security reasons); `onLoaded` fires from the iframe's `load` event; `onError` from `error` (same-origin only). `webviewEvaluateJs` only works on same-origin frames. UA is browser-controlled. See "Cross-origin messaging" below. |
| **tvOS / watchOS** | stub | All 14 FFIs link as no-ops returning `0`. The widget is invisible; cross-platform code compiles unchanged. |

## Cross-Origin Messaging (Web Target)

On the web target, the embedded iframe can `window.parent.postMessage`
out, and the host can `window.addEventListener("message", ...)` to
receive. This is a **browser-only** pattern — native targets don't
expose `postMessage` (that's the Tauri / Electron path Perry's WebView
deliberately avoids).

The portable contract that works on every target:

- Push state **in** with `webviewEvaluateJs(wv, "window.someHook(...)")`.
- Pull state **out** by intercepting a known callback URL in
  `onShouldNavigate`.

## Common Pitfalls

- **Don't reuse one `WebView` for unrelated sessions.** Cookie isolation
  is per-WebView, not per-call. If you need to log a different user in,
  call `webviewClearCookies(handle)` first or destroy and recreate the
  widget.
- **`onShouldNavigate` runs on the main thread.** Keep it cheap — it
  blocks the navigation until you return. Heavy work belongs in
  `onLoaded` or off-thread via `spawn`.
- **WebView2 runtime requirement on older Windows.** WebView2 is
  preinstalled on Windows 10 1803+ and Windows Server 2019+. On older
  builds the runtime needs to be installed separately (Microsoft ships
  an evergreen bootstrapper).
- **No bidirectional RPC.** If you find yourself round-tripping
  structured data through `webviewEvaluateJs` callbacks, you're past
  the design scope — pick Tauri / Electron instead, or move the logic
  out of the embedded page.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — React to `onLoaded` / `onError` from the rest of the UI
- [Multi-Window](multi-window.md) — Pop a fresh window with a WebView for isolated sessions
