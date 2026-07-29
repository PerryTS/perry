// electron — Electron-compatible app shell for Perry.
//
// Existing Electron app code (`import { app, BrowserWindow, ipcMain } from
// 'electron'`) runs unmodified. App logic compiles natively; the view runs in
// the OS-native webview. Internals are the Tauri model (system webview, single
// native process); the public surface is Electron's.
//
// Main process (this file, compiled native): app, BrowserWindow, ipcMain, Menu,
// dialog, webContents. Renderer/preload (ipcRenderer, contextBridge) is the JS
// in ./preload-runtime.ts injected into each webview.

import { EventEmitter } from "events";
import {
  Window,
  WebView,
  webviewLoadUrl,
  webviewEvaluateJs,
  webviewSetOnMessage,
  webviewSetOnLoaded,
  webviewSetOnError,
  webviewAddUserScript,
  appRequestLoop,
  appQuit,
  clipboardRead,
  clipboardWrite,
  trayCreate,
  traySetIcon,
  traySetTooltip,
  trayAttachMenu,
  trayOnClick,
  trayDestroy,
  menuCreate,
  menuAddItem,
  menuAddSeparator,
  menuAddSubmenu,
  menuAddItemWithShortcut,
  menuAddStandardAction,
  menuBarCreate,
  menuBarAddMenu,
  menuBarAttach,
  openFileDialog,
  openFolderDialog,
  saveFileDialog,
  onActivate,
} from "perry/ui";
import * as os from "os";
import * as path from "path";
import * as fs from "fs";
import * as childProcess from "child_process";
import { PRELOAD_RUNTIME } from "./preload-runtime";

// ---------------------------------------------------------------------------
// Small helpers for marshalling values across the webview boundary.
// ---------------------------------------------------------------------------

function jsStringLiteral(s: string): string {
  // Produce a JS source string literal for embedding in an evaluateJs payload.
  return JSON.stringify(s);
}

function ipcJson(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch (err) {
    throw new TypeError("IPC payload must be JSON-serializable: " + errMessage(err));
  }
}

// Build `window.__perryResolve(id, ok, payload)` source. `payload` is the
// result re-encoded as a JS string literal of its JSON (double-encoded), which
// the renderer JSON.parses. undefined results pass the bare `undefined` token.
function resolveJs(id: number, ok: boolean, value: unknown): string {
  let arg3: string;
  if (value === undefined) {
    arg3 = "undefined";
  } else {
    arg3 = jsStringLiteral(ipcJson(value));
  }
  return "window.__perryResolve(" + id + "," + (ok ? "true" : "false") + "," + arg3 + ")";
}

function deliverJs(channel: string, args: unknown[]): string {
  const argsJson = ipcJson(args);
  return "window.__perryDeliver(" + jsStringLiteral(channel) + "," + jsStringLiteral(argsJson) + ")";
}

function serializationFailureJs(id: number, err: unknown): string {
  return resolveJs(id, false, {
    message: errMessage(err),
  });
}

// ---------------------------------------------------------------------------
// ipcMain
// ---------------------------------------------------------------------------

type IpcInvokeHandler = (event: IpcMainInvokeEvent, ...args: any[]) => any;

interface IpcMainInvokeEvent {
  sender: WebContents;
  frameId: number;
  processId: number;
}

interface IpcMainEvent {
  sender: WebContents;
  reply: (channel: string, ...args: any[]) => void;
  frameId: number;
  processId: number;
  returnValue?: any;
}

class IpcMain extends EventEmitter {
  // channel -> invoke handler (for ipcRenderer.invoke / ipcMain.handle).
  // Initialized in the constructor (not as a field initializer): Perry does not
  // reliably run field initializers on EventEmitter subclasses.
  private handlers: { [channel: string]: IpcInvokeHandler };

  constructor() {
    super();
    this.handlers = {};
  }

  handle(channel: string, listener: IpcInvokeHandler): void {
    this.handlers[channel] = listener;
  }
  handleOnce(channel: string, listener: IpcInvokeHandler): void {
    const self = this;
    this.handlers[channel] = function (event: IpcMainInvokeEvent, ...args: any[]) {
      delete self.handlers[channel];
      return listener(event, ...args);
    };
  }
  removeHandler(channel: string): void {
    delete this.handlers[channel];
  }

  // Internal: dispatch a parsed message coming from a webview.
  _dispatch(win: BrowserWindow, msg: { kind: string; id: number; channel: string; args: any[] }): void {
    const wc = win.webContents;
    if (msg.kind === "console") {
      // Renderer console / errors forwarded over the bridge.
      const text = msg.args && msg.args.length > 0 ? String(msg.args[0]) : "";
      if (msg.channel === "error") console.error("[renderer-console] " + text);
      else console.log("[renderer-console] " + text);
      return;
    }
    if (msg.kind === "invoke") {
      const handler = this.handlers[msg.channel];
      const invokeEvent: IpcMainInvokeEvent = { sender: wc, frameId: 0, processId: 0 };
      if (!handler) {
        wc._eval(resolveJs(msg.id, false, { message: "No handler registered for '" + msg.channel + "'" }));
        return;
      }
      try {
        const result = handler(invokeEvent, ...msg.args);
        Promise.resolve(result).then(
          (value) => {
            try {
              wc._eval(resolveJs(msg.id, true, value));
            } catch (err) {
              wc._eval(serializationFailureJs(msg.id, err));
            }
          },
          (err) => wc._eval(resolveJs(msg.id, false, { message: errMessage(err) }))
        );
      } catch (err) {
        wc._eval(resolveJs(msg.id, false, { message: errMessage(err) }));
      }
    } else if (msg.kind === "send") {
      const event: IpcMainEvent = {
        sender: wc,
        reply: (channel: string, ...args: any[]) => wc.send(channel, ...args),
        frameId: 0,
        processId: 0,
      };
      this.emit(msg.channel, event, ...msg.args);
    }
  }
}

function errMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in (err as any)) return String((err as any).message);
  return String(err);
}

export const ipcMain = new IpcMain();

// ---------------------------------------------------------------------------
// WebContents — the per-window bridge to its webview.
// ---------------------------------------------------------------------------

class WebContents extends EventEmitter {
  // The perry/ui WebView widget handle for this window.
  _wv: any;
  id: number;

  constructor(id: number) {
    super();
    this._wv = 0;
    this.id = id;
  }

  _eval(js: string): void {
    if (this._wv) {
      webviewEvaluateJs(this._wv, js, (_result: string) => {});
    }
  }

  // main -> renderer push (ipcRenderer.on receives it)
  send(channel: string, ...args: any[]): void {
    this._eval(deliverJs(channel, args));
  }

  executeJavaScript(code: string): Promise<any> {
    return new Promise((resolve) => {
      if (!this._wv) {
        resolve(undefined);
        return;
      }
      webviewEvaluateJs(this._wv, code, (result: string) => resolve(result));
    });
  }

  openDevTools(): void {
    /* no-op: system webview devtools are opened via the OS inspector */
  }
  closeDevTools(): void {}
  setWindowOpenHandler(_handler: (details: any) => any): void {}
  get session(): any {
    return { clearStorageData: () => Promise.resolve() };
  }
}

// ---------------------------------------------------------------------------
// BrowserWindow
// ---------------------------------------------------------------------------

interface BrowserWindowOptions {
  width?: number;
  height?: number;
  title?: string;
  show?: boolean;
  webPreferences?: { preload?: string; [k: string]: any };
  [k: string]: any;
}

let nextWebContentsId = 1;
const openWindows: BrowserWindow[] = [];

// True if any open window is currently visible (for the 'activate' event's
// hasVisibleWindows arg — a hidden window must not count).
function anyWindowVisible(): boolean {
  for (let i = 0; i < openWindows.length; i++) {
    if (openWindows[i].isVisible()) return true;
  }
  return false;
}

// Drop a one-shot native callback from the GC-root array once it has fired
// (dialog completion handlers only need to live until the native side replies).
function unrootNativeUi(cb: any): void {
  const i = rootedNativeUi.indexOf(cb);
  if (i >= 0) rootedNativeUi.splice(i, 1);
}

// Keeps the app-ready closure passed to appRequestLoop alive from module init
// until the native loop fires it after main top-level (GC root).
let rootedOnReady: (() => void) | null = null;
// Same GC-root rationale for the dock-activate closure handed to the native
// onActivate hook — held until the user clicks the dock icon.
let rootedOnActivate: (() => void) | null = null;
// Native-held menubar/tray handles + their click closures. The native side
// stores these; rooting them here keeps the menubar/tray and every menu-item
// callback alive for the app's lifetime (otherwise GC could reclaim them).
const rootedNativeUi: any[] = [];

class BrowserWindow extends EventEmitter {
  // perry/ui window operations, captured as closures over the LOCAL `win` in
  // the constructor. This is required: perry/ui instance-method dispatch
  // (setBody/show/…) only fires on a local flow-tracked from the `Window()`
  // call — calling on a property receiver (`this.win.show()`) compiles to a
  // generic no-op, so the window would never composite or respond. A closure
  // capturing the local preserves the dispatch.
  private _show: () => void;
  private _hide: () => void;
  private _close: () => void;
  private _setSize: (w: number, h: number) => void;
  webContents: WebContents;
  private destroyed: boolean;
  private _visible: boolean;
  private preloadPath: string | undefined;

  constructor(options?: BrowserWindowOptions) {
    super();
    this.destroyed = false;
    this._visible = false;
    const opts = options || {};
    const width = opts.width || 800;
    const height = opts.height || 600;
    const title = opts.title || "";
    this.preloadPath = opts.webPreferences && opts.webPreferences.preload;

    const win = Window(title, width, height);
    this._show = () => win.show();
    this._hide = () => win.hide();
    this._close = () => win.close();
    this._setSize = (w: number, h: number) => win.setSize(w, h);
    this.webContents = new WebContents(nextWebContentsId++);

    // Servo does not yet implement the document-start preload and renderer IPC
    // contracts required by BrowserWindow. Explicitly select the system backend
    // for Electron windows until those APIs exist instead of returning a Servo
    // handle whose bridge setup would be silently ignored.
    const previousWebview = process.env.PERRY_WEBVIEW;
    process.env.PERRY_WEBVIEW = "system";
    // Start without an initial navigation so a later loadURL/loadFile promise
    // cannot be resolved by an in-flight about:blank navigation.
    const wv = WebView({ url: "", width, height });
    if (previousWebview === undefined) delete process.env.PERRY_WEBVIEW;
    else process.env.PERRY_WEBVIEW = previousWebview;
    this.webContents._wv = wv;

    // Inject the IPC bridge runtime + the app's preload (document-start),
    // before any page loads. Order matters: bridge first so `require('electron')`
    // and `contextBridge` exist when the app's preload runs.
    webviewAddUserScript(wv, PRELOAD_RUNTIME);
    if (this.preloadPath) {
      const preloadSrc = tryReadFile(this.preloadPath);
      if (preloadSrc) {
        const preloadAbs = path.isAbsolute(this.preloadPath)
          ? this.preloadPath
          : path.join(process.cwd(), this.preloadPath);
        webviewAddUserScript(
          wv,
          "window.__perryRunPreload(" +
            jsStringLiteral(preloadSrc) +
            "," +
            jsStringLiteral("file://" + preloadAbs) +
            ");"
        );
      }
    }
    // The temporary runner is present only while trusted document-start scripts
    // execute. Page scripts never receive require/electron/ipcRenderer.
    webviewAddUserScript(wv, "delete window.__perryRunPreload;");

    // Fire webContents 'did-finish-load' / 'dom-ready' when the page loads, so
    // apps that push to the renderer after load (a very common pattern) work.
    const self = this;
    webviewSetOnLoaded(wv, (_url: string) => {
      self.webContents.emit("did-finish-load");
      self.webContents.emit("dom-ready");
    });
    webviewSetOnError(wv, (code: number, message: string) => {
      self.webContents.emit("did-fail-load", code, message);
    });

    // Route inbound IPC from this window's renderer to ipcMain.
    webviewSetOnMessage(wv, (json: string) => {
      let msg: any;
      try {
        msg = JSON.parse(json);
      } catch (e) {
        return;
      }
      ipcMain._dispatch(self, msg);
    });

    win.setBody(wv);
    if (opts.show !== false) {
      win.show();
      this._visible = true;
    }
    openWindows.push(this);
  }

  loadURL(url: string): Promise<void> {
    return this._load(url);
  }

  loadFile(filePath: string, _options?: any): Promise<void> {
    // Resolve relative to cwd; Electron resolves relative to the app dir.
    const abs = path.isAbsolute(filePath) ? filePath : path.join(process.cwd(), filePath);
    return this._load("file://" + abs);
  }

  private _load(url: string): Promise<void> {
    const wc = this.webContents;
    return new Promise((resolve, reject) => {
      const onLoaded = () => {
        wc.removeListener("did-fail-load", onFailed);
        resolve();
      };
      const onFailed = (code: number, message: string) => {
        wc.removeListener("did-finish-load", onLoaded);
        reject(new Error("Navigation failed (" + code + "): " + message));
      };
      wc.once("did-finish-load", onLoaded);
      wc.once("did-fail-load", onFailed);
      webviewLoadUrl(wc._wv, url);
    });
  }

  show(): void {
    this._show();
    this._visible = true;
  }
  hide(): void {
    this._hide();
    this._visible = false;
  }
  isVisible(): boolean {
    return this._visible && !this.destroyed;
  }
  close(): void {
    if (this.destroyed) return;
    this.emit("close");
    this._close();
    this.destroyed = true;
    this._visible = false;
    const idx = openWindows.indexOf(this);
    if (idx >= 0) openWindows.splice(idx, 1);
    this.emit("closed");
    if (openWindows.length === 0) {
      app.emit("window-all-closed");
    }
  }
  destroy(): void {
    this.close();
  }
  isDestroyed(): boolean {
    return this.destroyed;
  }
  setSize(width: number, height: number): void {
    this._setSize(width, height);
  }
  setTitle(_title: string): void {
    /* perry/ui window title is set at create; runtime retitle is a follow-up */
  }
  focus(): void {
    this._show();
  }

  static getAllWindows(): BrowserWindow[] {
    return openWindows.slice();
  }
  static getFocusedWindow(): BrowserWindow | null {
    return openWindows.length > 0 ? openWindows[openWindows.length - 1] : null;
  }
}

function tryReadFile(p: string): string | null {
  try {
    const abs = path.isAbsolute(p) ? p : path.join(process.cwd(), p);
    return fs.readFileSync(abs, "utf8");
  } catch (e) {
    console.error("[electron] failed to read preload '" + p + "': " + errMessage(e));
    return null;
  }
}

// ---------------------------------------------------------------------------
// app
// ---------------------------------------------------------------------------

class App extends EventEmitter {
  private ready: boolean;
  private readyResolvers: Array<() => void>;
  private loopScheduled: boolean;
  private appName: string;
  dock: { hide: () => void; show: () => void; setIcon: (icon: any) => void };

  constructor() {
    super();
    this.ready = false;
    this.readyResolvers = [];
    this.loopScheduled = false;
    this.appName = "Perry App";
    this.dock = { hide: () => {}, show: () => {}, setIcon: (_icon: any) => {} };
    // Register the native UI event loop. This does NOT block — the generated
    // `main` enters the loop at the top level AFTER the user's `main` top-level
    // code runs (registering ipcMain/whenReady handlers). Entering the loop at
    // the top level (rather than from a microtask) is what lets windows created
    // in `whenReady().then(...)` actually composite on screen.
    this.startLoop();
  }

  private startLoop(): void {
    if (this.loopScheduled) return;
    this.loopScheduled = true;
    const self = this;
    // onReady fires when the loop takes over (top level), resolving whenReady();
    // the `.then(createWindow)` chain then runs on the first pump tick. Held in
    // a module-level slot (rootedOnReady) so GC can't collect it before then.
    rootedOnReady = () => {
      self.ready = true;
      self.emit("ready");
      const resolvers = self.readyResolvers;
      self.readyResolvers = [];
      for (let i = 0; i < resolvers.length; i++) resolvers[i]();
    };
    appRequestLoop(rootedOnReady);
    // macOS 'activate' — fired when the dock icon is clicked (incl. after all
    // windows are closed). Electron apps reopen their window from here.
    // hasVisibleWindows must reflect *visible* windows, not merely open ones
    // (a hidden window must not suppress the reopen pattern).
    rootedOnActivate = () => {
      self.emit("activate", {}, anyWindowVisible());
    };
    onActivate(rootedOnActivate);
  }

  whenReady(): Promise<void> {
    if (this.ready) return Promise.resolve();
    const self = this;
    return new Promise<void>((resolve) => {
      self.readyResolvers.push(resolve);
    });
  }

  isReady(): boolean {
    return this.ready;
  }

  quit(): void {
    this.emit("before-quit");
    this.emit("will-quit");
    appQuit();
  }
  exit(_code?: number): void {
    appQuit();
  }

  getName(): string {
    return this.appName;
  }
  setName(name: string): void {
    this.appName = name;
  }
  getVersion(): string {
    return "0.0.0";
  }
  getAppPath(): string {
    return process.cwd();
  }
  getPath(name: string): string {
    const home = os.homedir();
    switch (name) {
      case "home":
        return home;
      case "appData":
        return path.join(home, "Library", "Application Support");
      case "userData":
        return path.join(home, "Library", "Application Support", this.appName);
      case "temp":
        return os.tmpdir();
      case "desktop":
        return path.join(home, "Desktop");
      case "documents":
        return path.join(home, "Documents");
      case "downloads":
        return path.join(home, "Downloads");
      case "exe":
        return process.execPath || process.cwd();
      default:
        return home;
    }
  }
  requestSingleInstanceLock(): boolean {
    return true;
  }
  setActivationPolicy(_policy: string): void {}
}

export const app = new App();

// ---------------------------------------------------------------------------
// Menu / MenuItem
//
// Menu.buildFromTemplate accepts the Electron template; setApplicationMenu wires
// it into the native macOS menubar (perry/ui menuBar*). Items with a `click`
// callback fire it; `role` items map to the standard AppKit selector
// (copy/paste/undo/quit/…) sent to the first responder; `type: "separator"`
// and nested `submenu` are supported. Accelerators ("CmdOrCtrl+S") are
// normalized to perry/ui's "Cmd+Shift+S" syntax.
// ---------------------------------------------------------------------------

// Electron accelerator → perry/ui shortcut. CmdOrCtrl collapses to Cmd on
// macOS; Super/Meta → Cmd; everything else passes through to parse_shortcut.
function normalizeAccelerator(accel: string): string {
  return accel
    .replace(/CommandOrControl/gi, "Cmd")
    .replace(/CmdOrCtrl/gi, "Cmd")
    .replace(/Command/gi, "Cmd")
    .replace(/Control/gi, "Ctrl")
    .replace(/Option/gi, "Alt")
    .replace(/Super/gi, "Cmd")
    .replace(/Meta/gi, "Cmd");
}

// Electron menu role → AppKit first-responder selector (so copy/paste/etc.
// act on the focused webview). Roles without a standard selector fall through
// to a plain labeled item.
const ROLE_SELECTORS: { [role: string]: string } = {
  undo: "undo:",
  redo: "redo:",
  cut: "cut:",
  copy: "copy:",
  paste: "paste:",
  pasteandmatchstyle: "pasteAsPlainText:",
  delete: "delete:",
  selectall: "selectAll:",
  quit: "terminate:",
  minimize: "performMiniaturize:",
  close: "performClose:",
  togglefullscreen: "toggleFullScreen:",
  hide: "hide:",
  hideothers: "hideOtherApplications:",
  unhide: "unhideAllApplications:",
};

const ROLE_LABELS: { [role: string]: string } = {
  undo: "Undo", redo: "Redo", cut: "Cut", copy: "Copy", paste: "Paste",
  pasteandmatchstyle: "Paste and Match Style", delete: "Delete",
  selectall: "Select All", quit: "Quit", minimize: "Minimize", close: "Close",
  togglefullscreen: "Toggle Full Screen", hide: "Hide", hideothers: "Hide Others",
  unhide: "Show All",
};

// A Menu instance carries `.items`; a raw template submenu is an array. (The
// shim's buildFromTemplate only wraps the top level, so nested submenus arrive
// as raw template objects — read fields generically.)
function menuItemList(submenu: any): any[] {
  if (!submenu) return [];
  if (Array.isArray(submenu)) return submenu;
  if (submenu.items && Array.isArray(submenu.items)) return submenu.items;
  return [];
}

// Build a native perry/ui menu handle from a list of Electron menu items.
function buildNativeMenu(items: any[]): any {
  const native = menuCreate();
  rootedNativeUi.push(native);
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    if (item.type === "separator") {
      menuAddSeparator(native);
      continue;
    }
    if (item.submenu) {
      const child = buildNativeMenu(menuItemList(item.submenu));
      menuAddSubmenu(native, item.label || "", child);
      continue;
    }
    const role = item.role ? String(item.role).toLowerCase() : "";
    if (role && ROLE_SELECTORS[role]) {
      const label = item.label || ROLE_LABELS[role] || "";
      const accel = item.accelerator ? normalizeAccelerator(item.accelerator) : "";
      menuAddStandardAction(native, label, ROLE_SELECTORS[role], accel);
      continue;
    }
    const label = item.label || ROLE_LABELS[role] || "";
    const click = item.click;
    const cb = () => {
      if (typeof click === "function") click();
    };
    rootedNativeUi.push(cb);
    if (item.accelerator) {
      menuAddItemWithShortcut(native, label, normalizeAccelerator(item.accelerator), cb);
    } else {
      menuAddItem(native, label, cb);
    }
  }
  return native;
}

class MenuItem {
  label: string;
  click?: () => void;
  submenu?: any;
  role?: string;
  type?: string;
  accelerator?: string;
  constructor(opts: any) {
    this.label = opts.label || "";
    this.click = opts.click;
    this.submenu = opts.submenu;
    this.role = opts.role;
    this.type = opts.type;
    this.accelerator = opts.accelerator;
  }
}

let appMenu: Menu | null = null;

class Menu {
  items: MenuItem[];
  constructor() {
    this.items = [];
  }
  append(item: MenuItem): void {
    this.items.push(item);
  }
  popup(_options?: any): void {}

  // Build the native menubar this Menu represents: each top-level item becomes
  // a bar menu whose contents are that item's submenu.
  _buildNativeMenuBar(): any {
    const bar = menuBarCreate();
    rootedNativeUi.push(bar);
    for (let i = 0; i < this.items.length; i++) {
      const top = this.items[i];
      const sub = buildNativeMenu(menuItemList(top.submenu));
      menuBarAddMenu(bar, top.label || "", sub);
    }
    return bar;
  }

  static buildFromTemplate(template: any[]): Menu {
    const menu = new Menu();
    for (let i = 0; i < template.length; i++) {
      menu.append(new MenuItem(template[i]));
    }
    return menu;
  }
  static setApplicationMenu(menu: Menu | null): void {
    appMenu = menu;
    if (menu) {
      menuBarAttach(menu._buildNativeMenuBar());
    } else {
      // No native "remove menubar" API; attach a fresh empty bar so the app's
      // custom menus are cleared rather than left live (keeps the native state
      // consistent with getApplicationMenu() === null).
      const empty = menuBarCreate();
      rootedNativeUi.push(empty);
      menuBarAttach(empty);
    }
  }
  static getApplicationMenu(): Menu | null {
    return appMenu;
  }
}

export { Menu, MenuItem, BrowserWindow, WebContents };

// ---------------------------------------------------------------------------
// dialog — native NSOpenPanel/NSSavePanel via perry/ui. The native dialogs are
// single-selection; `properties: ["openDirectory"]` picks the folder panel.
// Both options-only and (window, options) call signatures are accepted.
// ---------------------------------------------------------------------------

// dialog calls come as showXxx(options) or showXxx(browserWindow, options).
function dialogOptions(a?: any, b?: any): any {
  if (b !== undefined) return b || {};
  // Single arg: a BrowserWindow has no dialog option keys, so treat an object
  // with `properties`/`defaultPath`/`filters`/`title` as the options bag.
  if (a && (a.properties || a.defaultPath || a.filters || a.title)) return a;
  return a && !(a instanceof BrowserWindow) ? a : {};
}

export const dialog = {
  showOpenDialog(a?: any, b?: any): Promise<{ canceled: boolean; filePaths: string[] }> {
    const options = dialogOptions(a, b);
    const props: string[] = options.properties || [];
    const wantDir = props.indexOf("openDirectory") >= 0;
    return new Promise((resolve) => {
      const cb = (selected: string) => {
        unrootNativeUi(cb);
        if (!selected) resolve({ canceled: true, filePaths: [] });
        else resolve({ canceled: false, filePaths: [selected] });
      };
      rootedNativeUi.push(cb);
      if (wantDir) openFolderDialog(cb);
      else openFileDialog(cb);
    });
  },
  showSaveDialog(a?: any, b?: any): Promise<{ canceled: boolean; filePath: string }> {
    const options = dialogOptions(a, b);
    const defaultPath: string = options.defaultPath || "";
    const base = defaultPath ? path.basename(defaultPath) : "untitled";
    const ext = path.extname(base).replace(/^\./, "");
    return new Promise((resolve) => {
      const cb = (selected: string) => {
        unrootNativeUi(cb);
        if (!selected) resolve({ canceled: true, filePath: "" });
        else resolve({ canceled: false, filePath: selected });
      };
      rootedNativeUi.push(cb);
      saveFileDialog(cb, base, ext);
    });
  },
  showMessageBox(_window?: any, _options?: any): Promise<{ response: number; checkboxChecked: boolean }> {
    return Promise.resolve({ response: 0, checkboxChecked: false });
  },
  showErrorBox(title: string, content: string): void {
    console.error("[electron] " + title + ": " + content);
  },
};

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

export const shell = {
  openExternal(url: string): Promise<void> {
    try {
      childProcess.spawn("open", [url]);
    } catch (e) {
      console.error("[electron] shell.openExternal failed: " + errMessage(e));
    }
    return Promise.resolve();
  },
  openPath(p: string): Promise<string> {
    try {
      childProcess.spawn("open", [p]);
    } catch (e) {}
    return Promise.resolve("");
  },
  showItemInFolder(_p: string): void {},
  beep(): void {},
};

// ---------------------------------------------------------------------------
// clipboard — system pasteboard via perry/ui (text in v1).
// ---------------------------------------------------------------------------

export const clipboard = {
  readText(_type?: string): string {
    return clipboardRead();
  },
  writeText(text: string, _type?: string): void {
    clipboardWrite(text);
  },
  clear(_type?: string): void {
    clipboardWrite("");
  },
  availableFormats(_type?: string): string[] {
    return ["text/plain"];
  },
};

// ---------------------------------------------------------------------------
// nativeImage — thin wrapper carrying the source path (Tray/dock icons read it).
// ---------------------------------------------------------------------------

class NativeImage {
  // Source file path ("" for empty / unsupported sources).
  _path: string;
  constructor(p: string) {
    this._path = p;
  }
  isEmpty(): boolean {
    return this._path === "";
  }
  getSize(): { width: number; height: number } {
    return { width: 0, height: 0 };
  }
  toDataURL(): string {
    return "";
  }
}

// Accept a path string or a NativeImage anywhere Electron takes an "image".
function iconToPath(icon: any): string {
  if (typeof icon === "string") return icon;
  if (icon && typeof icon._path === "string") return icon._path;
  return "";
}

export const nativeImage = {
  createFromPath(p: string): NativeImage {
    return new NativeImage(p);
  },
  createEmpty(): NativeImage {
    return new NativeImage("");
  },
  createFromDataURL(_dataUrl: string): NativeImage {
    // v1 supports path-backed images only; data-URL/buffer sources return an
    // empty image. Warn so a missing Tray/dock icon isn't a silent surprise.
    console.warn("[electron] nativeImage.createFromDataURL is unsupported (returns an empty image)");
    return new NativeImage("");
  },
  createFromBuffer(_buffer: any, _options?: any): NativeImage {
    console.warn("[electron] nativeImage.createFromBuffer is unsupported (returns an empty image)");
    return new NativeImage("");
  },
};

// ---------------------------------------------------------------------------
// Tray — system menu-bar status item via perry/ui tray*.
// ---------------------------------------------------------------------------

class Tray extends EventEmitter {
  private _handle: any;
  private _destroyed: boolean;

  constructor(image: string | NativeImage) {
    super();
    this._destroyed = false;
    this._handle = trayCreate(iconToPath(image));
    rootedNativeUi.push(this._handle);
    const self = this;
    const onClick = () => self.emit("click");
    rootedNativeUi.push(onClick);
    trayOnClick(this._handle, onClick);
  }

  setImage(image: string | NativeImage): void {
    if (this._destroyed) return;
    traySetIcon(this._handle, iconToPath(image));
  }
  setToolTip(toolTip: string): void {
    if (this._destroyed) return;
    traySetTooltip(this._handle, toolTip);
  }
  setTitle(_title: string, _options?: any): void {
    /* status-item title text — not wired in v1 */
  }
  setContextMenu(menu: Menu | null): void {
    if (this._destroyed) return;
    // `null` detaches the current menu — attach a fresh empty native menu so
    // the previous context menu doesn't linger.
    const native = menu ? buildNativeMenu(menuItemList(menu)) : menuCreate();
    if (!menu) rootedNativeUi.push(native);
    trayAttachMenu(this._handle, native);
  }
  setPressedImage(_image: string | NativeImage): void {}
  popUpContextMenu(_menu?: any, _position?: any): void {}
  isDestroyed(): boolean {
    return this._destroyed;
  }
  destroy(): void {
    if (this._destroyed) return;
    trayDestroy(this._handle);
    this._destroyed = true;
  }
}

export { Tray };

// ---------------------------------------------------------------------------
// ipcRenderer / contextBridge — renderer-only in Electron. Provided here so a
// main-process `import { ipcRenderer } from 'electron'` doesn't crash; the real
// implementations run in the renderer (./preload-runtime.ts).
// ---------------------------------------------------------------------------

export const ipcRenderer = {
  invoke(_channel: string, ..._args: any[]): Promise<any> {
    return Promise.reject(new Error("ipcRenderer is only available in the renderer process"));
  },
  send(_channel: string, ..._args: any[]): void {},
  on(_channel: string, _listener: any): any {
    return ipcRenderer;
  },
  once(_channel: string, _listener: any): any {
    return ipcRenderer;
  },
  removeListener(_channel: string, _listener: any): any {
    return ipcRenderer;
  },
  removeAllListeners(_channel?: string): any {
    return ipcRenderer;
  },
};

export const contextBridge = {
  exposeInMainWorld(_key: string, _api: any): void {
    /* renderer-only; see preload-runtime.ts */
  },
};

export const nativeTheme = {
  shouldUseDarkColors: false,
  themeSource: "system",
  on: (_event: string, _cb: any) => {},
};

// Default export mirrors `const electron = require('electron')`.
export default {
  app,
  BrowserWindow,
  ipcMain,
  ipcRenderer,
  contextBridge,
  Menu,
  MenuItem,
  dialog,
  shell,
  clipboard,
  nativeImage,
  Tray,
  nativeTheme,
  WebContents,
};
