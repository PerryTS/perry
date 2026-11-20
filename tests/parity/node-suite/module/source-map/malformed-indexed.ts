import { SourceMap } from "node:module";

for (const mappings of ["", ";;;;;", "!", "AAAA,", ",AAAA"]) {
  const map = new SourceMap({
    version: 3,
    sources: ["a.ts"],
    names: [],
    mappings,
  });
  console.log(
    JSON.stringify(mappings),
    JSON.stringify(map.findEntry(0, 5)),
    JSON.stringify(map.findOrigin(1, 6)),
  );
}

const indexed = new SourceMap({
  version: 3,
  sections: [
    {
      offset: { line: 0, column: 0 },
      map: { version: 3, sources: ["a.ts"], names: [], mappings: "AAAA" },
    },
    {
      offset: { line: 2, column: 3 },
      map: { version: 3, sources: ["b.ts"], names: [], mappings: "AAAA" },
    },
  ],
});
console.log("indexed first:", JSON.stringify(indexed.findEntry(0, 0)));
console.log("indexed second:", JSON.stringify(indexed.findEntry(2, 3)));
