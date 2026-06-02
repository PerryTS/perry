// #3904: node:v8 modern diagnostics/profiler named exports surface.
import * as v8 from "node:v8";
import {
  getCppHeapStatistics,
  getHeapSnapshot,
  isStringOneByteRepresentation,
  queryObjects,
  startCpuProfile,
  writeHeapSnapshot,
} from "node:v8";

for (const [name, value] of [
  ["getCppHeapStatistics", getCppHeapStatistics],
  ["getHeapSnapshot", getHeapSnapshot],
  ["isStringOneByteRepresentation", isStringOneByteRepresentation],
  ["queryObjects", queryObjects],
  ["startCpuProfile", startCpuProfile],
  ["writeHeapSnapshot", writeHeapSnapshot],
] as const) {
  console.log(name, typeof value, value.length, typeof (v8 as any)[name]);
}
