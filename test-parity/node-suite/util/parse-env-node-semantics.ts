import util, { parseEnv } from "node:util";

const parsed = parseEnv(
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
    "A=last",
    "",
  ].join("\n"),
);

console.log("typeof:", typeof util.parseEnv, String(util.parseEnv === parseEnv));
console.log("null-proto:", String(Object.getPrototypeOf(parsed) === null));
console.log("keys:", Object.keys(parsed).join(","));
for (const key of [
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
]) {
  const value = (parsed as any)[key];
  console.log(key + ":", value === undefined ? "undefined" : JSON.stringify(value));
}

for (const [label, value] of [
  ["undefined", undefined],
  ["null", null],
  ["number", 123],
  ["buffer", Buffer.from("A=1\n")],
  [
    "object",
    {
      toString() {
        return "A=1\n";
      },
    },
  ],
  ["symbol", Symbol("x")],
] as const) {
  try {
    util.parseEnv(value as any);
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code);
  }
}
