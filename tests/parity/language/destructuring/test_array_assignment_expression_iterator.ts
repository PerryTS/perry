// An array destructuring assignment in expression position runs the iterator
// protocol and evaluates to its right-hand side, like the statement form.
const log: string[] = [];
function it(n: number) {
  let i = 0;
  return {
    [Symbol.iterator]() { log.push("iter"); return this; },
    next() { log.push("next"); return i < n ? { value: i++, done: false } : { value: undefined, done: true }; },
    return() { log.push("return"); return { done: true }; },
  };
}
let b: any;
let c: any;
const src = it(3);
const r: any = ([b] = src as any);
console.log(log.join(","), b, r === src);

log.length = 0;
const r2: any = ([b, c = 7, ...rest] = it(1) as any) as any;
console.log(log.join(","), b, c, JSON.stringify(rest), typeof r2);
var rest: any;

let calls = 0;
function once() { calls++; return [10, 20]; }
let x: any, y: any;
console.log(([x, y] = once())[1], x, y, calls);

function* gen() { yield 1; yield 2; }
for (const v of [([x] = gen())]) console.log(x, typeof v.next);
