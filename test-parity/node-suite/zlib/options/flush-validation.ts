import { constants, createGzip } from "node:zlib";

for (
  const [key, value] of [
    ["flush", constants.Z_SYNC_FLUSH],
    ["flush", "sync"],
    ["flush", 10000],
    ["finishFlush", constants.Z_SYNC_FLUSH],
    ["finishFlush", "sync"],
    ["finishFlush", 10000],
  ] as const
) {
  try {
    const stream = createGzip({ [key]: value });
    console.log(key, String(value), "ok");
    stream.destroy();
  } catch (error: any) {
    console.log(key, String(value), error.name, error.code);
  }
}
