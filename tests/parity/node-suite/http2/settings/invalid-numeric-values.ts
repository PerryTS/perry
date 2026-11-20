import { getPackedSettings } from "node:http2";

for (
  const [key, value] of [
    ["headerTableSize", -1],
    ["initialWindowSize", 2 ** 31],
    ["maxFrameSize", 16383],
    ["maxFrameSize", 2 ** 24],
    ["maxConcurrentStreams", 2 ** 32],
  ] as const
) {
  try {
    getPackedSettings({ [key]: value });
  } catch (error: any) {
    console.log(key, error.name, error.code);
  }
}
