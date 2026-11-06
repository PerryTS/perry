import { timerify } from "node:perf_hooks";
const value = { ok: true };
const wrapped = timerify(() => value);
console.log("same object:", wrapped() === value);
