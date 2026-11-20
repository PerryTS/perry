// callee stores parameter 1 into parameter 0: Kept. Sabotage fate:put#1 -> holder an object (worklist drain).
function put(box: { item: { v: number } | null }, o: { v: number }): number { box.item = o; return o.v; }
const box: { item: { v: number } | null } = { item: null };
let s = 0;
for (let i = 0; i < 3; i++) { s += put(box, { v: i + 10 }); }
console.log(s, box.item!.v);
