import { createDeflate, deflateSync } from "node:zlib";
import * as zlib from "node:zlib";

// Public construction and one-shot calls: no private stream fields or large allocations.
const captured = zlib.createDeflate;
const routes: Array<[string, (options: any) => void]> = [
  ["named", (options) => { createDeflate(options).destroy(); }],
  ["namespace", (options) => { zlib.createDeflate(options).destroy(); }],
  ["captured", (options) => { captured(options).destroy(); }],
  ["deflateSync", (options) => { deflateSync("payload", options); }],
  ["gzipSync", (options) => { zlib.gzipSync("payload", options); }],
  ["deflateRawSync", (options) => { zlib.deflateRawSync("payload", options); }],
];
for (const name of [
  "createInflate", "createDeflateRaw", "createInflateRaw",
  "createGzip", "createGunzip", "createUnzip",
] as const) {
  routes.push([name, (options) => { zlib[name](options).destroy(); }]);
}

function result(label: string, call: () => void) {
  try {
    call();
    console.log(label, "accepted");
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

const values: Array<[string, any]> = [
  ["undefined", undefined], ["null", null], ["NaN", NaN],
  ["Infinity", Infinity], ["-Infinity", -Infinity],
  ["zero", 0], ["below-min", 63], ["min", 64],
  ["fraction", 64.5], ["small", 128], ["default", 16384],
  ["string", "64"],
];
for (const [name, call] of routes) {
  result(`${name} absent`, () => call({}));
  for (const [label, value] of values) {
    result(`${name} ${label}`, () => call({ chunkSize: value }));
  }
  result(`${name} throwing getter`, () => call({
    get chunkSize() { throw new Error("chunkSize getter"); },
  }));
}

// Baseline differential controls for the sibling sizing reader; no HWM change.
for (const options of [
  { highWaterMark: 2 },
  { readableHighWaterMark: 3, writableHighWaterMark: 4 },
  { highWaterMark: 2, readableHighWaterMark: 3, writableHighWaterMark: 4 },
]) {
  const stream = createDeflate(options);
  console.log("HWM control", stream.readableHighWaterMark, stream.writableHighWaterMark);
  stream.destroy();
}
