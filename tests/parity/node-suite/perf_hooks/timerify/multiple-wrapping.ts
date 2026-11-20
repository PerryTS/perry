import { timerify } from "node:perf_hooks";
function work(a: unknown) {
  return a;
}
const first = timerify(work);
const second = timerify(work);
const nested = timerify(first);
console.log("distinct:", first !== second, first !== nested);
console.log("names:", first.name, nested.name);
console.log("lengths:", first.length, nested.length);
