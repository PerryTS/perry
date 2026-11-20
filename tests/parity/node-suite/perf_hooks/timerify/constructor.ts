import { timerify } from "node:perf_hooks";
class Value {
  marker = 1;
}
const Wrapped = timerify(Value);
const instance = new Wrapped();
console.log("instance:", instance instanceof Value, instance.marker);
try {
  (Wrapped as any)();
  console.log("call ok");
} catch (error) {
  console.log("call:", (error as Error).name);
}
