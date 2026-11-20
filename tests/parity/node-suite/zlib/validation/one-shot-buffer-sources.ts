import * as zlib from "node:zlib";

const arrayBuffer = new ArrayBuffer(1);
new Uint8Array(arrayBuffer)[0] = 120;

const sources: Array<[string, any]> = [
  ["string", "x"],
  ["buffer", Buffer.from("x")],
  ["uint8array", new Uint8Array([120])],
  ["dataview", new DataView(new Uint8Array([120]).buffer)],
  ["arraybuffer", arrayBuffer],
];

function crc32Shape(value: any) {
  try {
    return typeof zlib.crc32(value);
  } catch (error: any) {
    return `${error.name}:${error.code}`;
  }
}

for (const [label, value] of sources) {
  console.log(`${label} gzip buffer:`, Buffer.isBuffer(zlib.gzipSync(value)) ? "yes" : "no");
  console.log(`${label} crc32 shape:`, crc32Shape(value));
}
