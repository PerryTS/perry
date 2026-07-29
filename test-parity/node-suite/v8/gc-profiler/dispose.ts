import { GCProfiler } from "node:v8";

const profiler = new GCProfiler();
console.log("dispose method:", typeof profiler[Symbol.dispose]);
console.log("start twice:", profiler.start(), profiler.start());
console.log("first stop report:", typeof profiler.stop() === "object");
console.log("second stop:", profiler.stop());
console.log("dispose return:", profiler[Symbol.dispose]());

const active = new GCProfiler();
active.start();
console.log("active dispose:", active[Symbol.dispose]());
console.log("stop after dispose:", active.stop());
