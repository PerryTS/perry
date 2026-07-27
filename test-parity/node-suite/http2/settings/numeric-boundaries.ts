import { getPackedSettings } from "node:http2";

const cases = [
  ["headerTableSize", 0],
  ["headerTableSize", 2 ** 32 - 1],
  ["initialWindowSize", 0],
  ["initialWindowSize", 2 ** 31 - 1],
  ["maxFrameSize", 16384],
  ["maxFrameSize", 2 ** 24 - 1],
  ["maxConcurrentStreams", 0],
  ["maxConcurrentStreams", 2 ** 32 - 1],
  ["maxHeaderListSize", 0],
  ["maxHeaderSize", 2 ** 32 - 1],
] as const;

for (const [key, value] of cases) {
  console.log(
    key,
    value,
    getPackedSettings({ [key]: value }).toString("hex"),
  );
}
