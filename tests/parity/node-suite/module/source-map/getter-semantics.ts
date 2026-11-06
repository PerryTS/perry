import { SourceMap } from "node:module";

const payload = { version: 3, sources: ["a.ts"], names: [], mappings: "AAAA" };
const lengths = [4, 8];
const map = new SourceMap(payload, { lineLengths: lengths });
const firstPayload = map.payload;
const secondPayload = map.payload;
const firstLengths = map.lineLengths;
const secondLengths = map.lineLengths;
console.log(
  "getter identity:",
  firstPayload === secondPayload,
  firstLengths === secondLengths,
);
firstPayload.sources[0] = "changed.ts";
firstLengths[0] = 99;
console.log("mutation visibility:", map.payload.sources[0], map.lineLengths[0]);
console.log(
  "input links:",
  map.payload === payload,
  map.lineLengths === lengths,
);

const defaults = new SourceMap(payload);
console.log("default lengths:", String(defaults.lineLengths));
