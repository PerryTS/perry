// A store into the global object under an unknown key (main: guarded namespace calls).
const k = process.argv[2] ?? "someShim";
(globalThis as any)[k] = 1;
let s = 0;
for (let i = 0; i < 10; i++) s += Math.sin(i / 10);
console.log(s.toFixed(9), (globalThis as any)[k]);
