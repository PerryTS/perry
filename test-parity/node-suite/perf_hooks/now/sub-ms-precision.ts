import { performance } from "node:perf_hooks";
// High-resolution readings are finite numbers. Fractional output is common but
// not a portable contract for every clock/platform combination.
const readings = [performance.now(), performance.now(), performance.now()];
console.log("finite:", readings.every(Number.isFinite));
console.log(
  "ordered:",
  readings[0] <= readings[1] && readings[1] <= readings[2],
);
