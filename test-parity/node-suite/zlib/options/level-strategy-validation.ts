import { constants, createDeflate } from "node:zlib";

for (
  const [key, values] of [
    ["level", ["1", -2, -1, 0, 9, 10, NaN, Infinity]],
    ["memLevel", ["1", 0, 1, 9, 10, Infinity]],
    ["strategy", [
      "0",
      -1,
      constants.Z_DEFAULT_STRATEGY,
      constants.Z_FIXED,
      5,
      NaN,
      Infinity,
    ]],
  ] as const
) {
  for (const value of values) {
    try {
      const stream = createDeflate({ [key]: value } as any);
      console.log(key, String(value), "ok");
      stream.destroy();
    } catch (error: any) {
      console.log(key, String(value), error.name, error.code);
    }
  }
}
