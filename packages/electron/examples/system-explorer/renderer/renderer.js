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
  document.getElementById("sysinfo").innerHTML = rows
    .map((r) => '<div class="kv"><span>' + r[0] + "</span><span>" + r[1] + "</span></div>")
    .join("");
  api.log("system info rendered: " + info.hostname);
}

// ---- File listing + preview ----
let lastDir = "";
async function loadFiles() {
  const res = await api.listDir("");
  lastDir = res.dir;
  document.getElementById("crumbs").textContent = res.dir;
  const html = res.entries
    .map(function (e) {
      const ic = e.isDir ? "📁" : "📄";
      const sz = e.isDir ? "" : '<span class="sz">' + fmtBytes(e.sizeBytes) + "</span>";
      return (
        '<div class="file" data-name="' +
        e.name +
        '" data-dir="' +
        (e.isDir ? "1" : "0") +
        '"><span class="ic">' +
        ic +
        '</span><span>' +
        e.name +
        "</span>" +
        sz +
        "</div>"
      );
    })
    .join("");
  const filesEl = document.getElementById("files");
  filesEl.innerHTML = html;
  filesEl.querySelectorAll(".file").forEach(function (el) {
    el.addEventListener("click", async function () {
      if (el.getAttribute("data-dir") === "1") return;
      const name = el.getAttribute("data-name");
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
  document.getElementById("noteList").innerHTML = notes
    .map(function (n) {
      return '<div class="note">' + escapeHtml(n) + "</div>";
    })
    .join("");
}
function escapeHtml(s) {
  return String(s).replace(/[&<>]/g, function (c) {
    return c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;";
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
