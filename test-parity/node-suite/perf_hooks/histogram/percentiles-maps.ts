import { createHistogram } from "node:perf_hooks";
const h = createHistogram();
h.record(2);
h.record(3);
const numbers = h.percentiles;
const bigints = h.percentilesBigInt;
console.log("maps:", numbers instanceof Map, bigints instanceof Map);
console.log(
  "keys:",
  [...numbers.keys()].join(","),
  [...bigints.keys()].join(","),
);
console.log(
  "value types:",
  [...numbers.values()].every((v) => typeof v === "number"),
  [...bigints.values()].every((v) => typeof v === "bigint"),
);
