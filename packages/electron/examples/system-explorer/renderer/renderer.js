// Renderer — plain web JS calling the contextBridge-exposed `window.api`,
// which routes over the Perry IPC bridge to the native main process.
const api = window.api;

function fmtBytes(n) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KB";
  return (n / (1024 * 1024)).toFixed(1) + " MB";
}

// ---- System info (invoke/handle round trip) ----
async function loadSystem() {
  const info = await api.getSystemInfo();
  const rows = [
    ["Platform", info.platform + " / " + info.arch],
    ["Hostname", info.hostname],
    ["CPU", info.cpuModel],
    ["Cores", String(info.cpus)],
    ["Memory", info.freeMemMB + " / " + info.totalMemMB + " MB free"],
    ["Uptime", Math.round(info.uptimeSec / 60) + " min"],
    ["Release", info.release],
  ];
  const sysinfo = document.getElementById("sysinfo");
  sysinfo.replaceChildren();
  rows.forEach(function (row) {
    const item = document.createElement("div");
    item.className = "kv";
    const key = document.createElement("span");
    const value = document.createElement("span");
    key.textContent = row[0];
    value.textContent = row[1];
    item.append(key, value);
    sysinfo.appendChild(item);
  });
  api.log("system info rendered: " + info.hostname);
}

// ---- File listing + preview ----
let lastDir = "";
async function loadFiles() {
  const res = await api.listDir("");
  lastDir = res.dir;
  document.getElementById("crumbs").textContent = res.dir;
  const filesEl = document.getElementById("files");
  filesEl.replaceChildren();
  res.entries.forEach(function (entry) {
    const row = document.createElement("div");
    row.className = "file";
    row.dataset.name = entry.name;
    row.dataset.dir = entry.isDir ? "1" : "0";

    const icon = document.createElement("span");
    icon.className = "ic";
    icon.textContent = entry.isDir ? "📁" : "📄";
    const name = document.createElement("span");
    name.textContent = entry.name;
    row.append(icon, name);
    if (!entry.isDir) {
      const size = document.createElement("span");
      size.className = "sz";
      size.textContent = fmtBytes(entry.sizeBytes);
      row.appendChild(size);
    }
    filesEl.appendChild(row);
  });
  filesEl.querySelectorAll(".file").forEach(function (el) {
    el.addEventListener("click", async function () {
      if (el.dataset.dir === "1") return;
      const name = el.dataset.name;
      const fileRes = await api.readFile(lastDir + "/" + name);
      const pre = document.getElementById("preview");
      pre.textContent = fileRes.ok ? fileRes.text : "⚠️ " + fileRes.error;
    });
  });
}

// ---- Notes (persisted) ----
let notes = [];
async function loadNotes() {
  notes = await api.loadNotes();
  renderNotes();
}
function renderNotes() {
  const list = document.getElementById("noteList");
  list.replaceChildren();
  notes.forEach(function (note) {
    const item = document.createElement("div");
    item.className = "note";
    item.textContent = String(note);
    list.appendChild(item);
  });
}

document.getElementById("addNote").addEventListener("click", async function () {
  const input = document.getElementById("noteInput");
  const v = input.value.trim();
  if (!v) return;
  notes.unshift(v);
  input.value = "";
  renderNotes();
  const r = await api.saveNotes(notes);
  api.log("saved " + r.count + " notes");
});

// ---- Live clock push (main → renderer) ----
api.onClockTick(function (iso) {
  const d = new Date(iso);
  document.getElementById("clock").textContent = d.toLocaleTimeString();
});

loadSystem();
loadFiles();
loadNotes();
