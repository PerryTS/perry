// callee passes its parameter on to a callee that keeps it. Sabotage fate:outer#0 -> holder global_root.
const kept: { v: number }[] = [];
function inner(o: { v: number }): number { kept.push(o); return o.v; }
function outer(o: { v: number }): number { return inner(o) + 1; }
let s = 0;
for (let i = 0; i < 3; i++) { s += outer({ v: i }); }
console.log(s, kept.length);
