// Companion of reassigned-global.ts: a program that never writes a timer
// binding keeps the direct timer calls and the same observable behavior.
const g = globalThis as any;
// Timer and immediate order races in Node: print what fired, sorted.
const fired: string[] = [];
const st = g.setTimeout;
console.log(typeof setTimeout, typeof queueMicrotask, setTimeout === st);
setTimeout(() => fired.push("timeout"), 1);
const iv = setInterval(() => {
  fired.push("interval");
  clearInterval(iv);
}, 1);
const never = setTimeout(() => fired.push("never"), 50);
clearTimeout(never);
setImmediate(() => fired.push("immediate"));
queueMicrotask(() => fired.push("microtask"));
setTimeout(() => console.log(fired.sort().join(", ")), 20);
