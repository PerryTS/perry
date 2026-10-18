import { GCProfiler } from "node:v8";

const key = Symbol.dispose;
const prototype = GCProfiler.prototype;
const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
const dispose = prototype[key];
const first = new GCProfiler();
const second = new GCProfiler();
console.log(
  "inherited shared:",
  !Object.hasOwn(first, key),
  first[key] === dispose,
  second[key] === dispose,
);
console.log(
  "descriptor:",
  descriptor?.writable,
  descriptor?.enumerable,
  descriptor?.configurable,
);
console.log("function:", typeof dispose, dispose?.name, dispose?.length);

first.start();
second.start();
console.log("other receiver:", dispose.call(second), second.stop(), typeof first.stop());
console.log("repeated:", dispose.call(second), dispose.call(second));

let reads = 0;
let calls = 0;
const receiver = {
  get stop() {
    reads++;
    return function () {
      calls++;
      console.log("custom this:", this === receiver);
      return "ignored";
    };
  },
};
console.log("custom result:", dispose.call(receiver), reads, calls);

let overrides = 0;
(first as any).stop = () => { overrides++; return 42; };
console.log("override:", dispose.call(first), overrides);

const marker = new Error("marker");
for (const value of [
  { get stop() { throw marker; } },
  { stop() { throw marker; } },
]) {
  try {
    dispose.call(value);
    console.log("throw: no throw");
  } catch (error) {
    console.log("throw identity:", error === marker);
  }
}

for (const value of [undefined, null, {}]) {
  try {
    dispose.call(value);
    console.log("invalid receiver: no throw");
  } catch (error: any) {
    console.log("invalid receiver:", error.name, error.code ?? "no-code");
  }
}
