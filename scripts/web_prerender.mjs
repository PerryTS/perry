#!/usr/bin/env node
// web_prerender.mjs: snapshot a `perry --target web` page into static HTML.
//
// A Perry web page ships an empty `<div id="perry-root"></div>` and builds every element
// after the WASM boots, so a crawler (or a reader before the script runs) sees a blank
// page. This loads the page in headless Chrome, takes the rendered root, and writes it back
// into the HTML as `<div id="perry-root" data-perry-prerendered>...</div>`. At boot,
// perry_ui_app_create sees the attribute and replaces the snapshot with the live UI.
//
//   node scripts/web_prerender.mjs <page.html> [--url <url>] [--out <file>] [--chrome <exe>]
//
// --url renders a served page (data the app fetches, or a route other than /); the
// snapshot is still written into <page.html> (or --out). Requires Chrome or Chromium.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const args = process.argv.slice(2);
if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
  console.log("usage: node scripts/web_prerender.mjs <page.html> [--url <url>] [--out <file>] [--chrome <exe>]");
  process.exit(args.length === 0 ? 2 : 0);
}
const opt = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const page = resolve(args[0]);
const out = resolve(opt("--out") ?? page);
const url = opt("--url") ?? pathToFileURL(page).href;
const chrome = opt("--chrome") ?? [
  process.env.CHROME,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser",
].find((p) => p && existsSync(p));
if (!chrome) { console.error("web_prerender: no Chrome found; pass --chrome <exe>"); process.exit(1); }

const html = readFileSync(page, "utf8");
const EMPTY_ROOT = '<div id="perry-root"></div>';
if (!html.includes(EMPTY_ROOT)) {
  console.error("web_prerender: " + page + " has no empty perry-root (already prerendered?)");
  process.exit(1);
}

const dom = execFileSync(chrome, [
  "--headless=new", "--disable-gpu", "--allow-file-access-from-files",
  "--virtual-time-budget=5000", "--dump-dom", url,
], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, stdio: ["ignore", "pipe", "ignore"] });

// The root is the element right before the page's first <script>; take what lies between
// its opening tag and its closing </div>.
const open = dom.indexOf('<div id="perry-root"');
const openEnd = open < 0 ? -1 : dom.indexOf(">", open);
const scriptAt = openEnd < 0 ? -1 : dom.indexOf("<script", openEnd);
const close = scriptAt < 0 ? -1 : dom.lastIndexOf("</div>", scriptAt);
if (open < 0 || openEnd < 0 || close <= openEnd) {
  console.error("web_prerender: could not find the rendered perry-root in " + url);
  process.exit(1);
}
const inner = dom.slice(openEnd + 1, close);
if (inner.trim() === "") {
  console.error("web_prerender: the page rendered an empty root; nothing to snapshot");
  process.exit(1);
}
const rootTag = dom.slice(open, openEnd).replace(/\sdata-perry-prerendered(="[^"]*")?/, "");
writeFileSync(out, html.replace(EMPTY_ROOT, rootTag + " data-perry-prerendered>" + inner + "</div>"));
console.log("web_prerender: wrote " + out + " (" + inner.length + " bytes of rendered root)");
