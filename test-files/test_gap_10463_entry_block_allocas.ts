// #10463: several lowerings emitted their argument buffer (`alloca [N x double]`)
// or out-parameter (`alloca i64`) into whatever block was current instead of
// the function's entry block. Inside a loop that is a stack bump on every
// iteration, released only when the function returns, so these loops died
// with SIGSEGV once they had consumed the stack (~2^19 iterations for one
// 16-byte buffer at the default 8 MB). Each loop below would take at least
// 19 MB of stack that way on its own; with the buffers in the entry block it
// runs in constant stack.

import { addMinutes } from "./_helpers/add_minutes_10463.ts";

function dateSetters(n: number): number {
  const d = new Date(0);
  let acc = 0;
  for (let i = 0; i < n; i++) {
    d.setTime(i * 1000);
    acc = (acc + d.getUTCSeconds()) % 1_000_003;
    d.setUTCMinutes(i % 60);
    d.setUTCFullYear(2000 + (i % 30), i % 12, 1 + (i % 28));
    d.setUTCHours(i % 24, i % 60, i % 60, i % 1000);
    acc = (acc + d.getUTCHours() + d.getUTCMonth() + d.getUTCMinutes()) % 1_000_003;
  }
  return acc + d.getTime();
}

function dateUtc(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc = (acc + (Date.UTC(2000 + (i % 50), i % 12) % 7919)) % 1_000_003;
    acc = (acc + (Date.UTC(1970, 0, 1 + (i % 28), i % 24) % 7907)) % 1_000_003;
  }
  return acc;
}

function toSpliced(n: number): number {
  const a = [1, 2, 3];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const b = a.toSpliced(1, 1, i);
    const c = b.toSpliced(0, 2, i, i + 1, i + 2);
    acc = (acc + b[1] + c[2] + b.length + c.length) % 1_000_003;
  }
  return acc;
}

function concat(n: number): number {
  const a = [1, 2, 3];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const b = a.concat(i);
    const c = b.concat([i + 1], i + 2);
    acc = (acc + b[3] + c[5] + b.length + c.length) % 1_000_003;
  }
  return acc;
}

function spliceLocal(n: number): number {
  const a = [1, 2, 3];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const removed = a.splice(1, 1, i);
    const none = a.splice(1, 0);
    acc = (acc + removed[0] + none.length + a[1] + a.length) % 1_000_003;
  }
  return acc + a[0] + a[2];
}

class Holder {
  arr: number[] = [1, 2, 3];
}

function unshiftSpliceField(n: number): number {
  const h = new Holder();
  let acc = 0;
  for (let i = 0; i < n; i++) {
    h.arr.unshift(i);
    h.arr.shift();
    const removed = h.arr.splice(0, 1, i);
    acc = (acc + removed[0] + h.arr[0] + h.arr.length) % 1_000_003;
  }
  return acc;
}

function arrayPrototypeCall(n: number): number {
  const a: number[] = [];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    Array.prototype.push.call(a, i, i + 1);
    Array.prototype.unshift.call(a, i + 2);
    const removed: any = Array.prototype.splice.call(a, 0, 3);
    const joined: any = Array.prototype.concat.call(removed, i);
    acc = (acc + removed[1] + joined.length + joined[3] + a.length) % 1_000_003;
  }
  // The same generic lowerings over a plain array-like object.
  const o: any = { length: 0 };
  Array.prototype.push.call(o, 1, 2);
  Array.prototype.unshift.call(o, 0);
  const r: any = Array.prototype.splice.call(o, 1, 1);
  const j: any = Array.prototype.concat.call([7], o);
  return acc + o.length * 10 + r[0] + o[1] + j.length;
}

// The date-fns 4.4.0 `addMinutes` shape, imported: the cross-module inliner
// copies the helper's `setTime` call into this loop.
function addMinutesLoop(n: number): number {
  let d = new Date(Date.UTC(2020, 0, 1));
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const next = addMinutes(d, 1);
    d = next;
    acc = (acc + d.getUTCMinutes()) % 1_000_003;
  }
  return acc + d.getTime();
}

console.log("date setters", dateSetters(250_000));
console.log("Date.UTC", dateUtc(600_000));
console.log("toSpliced", toSpliced(600_000));
console.log("concat", concat(600_000));
console.log("splice (local)", spliceLocal(400_000));
console.log("unshift/splice (field)", unshiftSpliceField(400_000));
console.log("Array.prototype.*.call", arrayPrototypeCall(300_000));
console.log("addMinutes in a loop", addMinutesLoop(1_200_000));
