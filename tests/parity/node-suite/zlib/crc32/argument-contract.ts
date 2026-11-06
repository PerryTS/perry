import { crc32 } from "node:zlib";
import * as zlib from "node:zlib";

const captured = zlib.crc32;

function result(label: string, call: () => any) {
  try {
    console.log(label, "return", call());
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

const routes: Array<[string, (data: any, seed?: any) => number]> = [
  ["direct", (data, seed) => crc32(data, seed)],
  ["namespace", (data, seed) => zlib.crc32(data, seed)],
  ["captured", (data, seed) => captured(data, seed)],
];

const seeds: Array<[string, any]> = [
  ["undefined", undefined],
  ["negative-zero", -0],
  ["zero", 0],
  ["one", 1],
  ["maximum", 0xffffffff],
  ["negative", -1],
  ["overflow", 0x100000000],
  ["fraction", 1.5],
  ["nan", NaN],
  ["infinity", Infinity],
  ["string", "0"],
  ["null", null],
  ["boolean", true],
  ["object", {}],
];

for (const [name, call] of routes) {
  for (const [label, seed] of seeds) {
    result(`${name} seed ${label}`, () => call("abc", seed));
  }
  result(`${name} data before seed`, () => call(null, "invalid"));
  result(`${name} empty`, () => call("", 0));
  console.log(`${name} chaining`, call("bc", call("a")) === call("abc"));
}

console.log("omitted direct", crc32("abc"));
console.log("omitted namespace", zlib.crc32("abc"));
console.log("omitted captured", captured("abc"));

const view = new Uint8Array([97, 98, 99]);
const sources: Array<[string, any]> = [
  ["string", "abc"],
  ["buffer", Buffer.from("abc")],
  ["uint8array", view],
  ["dataview", new DataView(view.buffer)],
  ["arraybuffer", view.buffer],
];

for (const [name, call] of routes) {
  for (const [label, data] of sources) {
    result(`${name} data ${label}`, () => call(data, 0));
  }
}

console.log(
  "gzip ArrayBuffer unchanged",
  zlib.gunzipSync(zlib.gzipSync(view.buffer)).toString() === "abc",
);
