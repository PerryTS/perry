import { serialize } from "node:v8";

for (
  const [label, value] of [
    ["function", () => 1],
    ["symbol", Symbol("x")],
    ["weakmap", new WeakMap()],
    ["promise", Promise.resolve(1)],
  ] as const
) {
  try {
    serialize(value);
    console.log(label + ": no throw");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
