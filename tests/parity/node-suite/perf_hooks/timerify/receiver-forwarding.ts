import { timerify } from "node:perf_hooks";

function receiver(this: unknown) {
  return this;
}
const wrapped = timerify(receiver);
const object = { value: 42 };
console.log("object:", wrapped.call(object) === object);
console.log("apply:", wrapped.apply(object, []) === object);
console.log("null:", wrapped.call(null) === null);
console.log("primitive:", wrapped.call(7) === 7);
console.log("plain:", wrapped() === undefined);
