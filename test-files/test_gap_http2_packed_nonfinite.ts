// http2.getPackedSettings with NaN / ±Infinity numeric settings.
// NaN passes Node's range check and packs as 0; ±Infinity is a RangeError
// whose message renders the value with String().
import * as http2 from "node:http2";

const names = [
  "headerTableSize",
  "maxConcurrentStreams",
  "initialWindowSize",
  "maxHeaderListSize",
];

for (const name of names) {
  for (const value of [NaN, Infinity, -Infinity]) {
    try {
      const packed = http2.getPackedSettings({ [name]: value });
      console.log(name, String(value), "packed", packed.toString("hex"));
    } catch (e: any) {
      console.log(name, String(value), "threw", e.name, e.code, e.message);
    }
  }
}

// Finite fractional controls keep their raw-range-then-truncate behavior.
console.log(http2.getPackedSettings({ headerTableSize: 1.5 }).toString("hex"));
console.log(http2.getPackedSettings({ maxFrameSize: 16384.5 }).toString("hex"));
try {
  http2.getPackedSettings({ initialWindowSize: 2147483647.5 });
} catch (e: any) {
  console.log(e.name, e.code, e.message);
}
