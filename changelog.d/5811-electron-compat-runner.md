- Add a `perry electron <path>` subcommand that runs an existing Electron app
  natively on macOS: it locates the main entry from a package.json directory, a
  packaged `.app` bundle (`Contents/Resources/app.asar`, then
  `Contents/Resources/app/`), an `.asar` archive, or a main-process file;
  compiles the main process (ESM or CommonJS) with `electron` imports redirected
  to the `packages/electron` compat shim; and launches the native binary with the
  app root as its cwd. `.asar` archives are unpacked by a new bounds-checked
  in-tree reader into a per-user cache (`<cache dir>/perry/electron-apps`) that
  is reused while the archive is unchanged. The cache deliberately sits outside
  `node_modules`: under Perry's project cache dir, module collection classified
  the extracted app's own `main.cjs` as an untrusted npm package named `.cache`
  and refused the build as runtime JavaScript. A unit test pins the default root.
  A bundle's unpacked `Contents/Resources/app/` builds with its cache under the
  per-user cache dir too, so the installed (signed) app is never modified. The
  shim is found next to the `perry` binary or via `PERRY_ELECTRON_SHIM`.
- Add a repeatable `--package-alias FROM=TO` compile flag. `TO` is an npm
  package name or an absolute source path, merged after
  `perry.packageAliases` (the flag wins on a duplicate `FROM`).
- Add `packages/electron` (published as `@perryts/electron`): an Electron API
  shell (`app`, `BrowserWindow`, `webContents`, `ipcMain`, `Menu`, `dialog`,
  `Tray`, `clipboard`, `shell`, `nativeImage`) on `perry/ui`, plus a renderer
  bridge (`ipcRenderer`, `contextBridge`, preload `require`) injected as
  document-start user scripts. The preload runner is deleted before page
  scripts run, so pages see only `contextBridge`-exposed APIs. IPC is JSON;
  a non-serializable handler result rejects the renderer's `invoke`.
- Native plumbing in `perry-ui-macos`: a WKWebView renderer→main message bridge
  (`webviewSetOnMessage`), document-start user scripts
  (`webviewAddUserScript`), a body-less app loop entered from the true top level
  of generated `main` (`appRequestLoop` + the runtime's `js_ui_loop_take_over`)
  so windows created in `app.whenReady().then(...)` composite, `appQuit`,
  full-window body pinning, and activation for windows opened mid-loop. The
  pending `whenReady` closure is a registered GC root so a moving collection
  cannot leave it dangling. Universal file access stays off; `file://` pages get
  containing-directory read access only.
- `perry run <dir>`: a `perry.toml` `entry` (`[project] entry` or top-level)
  that is missing or malformed is now an error instead of silently falling back
  to `src/main.ts`.
- `perry init` always syncs the built-in `perry/*` tsconfig path, mirrors
  `perry.packageAliases` into `compilerOptions.paths`, and rebases generated
  paths onto an existing `baseUrl`.
- Add `packages/perry-react` (`@perryts/react`): types that augment
  `react-dom/client`'s `createRoot` with Perry's native-window overload, taking
  the React 18/19 type packages as peers.
