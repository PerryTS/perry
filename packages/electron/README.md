# electron (Perry compat)

Run existing **Electron** apps natively on [Perry](https://github.com/PerryTS/perry).
Your app's logic compiles to a native binary; the view runs in the **OS-native
webview** (WKWebView / WebView2 / WebKitGTK). Internally this is the Tauri model
— system webview, single native process, no bundled Chromium — but the public
API is Electron's, so existing app code runs **unmodified**.

> Result: Electron's DX (all-TypeScript, `app`/`BrowserWindow`/`ipcMain`), Tauri's
> footprint (~5 MB binary, no Chromium), and **no Rust to write**.

## Status

Experimental. macOS first. The genuinely-new native piece — the bidirectional
JS↔native IPC bridge that Electron/Tauri both need — is implemented on
WKWebView; Windows/Linux backends are a follow-up.

## How it works

Electron splits across a main process (Node) and renderers (Chromium). This
package mirrors that split:

| Electron surface | Perry implementation |
|---|---|
| `app`, `BrowserWindow`, `ipcMain`, `Menu`, `dialog`, `webContents` | native TS (`src/index.ts`) on top of `perry/ui` + `node:*` |
| `ipcRenderer`, `contextBridge`, `require('electron')` in preload | JS injected into each webview (`src/preload-runtime.ts`) |

IPC transport on macOS:

- **renderer → main**: a `WKScriptMessageHandler` named `perry`. The renderer
  calls `window.webkit.messageHandlers.perry.postMessage(json)`; native routes
  it to the window's `ipcMain` dispatcher.
- **main → renderer**: `evaluateJavaScript` injects
  `window.__perryDeliver(channel, payload)` / `window.__perryResolve(id, ok, value)`.
- **`ipcRenderer.invoke` ↔ `ipcMain.handle`**: request/response correlated by a
  monotonic id; the renderer parks a Promise until the main side replies.

## Install

This package intentionally keeps the npm name **`electron`** so your app's
existing `import … from "electron"` resolves to it with zero code changes. npm
won't let anyone publish a package literally named `electron`, and it doesn't
support installing a single subdirectory straight from a GitHub repo, so install
it one of these two ways:

**A. Clone + install by path (works today):**

```bash
git clone https://github.com/PerryTS/perry
# in your app, install the compat package from the checkout:
npm install /absolute/path/to/perry/packages/electron
```

> `npm install github:PerryTS/perry#feat/electron-compat` installs the whole
> Perry repo, not just this package — the repo root now carries `name`/`version`
> so that no longer crashes npm's arborist, but you still want the subdirectory.

**B. Scoped npm package + alias (once published):** the package is also
published as **`@perryts/electron`**. Because it must keep importing as
`electron`, point Perry's resolver at the scoped install with a `packageAlias`:

```jsonc
// your-app/package.json
{
  "dependencies": { "@perryts/electron": "^0.1.0" },
  "perry": {
    "packageAliases": { "electron": "@perryts/electron" }
  }
}
```

`perry init` mirrors `packageAliases` into `tsconfig.json` `compilerOptions.paths`,
so your IDE's tsc resolves `electron` to the compat types too.

## Use it

If the package resolves under the bare name `electron` (install option A, or a
plain `node_modules/electron`), your app needs no Perry config at all:

```jsonc
// your-app/package.json — no code changes to your app
{
  "dependencies": { "electron": "*" }   // resolves to this compat package
}
```

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

- `app`: `whenReady`, `quit`, `getPath`, `getName`/`setName`, `on('ready'|'activate'|'window-all-closed'|'before-quit')`, `getAllWindows`
- `BrowserWindow`: `loadFile`, `loadURL`, `show`/`hide`/`close`, `webContents.send`/`executeJavaScript`, `getAllWindows`/`getFocusedWindow`, full-window webview that resizes
- `ipcMain`: `handle`, `handleOnce`, `removeHandler`, `on` (+ `event.reply`)
- `ipcRenderer` (renderer): `invoke`, `send`, `on`, `once`, `removeListener`
- `contextBridge.exposeInMainWorld` (renderer)
- `shell.openExternal`/`openPath`

## Not yet (honest gaps)

- **Windows / Linux IPC bridge** — macOS only so far (same pattern, more glue).
- **`dialog`** — v1 returns `canceled`; native NSOpenPanel/NSSavePanel wiring TODO.
- **`Menu`** — template accepted but not yet rendered to a native menubar (the
  standard Cmd-Q menubar is present).
- **IPC payloads** are JSON — no transferables / `Buffer` / `MessagePort` yet.
- **Single process** — a webview hang takes the app down; no `contextIsolation`
  security boundary (we emulate `contextBridge`, but everything shares one process).
- **Arbitrary npm deps** — packages without TS source or a Perry native binding
  won't compile (the Perry npm long-tail).

See [`DESIGN.md`](./DESIGN.md) for the full architecture and the native changes
this required in `perry-ui-macos`.
