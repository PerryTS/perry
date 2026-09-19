// AUTO-WRAPPED from bridge/preload-runtime.js — do not edit here; edit the .js and re-wrap.
// The renderer-side Electron-compat runtime, injected into every webview as a
// document-start user script (before the app preload) via webviewAddUserScript.
export const PRELOAD_RUNTIME: string = `// Renderer-side Electron-compat runtime.
//
// Injected into every webview as a document-start WKUserScript (before the app's own
// preload). Provides \`require('electron')\` → { ipcRenderer, contextBridge } and the
// transport to the native (Perry-compiled) main process.
//
// Transport:
//   renderer -> main : window.webkit.messageHandlers.perry.postMessage(jsonString)
//                      (WebView2: window.chrome.webview.postMessage; GTK: same shape)
//   main -> renderer : native evals window.__perryDeliver / __perryResolve into the page
(function () {
  "use strict";
  if (window.__perryBridgeInstalled) return;
  window.__perryBridgeInstalled = true;

  function postToNative(msg) {
    var json = JSON.stringify(msg);
    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.perry) {
      window.webkit.messageHandlers.perry.postMessage(json); // WKWebView (macOS/iOS)
    } else if (window.chrome && window.chrome.webview) {
      window.chrome.webview.postMessage(json);               // WebView2 (Windows)
    } else if (window.__perryNativePost) {
      window.__perryNativePost(json);                        // GTK / fallback
    }
    // (no native channel → silently drop; console forwarding below would recurse)
  }

  // Forward renderer console + uncaught errors to the native main process, so
  // they land in the terminal (Electron pipes the renderer console to stdout via
  // devtools; we do it over the bridge). Also the author's diagnostic window.
  function fwd(level, args) {
    try {
      var parts = [];
      for (var i = 0; i < args.length; i++) {
        var a = args[i];
        parts.push(typeof a === "string" ? a : safeStringify(a));
      }
      postToNative({ kind: "console", id: 0, channel: level, args: [parts.join(" ")] });
    } catch (e) {}
  }
  function safeStringify(v) {
    try { return JSON.stringify(v); } catch (e) { return String(v); }
  }
  var realConsole = window.console || {};
  ["log", "info", "warn", "error", "debug"].forEach(function (level) {
    var orig = realConsole[level] ? realConsole[level].bind(realConsole) : function () {};
    realConsole[level] = function () {
      fwd(level, arguments);
      orig.apply(null, arguments);
    };
  });
  window.addEventListener("error", function (e) {
    fwd("error", ["Uncaught " + (e.message || "error") + (e.filename ? " @ " + e.filename + ":" + e.lineno : "")]);
  });
  window.addEventListener("unhandledrejection", function (e) {
    fwd("error", ["Unhandled rejection: " + (e.reason && e.reason.message ? e.reason.message : safeStringify(e.reason))]);
  });

  var nextInvokeId = 1;
  var pending = Object.create(null);          // id -> {resolve, reject}
  var listeners = Object.create(null);        // channel -> [fn,...]

  // ---- main -> renderer entry points (called by native via evaluateJs) ----
  window.__perryResolve = function (id, ok, payloadJson) {
    var p = pending[id];
    if (!p) return;
    delete pending[id];
    var value = payloadJson === undefined ? undefined : JSON.parse(payloadJson);
    if (ok) p.resolve(value);
    else p.reject(Object.assign(new Error(value && value.message ? value.message : "ipc error"), value || {}));
  };
  window.__perryDeliver = function (channel, payloadJson) {
    var fns = listeners[channel];
    if (!fns || !fns.length) return;
    var args = payloadJson === undefined ? [] : JSON.parse(payloadJson);
    var event = { sender: ipcRenderer, senderId: 0 };
    for (var i = 0; i < fns.length; i++) {
      try { fns[i].apply(null, [event].concat(args)); }
      catch (e) { console.error("[perry-electron] listener for '" + channel + "' threw:", e); }
    }
  };

  // ---- ipcRenderer ----
  var ipcRenderer = {
    invoke: function (channel) {
      var args = Array.prototype.slice.call(arguments, 1);
      var id = nextInvokeId++;
      return new Promise(function (resolve, reject) {
        pending[id] = { resolve: resolve, reject: reject };
        postToNative({ kind: "invoke", id: id, channel: channel, args: args });
      });
    },
    send: function (channel) {
      var args = Array.prototype.slice.call(arguments, 1);
      postToNative({ kind: "send", id: 0, channel: channel, args: args });
    },
    sendSync: function () {
      throw new Error("[perry-electron] ipcRenderer.sendSync is not supported; use invoke()");
    },
    on: function (channel, fn) {
      (listeners[channel] || (listeners[channel] = [])).push(fn);
      return ipcRenderer;
    },
    once: function (channel, fn) {
      var wrap = function () { ipcRenderer.removeListener(channel, wrap); return fn.apply(this, arguments); };
      return ipcRenderer.on(channel, wrap);
    },
    removeListener: function (channel, fn) {
      var fns = listeners[channel];
      if (fns) listeners[channel] = fns.filter(function (f) { return f !== fn; });
      return ipcRenderer;
    },
    removeAllListeners: function (channel) {
      if (channel === undefined) listeners = Object.create(null);
      else delete listeners[channel];
      return ipcRenderer;
    },
  };

  // ---- contextBridge ----
  var contextBridge = {
    exposeInMainWorld: function (key, api) {
      // configurable:true so a renderer's top-level \`const x = window.x\` doesn't
      // throw "duplicate variable that shadows a global property" in JSC. (Real
      // Electron isolates the preload in a separate world; we share one world,
      // so the exposed name must not be a non-configurable global binding.)
      try {
        Object.defineProperty(window, key, {
          value: Object.freeze(deepBind(api)),
          enumerable: true,
          writable: false,
          configurable: true,
        });
      } catch (e) {
        window[key] = deepBind(api); // some apps re-expose; be lenient
      }
    },
    exposeInIsolatedWorld: function (_worldId, key, api) { contextBridge.exposeInMainWorld(key, api); },
  };
  function deepBind(api) {
    if (typeof api === "function") return api;
    if (api && typeof api === "object") {
      var out = Array.isArray(api) ? [] : {};
      for (var k in api) out[k] = deepBind(api[k]);
      return out;
    }
    return api;
  }

  // ---- require() for the renderer ----
  // Provides require('electron'), and a CommonJS loader for LOCAL files
  // (require('./foo.js')) so old nodeIntegration-style apps that load their
  // scripts/jQuery via require keep working — we sync-fetch the file and eval it
  // as a CommonJS module. (Node *builtins* are not available in the webview.)
  var electron = { ipcRenderer: ipcRenderer, contextBridge: contextBridge, webFrame: { setZoomFactor: function () {}, setZoomLevel: function () {} } };
  var moduleCache = Object.create(null);

  function loadSync(url) {
    try {
      var xhr = new XMLHttpRequest();
      xhr.open("GET", url, false); // sync — required for CommonJS require() semantics
      xhr.send();
      if (xhr.status === 0 || (xhr.status >= 200 && xhr.status < 300)) return xhr.responseText;
    } catch (e) {}
    return null;
  }
  function resolveUrl(name, base) {
    try { return new URL(name, base).href; } catch (e) { return name; }
  }
  function makeRequire(baseUrl) {
    return function (name) {
      if (name === "electron") return electron;
      if (name.charAt(0) === "." || name.charAt(0) === "/") {
        var url = resolveUrl(name, baseUrl);
        var src = loadSync(url);
        if (src === null && url.slice(-3) !== ".js") { url = url + ".js"; src = loadSync(url); }
        if (src === null) throw new Error("[perry-electron] require: cannot load '" + name + "'");
        if (moduleCache[url]) return moduleCache[url].exports;
        var mod = { exports: {} };
        moduleCache[url] = mod;
        var dir = url.replace(/\\/[^/]*$/, "/");
        try {
          var fn = new Function("module", "exports", "require", "__filename", "__dirname", src);
          fn(mod, mod.exports, makeRequire(dir), url, dir);
        } catch (e) {
          delete moduleCache[url];
          throw e;
        }
        return mod.exports;
      }
      throw new Error("[perry-electron] require('" + name + "') is not available in the renderer (no Node builtins in the system webview)");
    };
  }
  window.require = makeRequire(window.location ? window.location.href : "");

  window.electron = electron;           // also expose directly for apps using window.electron
  window.__perryIpcRenderer = ipcRenderer;

  // Diagnostic heartbeat so the main process can confirm the bridge installed
  // at document-start and the message channel is live.
  postToNative({ kind: "console", id: 0, channel: "debug", args: ["[perry-bridge] installed; channel=" + (!!(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.perry)) ] });
})();
`;
