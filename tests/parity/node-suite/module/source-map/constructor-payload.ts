import { SourceMap } from "node:module";

const payload = {
  version: 3,
  file: "out.js",
  sourceRoot: "../src/",
  sources: ["input.ts"],
  sourcesContent: ["export const value = 1;"],
  names: ["value"],
  mappings: "AAAAA",
};
const lengths = [12, 0, 7];
const map = new SourceMap(payload, { lineLengths: lengths });
console.log("instance:", map instanceof SourceMap);
console.log(
  "payload equal/not same:",
  JSON.stringify(map.payload) === JSON.stringify(payload),
  map.payload !== payload,
);
console.log(
  "nested cloned:",
  map.payload.sources !== payload.sources,
  map.payload.names !== payload.names,
  map.payload.sourcesContent !== payload.sourcesContent,
);
console.log(
  "line lengths:",
  JSON.stringify(map.lineLengths),
  map.lineLengths !== lengths,
);
payload.sources[0] = "mutated.ts";
lengths[0] = 999;
console.log(
  "payload isolated/lengths shared:",
  map.payload.sources[0],
  map.lineLengths[0],
);
console.log("entry:", JSON.stringify(map.findEntry(0, 0)));
