// Standard Electron preload — runs in the renderer with contextBridge, exposing
// a safe `window.api` surface. Unmodified Electron code; the electron-compat
// bridge runtime provides `require('electron')` (ipcRenderer + contextBridge)
// inside the webview.
const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("api", {
  getSystemInfo: () => ipcRenderer.invoke("system:info"),
  listDir: (dirPath) => ipcRenderer.invoke("fs:list", dirPath),
  readFile: (filePath) => ipcRenderer.invoke("fs:read", filePath),
  loadNotes: () => ipcRenderer.invoke("notes:load"),
  saveNotes: (notes) => ipcRenderer.invoke("notes:save", notes),
  log: (message) => ipcRenderer.send("log", message),
  onClockTick: (cb) => ipcRenderer.on("clock:tick", (_event, iso) => cb(iso)),
});
