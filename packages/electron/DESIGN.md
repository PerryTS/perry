# Electron-compat for Perry — design

Goal: an existing Electron app's source (`main.ts`/`main.js`, `preload.js`, renderer
HTML/JS) runs **unmodified** on Perry. App logic compiles natively; the view runs in
the OS-native webview (WKWebView / WebView2 / WebKitGTK). Internally this is the
Tauri model — system webview, single native process — but the *public surface* is the
Electron API, so it is a drop-in toolkit, not a new framework to learn.

## Two halves of the Electron API

Electron splits across two runtimes. We mirror that split:

| Electron surface | Runs where in Electron | Runs where in Perry |
|---|---|---|
| `app`, `BrowserWindow`, `ipcMain`, `Menu`, `dialog`, `webContents` | main process (Node) | **Perry-compiled native TS** — this package's `src/index.ts` |
| `ipcRenderer`, `contextBridge`, `require('electron')` in preload | renderer (Chromium) | **JS injected into the webview** as a document-start user script — `bridge/preload-runtime.js` |

The app's own `preload.js` is plain JS that runs *in the renderer*. We inject it (after
our renderer-side `require('electron')` shim) as another document-start user script, so
`contextBridge.exposeInMainWorld(...)` works exactly as the app expects.

## The IPC transport (the one genuinely-new native piece)

Native plumbing added to `perry-ui-macos` (mirror later on Windows/Linux):

- **renderer → main**: a `WKScriptMessageHandler` registered on the webview's
  `WKUserContentController` under the name `perry`. Renderer calls
  `window.webkit.messageHandlers.perry.postMessage(jsonString)`. The native
  `userContentController:didReceiveScriptMessage:` delegate method routes the payload
  to a TS closure stored on the webview (`on_message`), same NaN-box dance as the
  existing `onLoaded` callback.
- **main → renderer**: existing `webviewEvaluateJs(handle, js, cb)` — we eval
  `window.__perryDeliver(channel, payloadJson)` into the page.
- **request/response (`ipcRenderer.invoke` → `ipcMain.handle`)**: the renderer-side
  shim tags each invoke with a monotonic id, posts `{kind:'invoke', id, channel, args}`,
  and parks a Promise in a pending-map. The main side runs the handler and evals back
  `window.__perryResolve(id, ok, result)`. Fire-and-forget (`send`) uses the same
  transport with `kind:'send'`.

Payload codec: JSON for v1. (Perry already has a V8 structured-clone codec in
`child_process/v8_serde.rs` we can swap in later for transferables/Buffers.)

## Native changes required (batched into one perry rebuild)

1. **Webview IPC bridge** (`crates/perry-ui-macos/src/widgets/webview.rs` +
   `lib_ffi/interactivity.rs` + codegen `lower_call/native/mod.rs`):
   - `WebViewState.on_message: f64` field.
   - Create the delegate *before* the `WKWebViewConfiguration`, build a
     `WKUserContentController`, `addScriptMessageHandler:name:"perry"`, `addUserScript:`
     a document-start preload runtime, `setUserContentController:` on the config.
   - New delegate method `userContentController:didReceiveScriptMessage:` → unbox
     `on_message`, `js_closure_call1(closure, nanbox_str(json))`.
   - FFI `perry_ui_webview_set_on_message(handle, closure: f64)` and
     `perry_ui_webview_add_user_script(handle, src_ptr)` (for injecting the app's
     preload + a `loadURL`-time bridge).
   - codegen: add `onMessage` key to the `WebView({...})` lowering.

2. **Dynamic multi-window from TS** — verify whether `Window()` + instance methods are
   *already* reachable via the `perry-dispatch` tables (probe says: TBD). If not, wire
   `perry_ui_window_create`/`window_set_body`/`window_show`/`window_close` into codegen.

3. **Full-window pinning** (`app.rs::window_set_body`): add the same 4 Auto-Layout
   constraints `app_set_body` uses so a webview fills/resizes with the window.

4. **`app.whenReady()` resolver**: a runtime promise resolved once after
   `applicationDidFinishLaunching:` / first pump tick, so
   `app.whenReady().then(() => new BrowserWindow())` fires after `app.run()` starts.

5. **`app.quit()` FFI**: `perry_ui_app_quit()` → `NSApplication terminate:`.
   Plus `window-all-closed` tracking for the Electron lifecycle event.

## Resolution

The app installs `@perryts/electron` and maps `"electron"` to it through
`perry.packageAliases`. The shim's `nativeModule:true` metadata then compiles
`src/index.ts` natively. Existing `import { app, BrowserWindow } from 'electron'`
statements remain unchanged. No `electron` binary, no Chromium.

## Known limitations (state honestly)

- Single process: a webview hang takes the app down. The trusted preload loader
  is removed before page scripts run and only contextBridge-selected APIs are
  exposed, but this is not a Chromium process-isolation boundary.
- Cross-engine rendering differences (Safari vs Edge vs WebKitGTK), same caveat as Tauri.
- npm long-tail: deps without TS source / native bindings won't compile.
- v1 IPC is JSON — no transferables/Buffers/MessagePort yet.
