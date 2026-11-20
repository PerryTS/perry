import { timerify } from "node:perf_hooks";
const receiver = { base: 10 };
function sum(this: typeof receiver, a: number, b: number) {
  return { total: this.base + a + b, args: arguments.length };
}
const wrapped = timerify(sum);
const result = wrapped.call(receiver, 2, 3);
console.log("result:", result.total, result.args);
console.log("shape:", wrapped.name, wrapped.length);
