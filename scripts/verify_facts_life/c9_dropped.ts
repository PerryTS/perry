// callee only reads its parameter: Proven Dropped (a real claim, no sabotage).
function len(p: { x: number; y: number }): number { return p.x + p.y; }
let s = 0;
for (let i = 0; i < 100000; i++) { s += len({ x: i, y: 1 }); }
console.log(s);
