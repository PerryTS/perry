import * as zlib from "node:zlib";

console.log("zlib keys includes codes:", Object.keys(zlib).includes("codes"));
console.log("codes type:", typeof (zlib as any).codes);
console.log("codes stable identity:", (zlib as any).codes === (zlib as any).codes);
console.log("codes keys:", JSON.stringify(Object.keys((zlib as any).codes)));
console.log(
  "code values:",
  [
    (zlib as any).codes.Z_OK,
    (zlib as any).codes.Z_STREAM_END,
    (zlib as any).codes.Z_NEED_DICT,
    (zlib as any).codes.Z_ERRNO,
    (zlib as any).codes.Z_STREAM_ERROR,
    (zlib as any).codes.Z_DATA_ERROR,
    (zlib as any).codes.Z_MEM_ERROR,
    (zlib as any).codes.Z_BUF_ERROR,
    (zlib as any).codes.Z_VERSION_ERROR,
  ].join(","),
);
console.log(
  "reverse values:",
  [
    (zlib as any).codes[0],
    (zlib as any).codes[1],
    (zlib as any).codes[2],
    (zlib as any).codes[-1],
    (zlib as any).codes[-2],
    (zlib as any).codes[-3],
    (zlib as any).codes[-4],
    (zlib as any).codes[-5],
    (zlib as any).codes[-6],
  ].join(","),
);
