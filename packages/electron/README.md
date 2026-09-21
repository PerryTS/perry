# electron (Perry compat)

Run existing **Electron** apps natively on [Perry](https://github.com/PerryTS/perry).
Your app's main process compiles to a native binary; the view runs in the
**OS-native webview** (WKWebView on macOS). Internally this is the Tauri model
(system webview, single native process, no bundled Chromium), but the public
API is Electron's, so existing app code runs **unmodified**.

> Result: Electron's DX (all-TypeScript, `app`/`BrowserWindow`/`ipcMain`), Tauri's
> footprint (~5 MB binary, no Chromium), and **no Rust to write**.

## Status

Experimental, **macOS only**. The renderer↔main IPC bridge is implemented on
WKWebView; Windows (WebView2) and Linux (WebKitGTK) backends are not wired yet.

## Running an existing app: `perry electron`

With the Perry CLI built from a checkout, run an existing Electron app as-is:
no npm install, no config, no code changes:

```bash
perry electron ./my-electron-app     # a directory with a package.json
perry electron ./dist/MyApp.app      # a packaged macOS bundle
perry electron ./app.asar            # a packed asar archive
perry electron ./main.js             # a main-process file directly
perry electron ./my-electron-app -- --some-flag   # args after -- go to the app
```

What happens:

1. **Find the entry.** The main-process entry comes from the package.json
   `main` field (default `index.js`, Electron's own rule). A packaged `.app`
   bundle resolves `Contents/Resources/app.asar` first, then the unpacked
   `Contents/Resources/app/` directory.
2. **Unpack asar archives.** `.asar` archives are extracted to a per-user
   cache (`~/Library/Caches/perry/electron-apps/` on macOS,
   `$XDG_CACHE_HOME/perry/electron-apps/` on Linux, the system temp dir if
   neither is writable) and reused while the archive's path, size, and mtime
   are unchanged. Nothing is written next to the archive, so signed read-only
   bundles work. `unpacked: true` entries are copied from the sibling
   `app.asar.unpacked/` directory.
3. **Compile the main process.** `import … from "electron"` and
   `require("electron")` are redirected to this package's `src/index.ts` via a
   compile-time package alias, so the app's own `node_modules` never needs the
   real Electron. ESM and CommonJS entries (`main.js`, `main.cjs`, `main.mjs`,
   `main.ts`) are all accepted; CommonJS is rewritten to ESM before compiling.
4. **Launch.** The binary is written to the current directory and runs with
   the app directory as its working directory, so renderer paths written
   relative to the app (`loadFile("index.html")`) resolve as they do under
   Electron. The app's exit code is passed through.

Build artifacts go to the usual `node_modules/.cache/perry` under the app root
(the extraction directory, for an asar). The one exception is a `.app` bundle
with an unpacked `Contents/Resources/app/`: writing there would modify the
installed, signed application, so that build cache lives under the per-user
cache instead (`PERRY_CACHE_DIR` still overrides it).

`perry electron` finds this package by walking up from the `perry` executable
to `packages/electron/src/index.ts`, which works for binaries built inside a
Perry checkout. For any other layout (for example a `CARGO_TARGET_DIR` outside
the checkout), set `PERRY_ELECTRON_SHIM` to this directory, its
`src/index.ts`, or the Perry checkout root.

It is shorthand for a plain compile with a package alias, which you can also
run yourself:

```bash
perry compile main.js -o my-app \
  --package-alias electron=/path/to/perry/packages/electron/src/index.ts
./my-app
```

## How it works

Electron splits across a main process (Node) and renderers (Chromium). This
package mirrors that split:

| Electron surface | Perry implementation |
|---|---|
| `app`, `BrowserWindow`, `ipcMain`, `Menu`, `dialog`, `webContents`, `Tray`, `clipboard` | native TS (`src/index.ts`) on top of `perry/ui` + `node:*` |
| `ipcRenderer`, `contextBridge`, `require('electron')` in preload | JS injected into each webview (`src/preload-runtime.ts`) |

The app's preload runs as a document-start user script with a CommonJS
`require`; page scripts afterwards see only what it exposed via `contextBridge`.

IPC transport on macOS:

- **renderer → main**: a `WKScriptMessageHandler` named `perry`. The renderer
  calls `window.webkit.messageHandlers.perry.postMessage(json)`; native routes
  it to the window's `ipcMain` dispatcher.
- **main → renderer**: `evaluateJavaScript` injects
  `window.__perryDeliver(channel, payload)` / `window.__perryResolve(id, ok, value)`.
- **`ipcRenderer.invoke` ↔ `ipcMain.handle`**: request/response correlated by a
  monotonic id; the renderer parks a Promise until the main side replies.

Renderer `console.*` output and uncaught errors are forwarded to the main
process's stdout. That is the easiest way to see what the page is doing.

## Library mode: compiling with the shim yourself

Use this when you build with `perry compile`/`perry run` yourself and want your
IDE's tsc to resolve `electron` to the compat types too.

The package publishes as **`@perryts/electron`** to avoid colliding with the
official `electron` package. Existing `import … from "electron"` statements
stay unchanged: a Perry package alias maps that specifier to the scoped shim.

```bash
npm install @perryts/electron
# or, from a Perry checkout (installs the same scoped package):
npm install /absolute/path/to/perry/packages/electron
```

```jsonc
// your-app/package.json
{
  "dependencies": { "@perryts/electron": "^0.1.0" },
  "perry": {
    "packageAliases": { "electron": "@perryts/electron" }
  }
}
```

`perry init` mirrors `packageAliases` into `tsconfig.json` `compilerOptions.paths`
(rebased onto an existing `baseUrl`), so your IDE's tsc resolves `electron` to
the compat types too. Then compile as usual:

```bash
perry main.ts -o my-app && ./my-app
```

Your `main.ts`, `preload.js`, and renderer HTML/JS stay as-is:

```ts
import { app, BrowserWindow, ipcMain } from "electron";

ipcMain.handle("ping", async () => "pong");

app.whenReady().then(() => {
  const win = new BrowserWindow({
    width: 900, height: 640,
    webPreferences: { preload: `${__dirname}/preload.js` },
  });
  win.loadFile("renderer/index.html");
});
```

See [`examples/system-explorer`](./examples/system-explorer) for a full app:
multiple IPC channels (invoke/handle + send/on), a live main→renderer clock
push, `fs`/`os` access, `contextBridge` preload, and disk-persisted notes.

## Implemented

- `app`: `whenReady`, `isReady`, `quit`, `getPath`, `getName`/`setName`, `getAppPath`,
  `on('ready'|'activate'|'window-all-closed'|'before-quit'|'will-quit')`
- `BrowserWindow`: `loadFile` / `loadURL` (the promise settles when the page
  loads, and rejects with `did-fail-load`), `show`/`hide`/`close`/`destroy`/`focus`,
  `setSize`, `isVisible`/`isDestroyed`, `getAllWindows`/`getFocusedWindow`,
  multiple windows, a full-window webview that resizes with the window
- `webContents`: `send`, `executeJavaScript`
- `ipcMain`: `handle`, `handleOnce`, `removeHandler`, `on` (+ `event.reply`)
- `ipcRenderer` (renderer): `invoke`, `send`, `on`, `once`, `removeListener`
- `contextBridge.exposeInMainWorld` (renderer). The preload runs with a
  CommonJS `module`/`require`/`__dirname` (`require('electron')` plus relative
  preload files); that loader is deleted before page scripts run, so the page
  sees only what the preload exposed through `contextBridge`
- `Menu`: `buildFromTemplate` + `setApplicationMenu` render a native menubar,
  with submenus, separators, `click` callbacks, standard `role`s
  (copy/paste/undo/quit/…), and `CmdOrCtrl+…` accelerators
- `dialog`: `showOpenDialog` / `showSaveDialog` on NSOpenPanel / NSSavePanel
- `Tray`: status item with icon, tooltip, click event, and context menu
- `clipboard`: `readText` / `writeText` / `clear`
- `shell.openExternal` / `openPath`

## Not yet (honest gaps)

- **macOS only.** The WebView2 / WebKitGTK bridge is not wired.
- **npm long-tail.** Packaged apps ship their production `node_modules` inside
  the asar. Perry compiles those dependencies ahead of time like the app
  itself; there is no JavaScript runtime to fall back to, so a dependency Perry
  can't compile (native `.node` addons, dynamic `require` of computed paths,
  unsupported syntax) fails the build instead of running.
- **`dialog` depth.** Open/save panels are single-selection (`multiSelections`
  and `filters` are ignored); `showMessageBox` returns `{ response: 0 }`
  without showing anything, and `showErrorBox` only logs.
- **`Menu` depth.** `Menu.popup()` and `Tray.popUpContextMenu()` are no-ops,
  and there is no live `enabled`/`checked` state after the menu is built.
- **IPC payloads are JSON.** No transferables, `Buffer`, or `MessagePort`. A
  handler result that can't be serialized rejects the renderer's `invoke`.
- **No `nodeIntegration` in pages.** Page scripts get no `require`; old apps
  that `require('./jquery.js')` from inline page script need a `<script src>`
  or a preload.
- **Smaller stubs.** `BrowserWindow.setTitle` (the title is fixed at creation),
  `app.getVersion()` (returns `0.0.0`), `app.exit(code)` (the code is ignored),
  `nativeImage` (path-backed only), `nativeTheme` (static values).
- **Single process.** A webview hang takes the app down. The trusted preload
  loader is removed before page scripts run and only `contextBridge` APIs reach
  the page, but this is not Chromium's process-isolation boundary.

See [`DESIGN.md`](./DESIGN.md) for the architecture and the native changes
this required in `perry-ui-macos`.
