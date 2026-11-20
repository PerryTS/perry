// yield* inside a generator looped by for-of: over a generator (its return
// value is the yield* value), over an array, and over a user iterator.
function* inner(): Generator<number, string> { yield 1; yield 2; return "r"; }
function* overGen(): Generator<number | string> { const r = yield* inner(); yield r; }
console.log([...overGen()].join(","));
const a: (number | string)[] = [];
for (const v of overGen()) a.push(v);
console.log("gen", a.join(","));
function* overArr(): Generator<number> { yield 0; yield* [1, 2, 3]; yield 4; }
const b: number[] = [];
for (const v of overArr()) b.push(v);
console.log("arr", b.join(","));
const log: string[] = [];
const user = {
  i: 0,
  [Symbol.iterator]() { return this; },
  next() { this.i++; return { get done() { log.push("done?" + user.i); return user.i > 2; }, get value() { log.push("value" + user.i); return user.i * 100; } }; },
  return() { log.push("return"); return { done: true, value: undefined }; },
};
function* overUser(): Generator<number> { yield* user as any; yield 7; }
for (const v of overUser()) log.push("got" + v);
console.log("user", log.join(","));
user.i = 0; log.length = 0;
for (const v of overUser()) { log.push("got" + v); break; }
console.log("user-break", log.join(","));
