// Gap test: node:process miscellaneous parity.
//   #3045 — process.loadEnvFile(path?) argument handling (string/Buffer/URL,
//           omitted/undefined/null default to .env, invalid-type throws)
//   #3052 — process.emit("error", ...) unhandled-error throwing semantics
//   #3108 — process.sourceMapsEnabled + setSourceMapsEnabled(bool) toggle
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import * as fs from "node:fs";
import * as path from "node:path";

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "OK");
  } catch (err) {
    const e = err as { name: string; code?: string; message: string };
    console.log(label, "THROW", e.name, e.code, e.message.split("\n")[0]);
  }
}

// ---- #3108: sourceMapsEnabled toggle round-trip ----
console.log("sme typeof", typeof process.sourceMapsEnabled, typeof process.setSourceMapsEnabled);
console.log("sme initial", process.sourceMapsEnabled);
console.log("sme set true ret", process.setSourceMapsEnabled(true));
console.log("sme after true", process.sourceMapsEnabled);
console.log("sme set false ret", process.setSourceMapsEnabled(false));
console.log("sme after false", process.sourceMapsEnabled);
show("sme missing", () => process.setSourceMapsEnabled());
show("sme null", () => process.setSourceMapsEnabled(null));
show("sme number", () => process.setSourceMapsEnabled(1));
show("sme string", () => process.setSourceMapsEnabled("x"));
show("sme object", () => process.setSourceMapsEnabled({}));
show("sme array", () => process.setSourceMapsEnabled([]));

// ---- #3052: emit("error", ...) unhandled-error semantics ----
show("err noarg", () => process.emit("error"));
show("err string", () => process.emit("error", "boom"));
show("err number", () => process.emit("error", 42));
show("err null", () => process.emit("error", null));
show("err bool", () => process.emit("error", true));

const customErr = new Error("custom");
(customErr as { code?: string }).code = "EFOO";
let rethrown: unknown = undefined;
try {
  process.emit("error", customErr);
} catch (e) {
  rethrown = e;
}
console.log("err instance same", rethrown === customErr, (rethrown as { code?: string }).code);

// With an error listener registered, emit fires it and returns true.
let listenerSaw: unknown = "none";
process.on("error", (v: unknown) => {
  listenerSaw = v;
});
const emitRet = process.emit("error", "handled");
console.log("err with listener", emitRet, listenerSaw);
process.removeAllListeners("error");

// ---- #3045: loadEnvFile path handling ----
const dir = fs.mkdtempSync(path.join(process.cwd(), "perry-loadenv-"));
const envPath = path.join(dir, "config.env");
fs.writeFileSync(envPath, "PERRY_GAP_LOADENV=ok\n");

show("loadenv string", () => {
  delete process.env.PERRY_GAP_LOADENV;
  process.loadEnvFile(envPath);
});
console.log("loadenv string value", process.env.PERRY_GAP_LOADENV);

show("loadenv buffer", () => {
  delete process.env.PERRY_GAP_LOADENV;
  process.loadEnvFile(Buffer.from(envPath));
});
console.log("loadenv buffer value", process.env.PERRY_GAP_LOADENV);

show("loadenv url", () => {
  delete process.env.PERRY_GAP_LOADENV;
  process.loadEnvFile(new URL("file://" + envPath));
});
console.log("loadenv url value", process.env.PERRY_GAP_LOADENV);

// Invalid argument types throw ERR_INVALID_ARG_TYPE before opening .env.
show("loadenv number", () => process.loadEnvFile(123));
show("loadenv boolean", () => process.loadEnvFile(true));
show("loadenv object", () => process.loadEnvFile({}));
show("loadenv array", () => process.loadEnvFile([]));

// Omitted / undefined / null default to ".env" in cwd (absent here → ENOENT).
show("loadenv omitted", () => process.loadEnvFile());
show("loadenv undefined", () => process.loadEnvFile(undefined));
show("loadenv null", () => process.loadEnvFile(null));

fs.rmSync(dir, { recursive: true, force: true });
