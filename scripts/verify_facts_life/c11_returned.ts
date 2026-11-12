// callee returns its parameter: Returned. Sabotage fate:id#0 -> the caller's local holds it (a stack root).
function id(o: { v: number }, n: number): { v: number } { if (n > 0) return id(o, n - 1); return o; }
let s = 0;
for (let i = 0; i < 3; i++) { const k = id({ v: i }, 2); s += k.v; }
console.log(s);
