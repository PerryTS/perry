# Electron-compat for Perry — design

Goal: an existing Electron app (`main.js`/`main.ts`, `preload.js`, renderer
HTML/JS, packaged or not) runs **unmodified** on Perry. The main process compiles
natively; the view runs in the OS-native webview. Internally this is the Tauri
model (system webview, single native process), but the *public surface* is the
Electron API, so it is a drop-in toolkit, not a new framework to learn.

Only macOS (WKWebView) is implemented. The same bridge is needed on WebView2 and
WebKitGTK.

## Two halves of the Electron API

Electron splits across two runtimes. We mirror that split:

| Electron surface | Runs where in Electron | Runs where in Perry |
|---|---|---|
| `app`, `BrowserWindow`, `ipcMain`, `Menu`, `dialog`, `webContents`, `Tray`, `clipboard` | main process (Node) | **Perry-compiled native TS**: this package's `src/index.ts` |
| `ipcRenderer`, `contextBridge`, `require('electron')` in preload | renderer (Chromium) | **JS injected into the webview** as a document-start user script: `bridge/preload-runtime.js` (embedded as `src/preload-runtime.ts`) |

The app's own `preload.js` is plain JS that runs *in the renderer*. The bridge
runtime installs a one-shot `window.__perryRunPreload(source, filename)`;
`BrowserWindow` then adds the preload as a second document-start script that
calls it (giving the preload `module`, `exports`, `require`, `__filename`,
`__dirname`), and a third that deletes the runner. `require` serves
`electron` and relative preload files and never reaches page globals, so page
scripts see only what `contextBridge.exposeInMainWorld(...)` copied over.

Only the main process is compiled by Perry. The renderer is whatever the app
ships (vanilla JS, React, Vue, a Vite bundle); the system webview runs it as-is.
A renderer that calls Node builtins directly (`require('fs')` under
`nodeIntegration`, `remote.require(...)`) hits the Tauri-model boundary: the
webview has no Node, so that work has to move behind IPC.

## Package name and resolution

The package publishes as `@perryts/electron` so it never collides with the
official `electron` package. Apps keep `import … from "electron"`; the
specifier is mapped to the shim either by `perry.packageAliases`
(`{"electron": "@perryts/electron"}`) in library mode, or by `perry electron`'s
compile-time `--package-alias` pointing at the shim source.

## Running existing apps: `perry electron`

`perry electron <path>` (`crates/perry/src/commands/electron.rs`) is the entry
point for apps that were never written with Perry in mind:

1. **Entry resolution.** A directory uses its package.json `main` (default
   `index.js`, Electron's rule). A `.app` bundle uses
   `Contents/Resources/app.asar`, then `Contents/Resources/app/`. An `.asar`
   archive is extracted first. A plain file is its own entry.
2. **asar extraction** (`electron/asar.rs`). A minimal reader for Electron's
   pickle-framed header + JSON tree. Every entry name is validated as a single
   safe path component, and every payload's offset and size are bounds-checked
   against the archive before anything is written. `unpacked: true` entries
   come from the sibling `app.asar.unpacked/`. The extraction goes to
   `<user cache dir>/perry/electron-apps/perry-electron-<hash>/`, keyed on
   archive path + size + mtime, and is reused while that key is unchanged.

   The cache location matters. It must **not** sit under Perry's project cache
   (`node_modules/.cache/perry`): module collection treats any `.js`/`.cjs` with
   a `node_modules` path component as npm package code, so an extraction there
   made the app's own `main.cjs` an untrusted package named `.cache`, and the
   build was refused as runtime JavaScript. A unit test pins the default root
   outside `node_modules`.
3. **Compile.** The standard compile pipeline with one addition: the
   `--package-alias electron=<shim>/src/index.ts` compile flag redirects every
   static and dynamic import of `electron` to this package, merged after any
   `perry.packageAliases` in the app's package.json. CommonJS entries go through
   the existing CommonJS→ESM rewrite (`cjs_wrap`). The app's production
   `node_modules` are compiled ahead of time under the default
   auto-`compilePackages` policy.
4. **Launch.** The binary runs with the app root as its cwd, because the shim's
   `loadFile` and preload reads resolve relative paths against `process.cwd()`,
   which matches Electron's app-directory convention.

The compile's build cache stays at its default under the app root, except for a
bundle's unpacked `Contents/Resources/app/`: writing there would modify the
installed, signed application, so that case builds under the per-user cache
(`perry-electron-build-<hash>`) unless `PERRY_CACHE_DIR` is set.

The shim is located via `PERRY_ELECTRON_SHIM` (the shim directory, its
`src/index.ts`, or a Perry checkout root), else by walking up from the `perry`
executable to `packages/electron/src/index.ts`.

## The IPC transport (the one genuinely new native piece)

Native plumbing in `perry-ui-macos` (`widgets/webview.rs`,
`lib_ffi/interactivity.rs`, `lib_ffi/core_widgets.rs`), exposed to TS through
`perry-dispatch` table rows and declared in `types/perry/ui/index.d.ts`:

- **renderer → main**: a `WKScriptMessageHandler` registered on the webview's
  `WKUserContentController` under the name `perry`. The renderer calls
  `window.webkit.messageHandlers.perry.postMessage(jsonString)`; the delegate's
  `userContentController:didReceiveScriptMessage:` hands the string to the TS
  closure registered with `webviewSetOnMessage`. The delegate is created before
  the `WKWebViewConfiguration` so the handler is attached at construction.
- **main → renderer**: the existing `webviewEvaluateJs(handle, js, cb)`; we eval
  `window.__perryDeliver(channel, payloadJson)` into the page.
- **request/response (`ipcRenderer.invoke` → `ipcMain.handle`)**: the renderer
  shim tags each invoke with a monotonic id, posts
  `{kind:'invoke', id, channel, args}`, and parks a Promise. The main side runs
  the handler and evals back `window.__perryResolve(id, ok, result)`.
  Fire-and-forget (`send`) uses the same transport with `kind:'send'`; renderer
  `console.*` and uncaught errors travel as `kind:'console'`.
- **document-start scripts**: `webviewAddUserScript` adds a `WKUserScript` for
  the bridge runtime and the app's preload before the first load.
- **file:// pages**: `loadFileURL:allowingReadAccessToURL:` grants read access
  to the page's containing directory, and `allowFileAccessFromFileURLs` (set via
  KVC) lets a local page load its own `<script src>` subresources and the
  preload loader XHR relative files. Universal file access stays off.

Payload codec: JSON. (Perry has a V8 structured-clone codec in
`child_process/v8_serde.rs` that could carry transferables and Buffers later.)

## App lifecycle and windows

- **Top-level loop takeover.** `app` registers the native loop with
  `appRequestLoop(onReady)` at module init, which does **not** block. Generated
  `main` calls `js_ui_loop_take_over()` (`perry-runtime/src/ui_loop.rs`) at the
  true top level, after the user's top-level code has registered its
  `ipcMain`/`whenReady` handlers, and that enters `[NSApp run]`. `onReady`
  resolves `app.whenReady()`. Entering the loop from a microtask instead leaves
  windows that exist but never composite on screen. When no UI loop is
  registered, the takeover call is a no-op.
- **Windows.** `Window()` and its instance methods are dispatched from TS via
  the `perry-dispatch` tables, so dynamic multi-window needs no codegen work.
  Instance-method dispatch only fires on a local flow-tracked from the
  `Window()` call, so `BrowserWindow` captures closures over that local
  (`_show`/`_hide`/`_close`/`_setSize`) instead of calling through a property.
- **Pinning and activation.** `window_set_body` adds full-window Auto-Layout
  constraints so the webview fills and resizes with its window; `window_show`
  activates the app and orders the window front, because windows open after the
  loop's initial activation.
- **GC rooting.** The pending `onReady` closure lives in a thread-local slot
  between `appRequestLoop` and the loop takeover, across allocations that can
  collect. The slot is registered with `js_gc_register_global_root` (plus a root
  write barrier) so a moving collection keeps it alive and rewrites it, and
  `app_run_loop` re-reads the slot right before calling it.
- **Quit.** `appQuit()` → `NSApplication terminate:`. `window-all-closed` is
  emitted by the shim when its last `BrowserWindow` closes.

## Known limitations

- macOS only.
- Single process: a webview hang takes the app down. The preload loader is
  removed before page scripts run and only `contextBridge` APIs are exposed, but
  this is not Chromium's process-isolation boundary.
- Cross-engine rendering differences (WebKit vs Chromium), same caveat as Tauri.
- npm long-tail: a main-process dependency Perry can't compile fails the build;
  there is no JavaScript runtime to fall back to.
- IPC is JSON only: no transferables, `Buffer`, or `MessagePort`.
- `dialog` is single-selection with no filters; `showMessageBox` is a stub.
  `Menu.popup` is a no-op.
