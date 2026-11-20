import { isBuiltin } from "node:module";

for (
  const value of [undefined, null, true, 0, 1n, Symbol("x"), {}, [], () => {}]
) {
  let result: unknown;
  try {
    result = isBuiltin(value as any);
  } catch (error) {
    result = `${(error as any).name}:${(error as any).code ?? "no-code"}`;
  }
  console.log(
    typeof value,
    Object.prototype.toString.call(value),
    String(result),
  );
}

console.log(
  "receiver ignored:",
  isBuiltin.call(null, "fs"),
  isBuiltin.call({ fake: true }, "node:path"),
);
