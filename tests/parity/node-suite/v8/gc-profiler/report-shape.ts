// #3142: node:v8 GCProfiler constructor and minimal report shape.
import * as v8 from "node:v8";
import { GCProfiler } from "node:v8";

console.log("exports:", typeof v8.GCProfiler, v8.GCProfiler.length);

const profiler: any = new v8.GCProfiler();
console.log("methods:", typeof profiler.start, typeof profiler.stop);
console.log("stop before start:", profiler.stop() === undefined);
console.log("start returns:", profiler.start() === undefined);

const report = profiler.stop();
const keys = Object.keys(report);
console.log("report keys:", keys.join(","));
console.log(
  "report types:",
  typeof report.version,
  typeof report.startTime,
  Array.isArray(report.statistics),
  typeof report.endTime,
);
console.log("stop after stop:", profiler.stop() === undefined);

const second: any = new v8.GCProfiler();
second.start();
console.log("independent:", Object.keys(second.stop()).includes("statistics"));

const named: any = new GCProfiler();
named.start();
console.log("named import:", Object.keys(named.stop()).includes("statistics"));
