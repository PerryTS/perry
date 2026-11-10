import { constants } from "node:perf_hooks";
// node:perf_hooks exposes NODE_PERFORMANCE_GC_* numeric constants.
console.log("GC_MAJOR:", typeof constants.NODE_PERFORMANCE_GC_MAJOR);
console.log("GC_MINOR:", typeof constants.NODE_PERFORMANCE_GC_MINOR);
console.log("GC_FLAGS_NO:", typeof constants.NODE_PERFORMANCE_GC_FLAGS_NO);
