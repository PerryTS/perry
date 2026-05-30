import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_process_loadenv_syntax";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_err) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/syntax.env";

const keys = [
  "A",
  "B",
  "C",
  "D",
  "E",
  "MULTI",
  "EMPTY",
  "PRESET",
  "BAD-NAME",
  "NO_EQUALS",
];

for (const key of keys) {
  delete process.env[key];
}
process.env.PRESET = "existing";

fs.writeFileSync(
  file,
  [
    "A=1",
    "B = two # comment",
    'C="three # not comment"',
    "D=unquoted value # comment",
    "export E=5",
    'MULTI="line1',
    'line2"',
    "EMPTY=",
    "PRESET=from-file",
    "BAD-NAME=bad",
    "NO_EQUALS",
    "",
  ].join("\n"),
);

const ret = process.loadEnvFile(file);
console.log("return:", ret === undefined);
for (const key of keys) {
  const value = process.env[key];
  console.log(key + ":", value === undefined ? "undefined" : JSON.stringify(value));
}
