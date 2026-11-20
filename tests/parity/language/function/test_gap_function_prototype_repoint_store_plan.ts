function F(this: any) {}
function setK(o: any, v: number) { o.k = v; }
const oldA: any = new (F as any)();
let seen = -1;
(F as any).prototype = { set k(v: number) { seen = v; } };
const mode = process.argv[2] || "old-first";
if (mode === "old-first") setK(oldA, 1);
const n: any = new (F as any)();
setK(n, 2);
console.log(mode, "seen=" + seen, "own=" + Object.prototype.hasOwnProperty.call(n, "k"));
