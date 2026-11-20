import { createHistogram } from "node:perf_hooks";
const source = createHistogram();
const target = createHistogram();
source.record(4);
source.record(8);
console.log("add return:", target.add(source) === undefined);
console.log("copied:", target.count, target.min, target.max);
source.record(16);
console.log("isolated:", target.count, target.max);
