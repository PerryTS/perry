// Generator protocol paths: for-of / spread / destructuring / yield* over
// sync generators and custom iterators, abrupt completion, and reuse.
const log: string[] = [];

function* three(n: number) {
  try {
    yield n;
    yield n + 1;
    yield n + 2;
  } finally {
    log.push("finally " + n);
  }
}

// for-of with break runs finally (IteratorClose -> return)
for (const v of three(10)) {
  log.push("v " + v);
  if (v === 11) break;
}
// spread and destructuring
log.push("spread " + JSON.stringify([...three(20)]));
const [a, b] = three(30);
log.push("destructure " + a + " " + b);

// return / throw mid-iteration
const g1 = three(40);
log.push(JSON.stringify(g1.next()));
log.push(JSON.stringify(g1.return(99)));
log.push(JSON.stringify(g1.next()));
const g2 = three(50);
g2.next();
try {
  g2.throw(new Error("boom"));
} catch (e) {
  log.push("caught " + (e as Error).message);
}
log.push(JSON.stringify(g2.next()));

// exceptions crossing yields, caught inside the generator
function* catcher() {
  while (true) {
    try {
      const x = yield "ready";
      log.push("got " + x);
    } catch (e) {
      log.push("inner caught " + e);
      return "done-after-catch";
    }
  }
}
const c = catcher();
c.next();
c.next(1);
log.push(JSON.stringify(c.throw("E")));
log.push(JSON.stringify(c.next()));

// generator body throwing propagates out of for-of and completes the generator
function* thrower() {
  yield 1;
  throw new Error("from body");
}
const t = thrower();
try {
  for (const v of t) log.push("t " + v);
} catch (e) {
  log.push("for-of caught " + (e as Error).message);
}
log.push(JSON.stringify(t.next()));

// reuse after completion
const r = three(60);
for (const _ of r) {}
log.push("reuse " + JSON.stringify([...r]) + " " + JSON.stringify(r.next()));

// yield* to a custom iterator with return; inner results pass through as-is
const marker = { value: 7, done: false, extra: "kept" };
let innerReturnCalls = 0;
const custom = {
  i: 0,
  [Symbol.iterator]() {
    return this;
  },
  next(v: unknown) {
    log.push("custom next arg " + String(v) + " argc " + arguments.length);
    this.i++;
    if (this.i === 1) return marker;
    if (this.i === 2) return { value: 8, done: false };
    return { value: "inner-ret", done: true };
  },
  return(v: unknown) {
    innerReturnCalls++;
    return { value: v, done: true };
  },
};
function* outer() {
  const r = yield* custom;
  log.push("yield* result " + r);
  return "outer-end";
}
const o = outer();
const first = o.next("ignored");
log.push("passthrough identity " + (first === (marker as unknown)) + " " + (first as any).extra);
log.push(JSON.stringify(o.next("s2")));
log.push(JSON.stringify(o.next("s3")));
custom.i = 0;
const o2 = outer();
o2.next();
log.push(JSON.stringify(o2.return("early")) + " returns " + innerReturnCalls);

// yield* to an iterator whose next returns a primitive -> TypeError
const bad = {
  [Symbol.iterator]() {
    return { next: () => 5 as unknown as IteratorResult<number> };
  },
};
function* useBad() {
  yield* bad as Iterable<number>;
}
try {
  useBad().next();
  log.push("no error");
} catch (e) {
  log.push("bad inner " + (e instanceof TypeError));
}

// yield* to generators (Effect.gen shape), class iterables
class Eff {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  *[Symbol.iterator](): Generator<Eff, number, number> {
    return yield this;
  }
}
function* prog() {
  const x = yield* new Eff(1);
  const y = yield* new Eff(x + 1);
  return x + y;
}
const p = prog();
let st = p.next(0 as any);
while (!st.done) st = p.next((st.value as Eff).v * 10);
log.push("eff " + st.value);

// a replaced next on the function's prototype is honoured
function* plain() {
  yield 1;
  yield 2;
}
(plain.prototype as any).next = function () {
  const done = !!this.done_;
  this.done_ = true;
  return { value: "patched", done };
};
const seen: unknown[] = [];
for (const v of plain()) seen.push(v);
log.push("patched proto " + JSON.stringify(seen));

// own next on the instance is honoured by for-of (read once at GetIterator)
function* own() {
  yield "real";
}
const ownIt = own();
let ownCalls = 0;
(ownIt as any).next = () => (ownCalls++ === 0 ? { value: "own", done: false } : { value: undefined, done: true });
log.push("own next " + JSON.stringify([...ownIt]));

// receiver checks and re-entrancy
const genNext = Object.getPrototypeOf(Object.getPrototypeOf(three(0))).next;
try {
  genNext.call({});
} catch (e) {
  log.push("bad receiver " + (e instanceof TypeError));
}
let selfRef: Generator<number> | null = null;
function* reenter() {
  try {
    selfRef!.next();
  } catch (e) {
    log.push("reentrant " + (e instanceof TypeError));
  }
  yield 1;
}
selfRef = reenter();
selfRef.next();

// URLSearchParams subclass iteration still reaches its backing
class Params extends URLSearchParams {}
const pairs: string[] = [];
for (const [k, v] of new Params("a=1&b=2")) pairs.push(k + "=" + v);
log.push("params " + pairs.join(","));

// yield* must not consult Function.prototype.call
const realCall = Function.prototype.call;
let callUsed = 0;
(Function.prototype as any).call = function (this: Function, ...args: unknown[]) {
  callUsed++;
  return realCall.apply(this, args as any);
};
function* viaCall() {
  yield* custom;
}
custom.i = 0;
log.push("call untouched " + JSON.stringify([...viaCall()].length) + " " + callUsed);
(Function.prototype as any).call = realCall;

for (const line of log) console.log(line);
