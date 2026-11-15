// bet sabotage fatebet:keep#0: the callee keeps it; the check reports PERRY_BET_FAILED and continues.
let G: { v: number } | null = null;
function keep(o: { v: number }): number { G = o; return o.v; }
let s = 0;
for (let i = 0; i < 3; i++) { s += keep({ v: i }); }
console.log(s, G!.v);
