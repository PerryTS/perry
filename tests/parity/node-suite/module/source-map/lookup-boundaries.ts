import { SourceMap } from "node:module";

const map = new SourceMap({
  version: 3,
  sources: ["a.ts", "b.ts"],
  names: ["first", "second"],
  mappings: "AAAAA,KAAKC;ACCL",
});

for (
  const [line, column] of [[0, -1], [0, 0], [0, 4], [0, 5], [0, 100], [1, 0], [
    2,
    0,
  ]]
) {
  console.log(
    `entry ${line},${column}:`,
    JSON.stringify(map.findEntry(line, column)),
  );
}
for (
  const args of [[1, 1], ["a.ts", 1, 1], ["b.ts", 2, 1], [
    "missing.ts",
    1,
    1,
  ]] as any[]
) {
  console.log(
    "origin",
    JSON.stringify(args),
    JSON.stringify(map.findOrigin(...args)),
  );
}
