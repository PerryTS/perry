import * as zlib from "node:zlib";

for (
  const [level, strategy] of [
    ["1", 0],
    [-2, 0],
    [0, "0"],
    [0, -1],
    [0, 5],
  ] as any[]
) {
  const stream = zlib.createDeflate();
  try {
    stream.params(level, strategy);
    console.log(String(level), String(strategy), "ok");
  } catch (error: any) {
    console.log(String(level), String(strategy), error.name, error.code);
  } finally {
    stream.destroy();
  }
}
