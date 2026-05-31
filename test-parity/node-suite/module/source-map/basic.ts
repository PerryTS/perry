import * as Module from "node:module";

console.log("find missing:", Module.findSourceMap("/tmp/perry-no-source-map.js") === undefined);

for (const [label, make] of [
  ["missing", () => new Module.SourceMap()],
  ["null", () => new Module.SourceMap(null as never)],
  ["number", () => new Module.SourceMap(1 as never)],
] as const) {
  try {
    make();
    console.log(`invalid ${label}: ok`);
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    console.log(`invalid ${label}:`, e.name, e.code);
  }
}

const payload = { version: 3, sources: ["a.js"], names: ["mappedName"], mappings: "AAAA" };
const sourceMap = new Module.SourceMap(payload);
console.log("payload identity:", sourceMap.payload === payload);
console.log("payload sources cloned:", sourceMap.payload.sources !== payload.sources);
console.log("payload names cloned:", sourceMap.payload.names !== payload.names);
console.log("method lengths:", sourceMap.findEntry.length, sourceMap.findOrigin.length);

const entry = sourceMap.findEntry(0, 0);
console.log(
  "entry:",
  entry.generatedLine,
  entry.generatedColumn,
  entry.originalSource,
  entry.originalLine,
  entry.originalColumn,
  entry.name,
);

const entryNextColumn = sourceMap.findEntry(0, 1);
console.log(
  "entry next column:",
  entryNextColumn.generatedLine,
  entryNextColumn.generatedColumn,
  entryNextColumn.originalSource,
  entryNextColumn.originalLine,
  entryNextColumn.originalColumn,
  entryNextColumn.name,
);

const origin = sourceMap.findOrigin(1, 1);
console.log("origin:", origin.name, origin.fileName, origin.lineNumber, origin.columnNumber);
console.log("origin before mapping keys:", Object.keys(sourceMap.findOrigin(1, 0)).length);
