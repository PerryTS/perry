import { deserialize, serialize } from "node:v8";

for (
  const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["true", true],
    ["false", false],
    ["zero", 0],
    ["negative-zero", -0],
    ["integer", 42],
    ["fraction", -1.25],
    ["nan", NaN],
    ["infinity", Infinity],
    ["string", "héllo"],
    ["bigint", 9007199254740993n],
  ] as const
) {
  const result = deserialize(serialize(value));
  const normalized = typeof result === "bigint"
    ? result + "n"
    : Number.isNaN(result)
    ? "NaN"
    : Object.is(result, -0)
    ? "-0"
    : String(result);
  console.log(label + ":", typeof result, normalized);
}
