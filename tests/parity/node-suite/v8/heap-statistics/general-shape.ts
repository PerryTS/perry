import { getHeapStatistics } from "node:v8";

const stats: any = getHeapStatistics();
const expected = [
  "does_zap_garbage",
  "external_memory",
  "heap_size_limit",
  "malloced_memory",
  "number_of_detached_contexts",
  "number_of_native_contexts",
  "peak_malloced_memory",
  "total_allocated_bytes",
  "total_available_size",
  "total_global_handles_size",
  "total_heap_size",
  "total_heap_size_executable",
  "total_physical_size",
  "used_global_handles_size",
  "used_heap_size",
];
const values = expected.map((key) => stats[key]);
console.log("keys:", Object.keys(stats).sort().join(","));
console.log(
  "exact:",
  Object.keys(stats).sort().join(",") === expected.join(","),
);
console.log("types:", values.every((value) => typeof value === "number"));
console.log(
  "finite nonnegative:",
  values.every((value) => Number.isFinite(value) && value >= 0),
);
console.log(
  "heap relation:",
  stats.used_heap_size <= stats.total_heap_size,
  stats.used_global_handles_size <= stats.total_global_handles_size,
);
