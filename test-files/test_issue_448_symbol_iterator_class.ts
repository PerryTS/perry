// Issue #448: `*[Symbol.iterator]()` generator method on a class, when
// iterated via `for…of`, used to allocate an empty `{ next, return,
// throw }` iter object whose closure fields all read back as 0 — the
// `next()` call dispatched against an undefined-typed value, never
// produced a `done = true`, and the loop ran forever.
//
// Root cause: `lower_object_literal`'s shape-cache fast path declared
// `js_object_set_field` as taking the value as `DOUBLE`, but the
// runtime function takes `JSValue` (`#[repr(transparent)] u64`). On
// AArch64 / x86_64 SysV / Win64, integer and floating-point arguments
// occupy disjoint register classes, so the closure pointer was placed
// in xmm0 / d0 while the runtime read its value arg from a GP register
// — every store landed with stale GP-register contents (typically 0).
// Fix: bitcast the double to i64 at the call site + flip the declared
// arg type to I64 (matching the runtime ABI).

// 1) The Fibonacci shape from the issue body.
class Fibonacci {
  limit: number;
  constructor(limit: number) {
    this.limit = limit;
  }

  *[Symbol.iterator](): Generator<number> {
    let a = 0;
    let b = 1;
    let count = 0;
    while (count < this.limit) {
      yield a;
      [a, b] = [b, a + b];
      count++;
    }
  }
}

const fibs: number[] = [];
for (const f of new Fibonacci(7)) {
  fibs.push(f);
}
console.log(fibs.join(","));

// 2) Range-over-class — exercises a different `*[Symbol.iterator]()`
// shape (for-loop body instead of while-loop).
class Range {
  start: number;
  end: number;
  constructor(start: number, end: number) {
    this.start = start;
    this.end = end;
  }

  *[Symbol.iterator](): Generator<number> {
    for (let i = this.start; i < this.end; i++) {
      yield i;
    }
  }
}

const rangeResult: number[] = [];
for (const n of new Range(3, 7)) {
  rangeResult.push(n);
}
console.log(rangeResult.join(","));

// 3) Plain top-level generator function via `for…of`. The same
// `Object([("next", Closure), ...])` HIR shape, allocated outside any
// class — the bug applied here too (the original issue claimed this
// worked, but it didn't on the maintainer's host either).
function* gen(): Generator<number> {
  yield 10;
  yield 20;
  yield 30;
}

const collected: number[] = [];
for (const v of gen()) {
  collected.push(v);
}
console.log(collected.join(","));

// 4) Direct `.next()` calls — exercises the iter-result path without
// going through the `for…of` lowering rewrite. Pre-fix every `.next()`
// returned an object whose `value` and `done` both read as `undefined`.
function* basicGen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}

const g = basicGen();
const r1 = g.next();
console.log(r1.value, r1.done);
const r2 = g.next();
console.log(r2.value, r2.done);
const r3 = g.next();
console.log(r3.value, r3.done);
const r4 = g.next();
console.log(r4.value, r4.done);

// 5) Empty-bound iteration on a class iterator — the loop should
// never enter the body and `done` must terminate cleanly. Pre-fix
// the loop hung indefinitely even though the generator body would
// have returned `done: true` immediately.
class EmptyRange {
  *[Symbol.iterator](): Generator<number> {
    // No yields.
  }
}

let entered = 0;
for (const _ of new EmptyRange()) {
  entered++;
}
console.log("entered=" + entered);

// 6) `typeof` of a generator's `.next` field — exercises that the
// closure pointer survives the object literal store. Pre-fix this
// returned "number" because the field stored `0` (an unboxed double).
function* tinyGen(): Generator<number> {
  yield 1;
}
const tg = tinyGen();
console.log("typeof next=" + typeof tg.next);
