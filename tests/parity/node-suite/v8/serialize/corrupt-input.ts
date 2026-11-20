import { deserialize } from "node:v8";

for (
  const [label, value] of [
    ["empty", Buffer.alloc(0)],
    ["header only", Buffer.from([0xff, 0x0f])],
    ["random", Buffer.from([1, 2, 3, 4])],
    ["truncated object", Buffer.from("ff0f6f220161", "hex")],
  ] as const
) {
  try {
    deserialize(value);
    console.log(label + ": no throw");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
