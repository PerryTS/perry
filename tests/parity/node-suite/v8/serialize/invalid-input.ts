import { deserialize } from "node:v8";

for (
  const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["string", "bad"],
    ["object", {}],
    ["arraybuffer", new ArrayBuffer(4)],
  ] as const
) {
  try {
    deserialize(value as any);
    console.log(label + ": no throw");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
