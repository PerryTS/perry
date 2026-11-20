import { getHeapCodeStatistics } from "node:v8";

const stats: any = getHeapCodeStatistics();
const expected = [
  "bytecode_and_metadata_size",
  "code_and_metadata_size",
  "cpu_profiler_metadata_size",
  "external_script_source_size",
];
const values = expected.map((key) => stats[key]);
console.log("keys:", Object.keys(stats).sort().join(","));
console.log(
  "exact:",
  Object.keys(stats).sort().join(",") === expected.join(","),
);
console.log("numbers:", values.every((value) => typeof value === "number"));
console.log(
  "finite nonnegative:",
  values.every((value) => Number.isFinite(value) && value >= 0),
);
