import * as os from "node:os";

// ── #3007: os.cpus() reports real per-core data on macOS ──────────────
const cpus = os.cpus();
console.log("cpus.nonEmpty:", cpus.length > 0);
console.log("cpus.realModel:", cpus.some((c) => !!c.model && c.model !== "unknown"));
console.log("cpus.speed:", cpus.some((c) => c.speed > 0));
console.log("cpus.times:", cpus.some((c) => Object.values(c.times).some((v) => v > 0)));
console.log("cpus.timesNum:", typeof cpus[0].times.user === "number");
console.log("cpus.modelStr:", typeof cpus[0].model === "string");

// ── #3006: os.networkInterfaces() reports real MACs on non-Linux ──────
const ni = os.networkInterfaces();
const rows = Object.entries(ni).flatMap(([name, addrs]) =>
  (addrs || []).map((addr) => ({ name, internal: addr.internal, mac: addr.mac })),
);
console.log("net.nonZeroMac:", rows.some((r) => !r.internal && r.mac !== "00:00:00:00:00:00"));
console.log("net.macShape:", rows.every((r) => /^[0-9a-f:]+$/.test(r.mac)));

// ── #3005: os.tmpdir() trailing-slash + precedence (TMPDIR is set) ─────
console.log("tmpdir:", os.tmpdir());

// ── #3004: os.userInfo({ encoding }) honors dynamic options ───────────
const optsLiteral = { encoding: "buffer" };
const optsVar = optsLiteral;
const optsCall = (() => ({ encoding: "buffer" }))();
const optsComputed = { ["encoding"]: "buffer" };

for (const opts of [optsVar, optsCall, optsComputed]) {
  const info = os.userInfo(opts as any);
  console.log("userInfo.username:", Buffer.isBuffer(info.username));
  console.log("userInfo.homedir:", Buffer.isBuffer(info.homedir));
}

// Default (string) form still returns strings.
const str = os.userInfo();
console.log("userInfo.defaultStr:", typeof str.username === "string");

// Non-"buffer" encoding stays a string.
const strOpts = { encoding: "utf8" };
const str2 = os.userInfo(strOpts as any);
console.log("userInfo.utf8Str:", typeof str2.username === "string");
