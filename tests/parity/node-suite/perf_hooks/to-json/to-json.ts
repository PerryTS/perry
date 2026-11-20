import { performance } from "node:perf_hooks";
// performance.toJSON() returns a snapshot object including timeOrigin.
const j = performance.toJSON();
console.log("is object:", typeof j === "object" && j !== null);
console.log("timeOrigin number:", typeof j.timeOrigin === "number");
