import { performance } from "node:perf_hooks";

for (const value of [undefined, null, 0, true, {}, [], Symbol("s")]) {
  try {
    performance.measure(value as any);
    console.log("invalid name: ok");
  } catch (e) {
    console.log(`invalid name: ${(e as Error).name}:${(e as any).code || "no-code"}`);
  }
}

console.log("measure string:", performance.measure("ok").name);
console.log("mark coerces number:", performance.mark(1 as any).name);
