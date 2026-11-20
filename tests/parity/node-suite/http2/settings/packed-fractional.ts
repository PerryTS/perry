import { getPackedSettings } from "node:http2";

// In-range fractions truncate to uint32; the range check sees the raw number.
const cases = [
  ["headerTableSize", 1.5],
  ["headerTableSize", 2 ** 32 - 1 + 0.5],
  ["headerTableSize", -0.5],
  ["maxConcurrentStreams", 7.25],
  ["initialWindowSize", 2147483646.9],
  ["initialWindowSize", 2147483647.5],
  ["maxFrameSize", 16384.5],
  ["maxFrameSize", 16383.5],
  ["maxFrameSize", 2 ** 24 - 1 + 0.5],
  ["maxHeaderListSize", 0.9],
  ["maxHeaderSize", 3.99],
] as const;

for (const [key, value] of cases) {
  try {
    console.log(
      key,
      value,
      getPackedSettings({ [key]: value }).toString("hex"),
    );
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(key, value, e.name, e.code, e.message);
  }
}
