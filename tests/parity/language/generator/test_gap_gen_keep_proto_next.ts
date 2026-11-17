// A generator function whose own prototype object gets a next.
function* g(): Generator<number> { yield 1; yield 2; yield 3; }
let calls = 0;
(g.prototype as any).next = function () { calls++; return calls > 2 ? { value: undefined, done: true } : { value: 100 + calls, done: false }; };
const vs: number[] = [];
for (const v of g()) vs.push(v);
console.log("proto-next", vs.join(","), calls);
