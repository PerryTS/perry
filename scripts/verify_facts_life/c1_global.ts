// callee stores its parameter into a module variable: Kept. Sabotage fate:keep#0 -> holder global_root.
let G: { v: number } | null = null;
function keep(o: { v: number }): number { G = o; return o.v; }
let s = 0;
for (let i = 0; i < 3; i++) { s += keep({ v: i }); }
console.log(s, G!.v);
