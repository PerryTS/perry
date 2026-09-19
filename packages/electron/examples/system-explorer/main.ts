// System Explorer — a representative real-world Electron app, running unmodified
// on Perry via the electron-compat shim. This is standard Electron main-process
// code: app lifecycle, a BrowserWindow with a preload, and several ipcMain
// handlers backed by real Node APIs (os, fs, child_process), plus a live
// main→renderer push (a 1Hz clock tick).
//
// Run on Perry:
//   perry main.ts -o system-explorer && ./system-explorer

import { app, BrowserWindow, ipcMain } from "electron";
import * as os from "os";
import * as fs from "fs";
import * as path from "path";

let mainWindow: BrowserWindow | null = null;
let tickTimer: any = null;

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 900,
    height: 640,
    title: "System Explorer (Perry × Electron)",
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
    },
  });

  mainWindow.loadFile(path.join(__dirname, "renderer", "index.html"));

  // Live push: send the time to the renderer every second (ipcRenderer.on).
  tickTimer = setInterval(() => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send("clock:tick", new Date().toISOString());
    }
  }, 1000);
}

// ---- IPC handlers (ipcRenderer.invoke -> these) ----

ipcMain.handle("system:info", async () => {
  return {
    platform: os.platform(),
    arch: os.arch(),
    hostname: os.hostname(),
    cpus: os.cpus().length,
    cpuModel: os.cpus()[0] ? os.cpus()[0].model : "unknown",
    totalMemMB: Math.round(os.totalmem() / (1024 * 1024)),
    freeMemMB: Math.round(os.freemem() / (1024 * 1024)),
    uptimeSec: Math.round(os.uptime()),
    release: os.release(),
    homedir: os.homedir(),
  };
});

ipcMain.handle("fs:list", async (_event, dirPath: string) => {
  const target = dirPath && dirPath.length > 0 ? dirPath : os.homedir();
  const entries = fs.readdirSync(target);
  const out: Array<{ name: string; isDir: boolean; sizeBytes: number }> = [];
  for (let i = 0; i < entries.length; i++) {
    const name = entries[i];
    if (name.charAt(0) === ".") continue; // skip dotfiles for a cleaner demo
    const full = path.join(target, name);
    try {
      const st = fs.statSync(full);
      out.push({ name: name, isDir: st.isDirectory(), sizeBytes: st.size });
    } catch (e) {
      /* unreadable entry — skip */
    }
  }
  out.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name < b.name ? -1 : 1;
  });
  return { dir: target, entries: out };
});

ipcMain.handle("fs:read", async (_event, filePath: string) => {
  const st = fs.statSync(filePath);
  if (st.size > 256 * 1024) {
    return { ok: false, error: "File too large to preview (>256KB)" };
  }
  const text = fs.readFileSync(filePath, "utf8");
  return { ok: true, text: text.slice(0, 20000) };
});

// Persisted notes — exercises userData path + write/read round trip.
function notesPath(): string {
  const dir = app.getPath("userData");
  try {
    fs.mkdirSync(dir, { recursive: true });
  } catch (e) {}
  return path.join(dir, "notes.json");
}

ipcMain.handle("notes:load", async () => {
  try {
    const text = fs.readFileSync(notesPath(), "utf8");
    return JSON.parse(text);
  } catch (e) {
    return [];
  }
});

ipcMain.handle("notes:save", async (_event, notes: string[]) => {
  fs.writeFileSync(notesPath(), JSON.stringify(notes, null, 2), "utf8");
  return { ok: true, count: notes.length };
});

// Fire-and-forget from renderer (ipcRenderer.send -> ipcMain.on).
ipcMain.on("log", (_event, message: string) => {
  console.log("[renderer] " + message);
});

// ---- App lifecycle (standard Electron) ----

app.whenReady().then(() => {
  console.log("System Explorer ready — creating window");
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (tickTimer) clearInterval(tickTimer);
  if (process.platform !== "darwin") app.quit();
});
