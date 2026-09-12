// #10086: array destructuring no longer drives the iterator protocol for a
// spread-free array literal or a statically-proven array — it reads the
// elements directly, behind a runtime guard on
// `Array.prototype[Symbol.iterator]`. Every observable behaviour of the old
// lowering has to survive that: evaluation order and count, holes, defaults,
// rest, nested patterns, member-expression targets, a real iterator for
// anything that is not a plain array, and the patched-prototype case itself.

const out: string[] = [];
function log(label: string, value: unknown): void {
  out.push(label + "=" + String(value));
}

// --- once-evaluation and source order -------------------------------------
let calls = "";
function f(): number {
  calls += "f";
  return 1;
}
function g(): number {
  calls += "g";
  return 2;
}
let a = 0;
let b = 0;
[a, b] = [f(), g()];
log("order.calls", calls);
log("order.a", a);
log("order.b", b);

// --- swap: aliased and identical targets ----------------------------------
let x = 7;
let y = 19;
[x, y] = [y, x];
log("swap.x", x);
log("swap.y", y);
let same = 0;
[same, same] = [1, 2];
log("swap.same", same);

// A swap in a loop must stay a swap (the literal is gone, the values are not).
let p = 1;
let q = 2;
let acc = 0;
for (let i = 0; i < 5; i++) {
  [p, q] = [q, p];
  acc = acc * 10 + p;
}
log("swap.loop", acc);
log("swap.loop.p", p);
log("swap.loop.q", q);

// --- holes ----------------------------------------------------------------
const src3 = [10, 20, 30];
let h1 = 0;
let h2 = 0;
[, h1, h2] = src3;
log("hole.h1", h1);
log("hole.h2", h2);
const [, hole2] = src3;
log("hole.decl", hole2);

// --- defaults -------------------------------------------------------------
const short: number[] = [5];
const [d1 = 1, d2 = 2] = short;
log("default.d1", d1);
log("default.d2", d2);
let e1 = 0;
let e2 = 0;
[e1 = 1, e2 = 2] = short;
log("default.e1", e1);
log("default.e2", e2);
// A genuine NaN element is NOT undefined, so the default must not fire.
const [nanKept = 99] = [NaN];
log("default.nan", nanKept);
// An explicit `undefined` element does fire it.
const [undefFires = 42] = [undefined];
log("default.undefined", undefFires);
// The spec re-reads the source length on every element, so a default that
// truncates the array must be visible to the next element.
const shrink: any[] = [undefined, 2];
const [s1 = (((shrink as any).length = 1), 5), s2 = 7] = shrink;
log("default.shrink.s1", s1);
log("default.shrink.s2", s2);

// --- rest -----------------------------------------------------------------
const [r1, ...rest] = [1, 2, 3, 4];
log("rest.r1", r1);
log("rest.rest", rest.join(","));
let ra = 0;
let restTail: number[] = [];
[ra, ...restTail] = [9, 8, 7];
log("rest.ra", ra);
log("rest.tail", restTail.join(","));

// --- nested patterns ------------------------------------------------------
const [[n1], { n2 }] = [[3], { n2: 4 }] as [number[], { n2: number }];
log("nested.n1", n1);
log("nested.n2", n2);
let m1 = 0;
let m2 = 0;
[[m1], { n2: m2 }] = [[5], { n2: 6 }] as [number[], { n2: number }];
log("nested.m1", m1);
log("nested.m2", m2);

// --- member-expression targets --------------------------------------------
const obj: any = { x: 0, y: 0 };
[obj.x, obj.y] = [11, 12];
log("member.x", obj.x);
log("member.y", obj.y);
const keyOrder: string[] = [];
const box: any = {};
function keyA(): string {
  keyOrder.push("a");
  return "ka";
}
function keyB(): string {
  keyOrder.push("b");
  return "kb";
}
[box[keyA()], box[keyB()]] = [21, 22];
log("member.computed", box.ka + ":" + box.kb + ":" + keyOrder.join(""));

// --- the source array still exists when it escapes -------------------------
function pair(v: number): number[] {
  return [v, v + 1];
}
const escaped = pair(1);
const [ea, eb] = escaped;
escaped.push(99);
log("escape.ea", ea);
log("escape.eb", eb);
log("escape.array", escaped.join(","));
let captured: number[] = [];
function capture(): number[] {
  const made = [30, 31];
  captured = made;
  return made;
}
const [ca, cb] = capture();
captured.push(32);
log("capture.ca", ca);
log("capture.cb", cb);
log("capture.array", captured.join(","));
log("capture.identity", String(captured.length === 3));

// --- a shorter array than the pattern -------------------------------------
const [t1, t2, t3] = [1];
log("short.t1", t1);
log("short.t2", String(t2));
log("short.t3", String(t3));

// --- the iterator protocol is intact for everything that is not an array ---
let nextCalls = 0;
const custom: any = {};
custom[Symbol.iterator] = function () {
  return {
    next: function () {
      nextCalls++;
      return { done: nextCalls > 3, value: nextCalls * 100 };
    },
  };
};
const [c1, c2] = custom;
log("custom.c1", c1);
log("custom.c2", c2);
log("custom.next", nextCalls);

function* gen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}
const [gA, gB] = gen();
log("gen.a", gA);
log("gen.b", gB);

const set = new Set<number>([4, 5, 6]);
const [sA, sB] = set;
log("set.a", sA);
log("set.b", sB);

const map = new Map<string, number>([["k", 1]]);
const [[mk, mv]] = map;
log("map.k", mk);
log("map.v", mv);

const [strA, strB] = "hi";
log("string.a", strA);
log("string.b", strB);

// An iterator that closes: `return()` must run when the pattern stops early.
let closed = 0;
const closing: any = {};
closing[Symbol.iterator] = function () {
  let i = 0;
  return {
    next: function () {
      i++;
      return { done: false, value: i };
    },
    return: function () {
      closed++;
      return { done: true };
    },
  };
};
const [z1] = closing;
log("close.z1", z1);
log("close.count", closed);

// --- inside an async function and a generator ------------------------------
// The async-to-generator transform boxes body locals into shared cells, so the
// guarded per-element shape has to survive being split into generator states.
async function asyncCase(): Promise<string> {
  const src: number[] = [41, 42];
  const [a, b] = src;
  await Promise.resolve(0);
  let c = 0;
  let d = 0;
  [c, d] = [b, a];
  const [e, f2 = 7] = [c];
  return a + ":" + b + ":" + c + ":" + d + ":" + e + ":" + f2;
}

function* genCase(): Generator<string> {
  const src: number[] = [51, 52];
  const [a, b] = src;
  yield a + ":" + b;
  let c = 0;
  let d = 0;
  [c, d] = [b, a];
  yield c + ":" + d;
}

// A destructuring inside a loop inside a generator: the guard must not break
// the loop's state split.
function* genLoop(): Generator<number> {
  let p = 1;
  let q = 2;
  for (let i = 0; i < 3; i++) {
    [p, q] = [q, p];
    yield p;
  }
}

const genOut: string[] = [];
for (const v of genCase()) genOut.push(v);
log("gen.case", genOut.join("|"));
const genLoopOut: number[] = [];
for (const v of genLoop()) genLoopOut.push(v);
log("gen.loop", genLoopOut.join(","));

// --- a patched %ArrayIteratorPrototype%.next is still honoured -------------
// The non-iterator arm never calls `.next()`, so it cannot observe a patched
// one. The runtime publishes "array iteration is not provably pristine" the
// moment this prototype object escapes through `Object.getPrototypeOf`, which
// is what makes both arms below decline it. Sticky in Perry, so this runs after
// everything that must exercise the fast arm.
function patchedNextChecks(): void {
  const arrayIterProto: any = Object.getPrototypeOf([][Symbol.iterator]());
  const originalNext = arrayIterProto.next;
  arrayIterProto.next = function (this: any) {
    const r = originalNext.call(this);
    if (!r.done) r.value = (r.value as number) * 2;
    return r;
  };
  const src: number[] = [1, 2];
  const [na, nb] = src;
  log("patchedNext.proven", na + ":" + nb);
  let nc = 0;
  let nd = 0;
  [nc, nd] = [3, 4];
  log("patchedNext.literal", nc + ":" + nd);
  const forOf: number[] = [];
  for (const v of src) forOf.push(v);
  log("patchedNext.forof", forOf.join(","));
  arrayIterProto.next = originalNext;
  const [ra, rb] = src;
  log("patchedNext.restored", ra + ":" + rb);
}

// --- a patched Array.prototype[Symbol.iterator] is still honoured ----------
// Sticky in Perry: everything after this point takes the protocol arm, so it
// runs last — after the async case has resolved, so that case still exercises
// the fast arm.
function patchedPrototypeChecks(): void {
  const original = (Array.prototype as any)[Symbol.iterator];
  let patchedCalls = 0;
  (Array.prototype as any)[Symbol.iterator] = function () {
    patchedCalls++;
    let i = 0;
    return {
      next: () => {
        i++;
        return { done: i > 2, value: i * 1000 };
      },
    };
  };
  let pa = 0;
  let pb = 0;
  [pa, pb] = [1, 2];
  log("patched.literal", pa + ":" + pb);
  const plain = [7, 8];
  const [pc, pd] = plain;
  log("patched.proven", pc + ":" + pd);
  log("patched.calls", patchedCalls);
  (Array.prototype as any)[Symbol.iterator] = original;
  const [qa, qb] = [1, 2];
  log("restored", qa + ":" + qb);
}

asyncCase().then((v) => {
  log("async.case", v);
  patchedNextChecks();
  patchedPrototypeChecks();
  console.log(out.join("\n"));
});
