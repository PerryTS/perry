// All argument expressions first (left to right), then ToNumber on the used ones in order.
const k = Number(process.argv[2] ?? "0");
const log: string[] = [];
const v = (name: string, x: number): any => ({ valueOf() { log.push("v" + name); return x; } });
const e = (name: string, x: number): any => { log.push("e" + name); return x; };
function flush(r: unknown): void { console.log(r, log.join(",")); log.length = 0; }
flush(Math.cos(v("a", k), e("b", 2)));
flush(Math.sin(v("a", k), e("b", 2), e("c", 3)));
flush(Math.pow(v("a", k + 2), v("b", 3), e("c", 4)));
flush(Math.atan2(v("a", k + 1), v("b", 2), v("c", 3)));
flush(Math.max(v("a", NaN), v("b", k), e("c", 3)));
flush(Math.min(v("a", NaN), v("b", k), v("c", 3)));
flush(Math.hypot(v("a", NaN), v("b", k), v("c", 3)));
flush(Math.imul(v("a", k + 2), v("b", 3), v("c", 4)));
flush(Math.atan2(v("a", NaN), v("b", k)));
flush(Math.pow(v("a", NaN), v("b", k)));
// Used arguments with a coercing first operand and an effectful later expression.
flush(Math.atan2(v("a", k + 1), e("b", 2)));
flush(Math.pow(v("a", k + 2), e("b", 3)));
flush(Math.imul(v("a", k + 2), e("b", 3)));
flush(Math.hypot(v("a", k + 3), e("b", 4)));
flush(Math.hypot(v("a", k + 3), v("b", 4), e("c", 5)));
flush(Math.max(v("a", k + 3), e("b", 4)));
flush(Math.min(v("a", k + 3), e("b", 4)));
flush(Math.max(v("a", k + 3), v("b", 4), e("c", 5)));
