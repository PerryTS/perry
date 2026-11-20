import { timerify } from "node:perf_hooks";
function sample(a: unknown, b: unknown) {
  return [a, b];
}
const wrapped = timerify(sample);
for (const key of ["name", "length"] as const) {
  const d = Object.getOwnPropertyDescriptor(wrapped, key)!;
  console.log(
    key,
    (wrapped as any)[key],
    d.writable,
    d.enumerable,
    d.configurable,
  );
}
