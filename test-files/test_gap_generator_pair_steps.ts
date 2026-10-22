// Generator steps consumed directly (for-of, spread, destructuring, yield*)
// carry (value, done) without an IteratorResult object. Every observable
// behavior must match a materialized result: abrupt completion, finally,
// yield* to user iterators (return/throw forwarding, result identity),
// exceptions crossing yields, and result objects user code does see.
const log: string[] = [];
const out = (s: string) => log.push(s);

// 1. return / throw mid-iteration through for-of
function* counted(tag: string) {
  try {
    for (let i = 0; i < 5; i++) yield tag + i;
    return tag + "-ret";
  } finally {
    out("finally " + tag);
  }
}
for (const v of counted("a")) {
  out(v);
  if (v === "a1") break;
}
try {
  for (const v of counted("b")) {
    out(v);
    if (v === "b2") throw new Error("loop body " + v);
  }
} catch (e) {
  out("caught " + (e as Error).message);
}
out("spread " + JSON.stringify([...counted("c")]));
const [d0, , d2] = counted("d");
out("destructure " + d0 + " " + d2);

// 2. finally that yields while the consumer closes it
function* yieldingFinally() {
  try {
    yield 1;
    yield 2;
  } finally {
    yield "cleanup";
    out("after cleanup yield");
  }
}
for (const v of yieldingFinally()) {
  out("yf " + v);
  if (v === 1) break;
}
const yf = yieldingFinally();
out(JSON.stringify(yf.next()));
out(JSON.stringify(yf.return(7)));
out(JSON.stringify(yf.next()));
out(JSON.stringify(yf.next()));

// 3. yield* to a user iterator with return(); results are passed through
class Counter {
  n = 0;
  results: object[] = [];
  [Symbol.iterator]() {
    return this;
  }
  next(v?: unknown) {
    const r: any = { value: this.n * 10, done: this.n >= 3, extra: "x" + this.n, sent: v };
    this.n++;
    this.results.push(r);
    return r;
  }
  return(v?: unknown) {
    out("user return " + v);
    return { value: "user-ret", done: true };
  }
}
function* delegatesToUser(c: Counter) {
  const r = yield* c;
  out("delegate result " + r);
  return "outer-done";
}
const uc = new Counter();
const du = delegatesToUser(uc);
const r0 = du.next("ignored");
out("passthrough identity " + (r0 === uc.results[0]) + " extra " + (r0 as any).extra);
const r1 = du.next("s1");
out("sent " + (r1 as any).sent + " identity " + (r1 === uc.results[1]));
out(JSON.stringify(du.return("R")));
out(JSON.stringify(du.next()));
for (const v of delegatesToUser(new Counter())) out("for-of user " + v);
const closer = new Counter();
for (const v of delegatesToUser(closer)) {
  out("break at " + v);
  break;
}

// 4. yield* to generators: values, sent values, return values, nesting
function* leaf(base: number) {
  const a = yield base;
  const b = yield base + 1;
  return "leaf " + a + " " + b;
}
function* middle() {
  const x = yield* leaf(10);
  out("middle got " + x);
  const y = yield* leaf(20);
  return x + " | " + y;
}
function* top() {
  const z = yield* middle();
  out("top got " + z);
  yield "last";
}
out("nested spread " + JSON.stringify([...top()]));
const t = top();
const steps: string[] = [];
let s = t.next();
let k = 0;
while (!s.done) {
  steps.push(JSON.stringify(s));
  s = t.next("v" + k++);
}
out("manual " + steps.join(","));
out("final " + JSON.stringify(s));

// 5. exceptions crossing yields and yield*
function* innerThrows() {
  yield "i1";
  throw new Error("inner boom");
}
function* outerCatches() {
  try {
    yield* innerThrows();
  } catch (e) {
    out("outer caught " + (e as Error).message);
    yield "recovered";
  }
}
out("cross " + JSON.stringify([...outerCatches()]));
function* innerCatches() {
  try {
    yield "w1";
  } catch (e) {
    out("inner caught " + e);
    yield "w2";
  }
  return "inner-end";
}
function* outerForwards() {
  const r = yield* innerCatches();
  yield "outer " + r;
}
const of = outerForwards();
out(JSON.stringify(of.next()));
out(JSON.stringify(of.throw("T")));
out(JSON.stringify(of.next()));
out(JSON.stringify(of.next()));
try {
  for (const v of innerThrows()) out("before throw " + v);
} catch (e) {
  out("for-of caught " + (e as Error).message);
}

// 6. result objects user code observes
function* two() {
  yield { tag: "obj" };
  return 5;
}
const g = two();
const ra = g.next();
const rb = g.next();
const rc = g.next();
out("keys " + Object.keys(ra).join(",") + " proto " + (Object.getPrototypeOf(ra) === Object.prototype));
out("own " + ra.hasOwnProperty("value") + " " + ra.hasOwnProperty("done"));
const desc = Object.getOwnPropertyDescriptor(ra, "value")!;
out("desc " + desc.writable + " " + desc.enumerable + " " + desc.configurable);
(ra as any).value = "mutated";
out("fresh " + (rb !== rc) + " " + JSON.stringify(rb) + " " + JSON.stringify(rc));
function* sameValue() {
  const o = { n: 1 };
  yield o;
  yield o;
}
const [o1, o2] = sameValue();
out("value identity " + (o1 === o2));

// 7. string accumulators handed out by a step are not mutated later
function* accumulate() {
  let s = "";
  for (let i = 0; i < 4; i++) {
    s += String.fromCharCode(97 + i);
    yield s;
  }
}
const parts: string[] = [];
for (const p of accumulate()) parts.push(p);
out("acc " + parts.join(","));
out("acc spread " + [...accumulate()].join(","));

// 8. protocol errors still surface
const bad = {
  [Symbol.iterator]() {
    return { next: () => 42 as any };
  },
};
function* delegatesBad() {
  yield* bad as any;
}
try {
  [...delegatesBad()];
} catch (e) {
  out("non-object " + (e instanceof TypeError));
}
let reentrant: Generator<number> | undefined;
function* selfStep() {
  try {
    reentrant!.next();
  } catch (e) {
    out("reentrant " + (e instanceof TypeError));
  }
  yield 1;
}
reentrant = selfStep();
out("reentrant spread " + JSON.stringify([...reentrant]));
const done = counted("z");
for (const _ of done) {
}
out("after done " + JSON.stringify(done.next()) + " " + JSON.stringify([...done]));
const doneGetter = {
  [Symbol.iterator]() {
    return {
      next: () => ({
        done: true,
        get value() {
          out("value getter");
          return "gv";
        },
      }),
    };
  },
};
function* readsOnce() {
  const v = yield* doneGetter as any;
  out("getter result " + v);
}
[...readsOnce()];

for (const line of log) console.log(line);
