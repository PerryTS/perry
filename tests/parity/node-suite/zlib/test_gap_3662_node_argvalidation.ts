// #3662 — node-core argument validation (fs / process / zlib sub-cluster).
//
// Where the spec/Node require a built-in to reject bad input, Perry must throw
// the same `TypeError [ERR_INVALID_ARG_TYPE]` / `RangeError [ERR_OUT_OF_RANGE]`
// rather than silently proceeding. Each probe prints the thrown error's `.name`
// and `.code` (and, for the cases that pin message parity, the full `.message`)
// so the output is compared byte-for-byte against
// `node --experimental-strip-types`.
//
// Scope note: this covers the zlib option/buffer validation, `process.emitWarning`
// argument validation, and `fs.mkdirSync` option validation. The collection
// brand checks (#3739) and `Function.prototype` not-callable checks (PR #4073)
// live in their own changes.
import * as fs from "node:fs";
import * as zlib from "node:zlib";

function probe(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code);
  }
}
function probeMsg(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code, "|", e.message);
  }
}

console.log("=== zlib: input buffer arg type ===");
probe("gzipSync(123)", () => zlib.gzipSync(123 as any));
probe("gzipSync(null)", () => zlib.gzipSync(null as any));
probe("gzipSync(undefined)", () => zlib.gzipSync(undefined as any));
probe("gzipSync(true)", () => zlib.gzipSync(true as any));
probe("gzipSync({})", () => zlib.gzipSync({} as any));
probe("deflateSync(123)", () => zlib.deflateSync(123 as any));
probe("gunzipSync(123)", () => zlib.gunzipSync(123 as any));
probe("inflateSync(null)", () => zlib.inflateSync(null as any));

console.log("=== zlib: gzipSync option validation ===");
probeMsg("level:'a'", () => zlib.gzipSync("x", { level: "a" } as any));
probeMsg("level:99", () => zlib.gzipSync("x", { level: 99 } as any));
probeMsg("level:-5", () => zlib.gzipSync("x", { level: -5 } as any));
probe("level:5.5 (ok)", () => zlib.gzipSync("x", { level: 5.5 } as any));
probeMsg("windowBits:8", () => zlib.gzipSync("x", { windowBits: 8 } as any));
probeMsg("windowBits:99", () => zlib.gzipSync("x", { windowBits: 99 } as any));
probeMsg("memLevel:0", () => zlib.gzipSync("x", { memLevel: 0 } as any));
probeMsg("memLevel:99", () => zlib.gzipSync("x", { memLevel: 99 } as any));
probeMsg("strategy:99", () => zlib.gzipSync("x", { strategy: 99 } as any));
probeMsg("strategy:'a'", () => zlib.gzipSync("x", { strategy: "a" } as any));
probeMsg("chunkSize:0", () => zlib.gzipSync("x", { chunkSize: 0 } as any));
probeMsg("chunkSize:-1", () => zlib.gzipSync("x", { chunkSize: -1 } as any));
probeMsg("chunkSize:'a'", () => zlib.gzipSync("x", { chunkSize: "a" } as any));
probeMsg("flush:'a'", () => zlib.gzipSync("x", { flush: "a" } as any));
probeMsg("flush:99", () => zlib.gzipSync("x", { flush: 99 } as any));

console.log("=== zlib: deflateSync windowBits (min 8, not 9) ===");
probe("deflate windowBits:8 (ok)", () => zlib.deflateSync("x", { windowBits: 8 } as any));
probeMsg("deflate windowBits:7", () => zlib.deflateSync("x", { windowBits: 7 } as any));

console.log("=== zlib: option order (first offender) ===");
probeMsg("chunkSize+level", () => zlib.gzipSync("x", { chunkSize: 0, level: 99 } as any));
probeMsg("level+windowBits", () => zlib.gzipSync("x", { level: 99, windowBits: 99 } as any));
probeMsg("bad-buffer+bad-opt", () => zlib.gzipSync(123 as any, { level: 99 } as any));

console.log("=== zlib: createGzip / createDeflate factory option validation ===");
probeMsg("createGzip level:99", () => zlib.createGzip({ level: 99 } as any));
probeMsg("createGzip windowBits:8", () => zlib.createGzip({ windowBits: 8 } as any));
probeMsg("createDeflate windowBits:7", () => zlib.createDeflate({ windowBits: 7 } as any));
probe("createDeflate windowBits:8 (ok)", () => zlib.createDeflate({ windowBits: 8 } as any));
probe("createGzip valid (ok)", () => zlib.createGzip({ level: 6 } as any));

console.log("=== process.emitWarning ===");
probeMsg("emitWarning(5)", () => (process as any).emitWarning(5));
probeMsg("emitWarning(true)", () => (process as any).emitWarning(true));
probeMsg("emitWarning(null)", () => (process as any).emitWarning(null));
probeMsg("emitWarning('ok', 5)", () => (process as any).emitWarning("ok", 5));

console.log("=== fs.mkdirSync option validation ===");
probeMsg("mkdirSync recursive:5", () => fs.mkdirSync("/tmp/perry-3662-x", { recursive: 5 } as any));
probeMsg("mkdirSync recursive:'x'", () => fs.mkdirSync("/tmp/perry-3662-x", { recursive: "x" } as any));
probeMsg("mkdirSync mode:{}", () => fs.mkdirSync("/tmp/perry-3662-x", { mode: {} } as any));
