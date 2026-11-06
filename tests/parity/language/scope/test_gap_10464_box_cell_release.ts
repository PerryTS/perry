// #10464: a `let`/`var` captured by a closure and reassigned lives in a box
// cell. Outside a lowered async state machine those cells were never released,
// so each call leaked a registered GC root plus everything the binding last
// referenced. This test checks both halves of the fix: memory stays bounded
// relative to an equal-allocation control, and cells that escape through
// closures (returned, stored, nested, generators, async) keep their values
// across many collections.

const maybeGc = (globalThis as any).gc as (() => void) | undefined;

function churn(rounds: number): number {
  let total = 0;
  for (let r = 0; r < rounds; r++) {
    const junk: Array<{ i: number; s: string; a: number[] }> = [];
    for (let i = 0; i < 2000; i++) junk.push({ i, s: "x" + i, a: [i, i + 1] });
    total += junk.length;
  }
  if (typeof maybeGc === "function") maybeGc();
  return total;
}

// ---- memory: the issue's repro, compared in-process against its control ----

function payload(i: number): number {
  let buf: number[] = new Array(512).fill(i);
  const swap = () => {
    buf = new Array(512).fill(i + 1);
  };
  swap();
  return buf.length;
}

function noBox(i: number): number {
  const holder = { buf: new Array(512).fill(i) };
  const swap = () => {
    holder.buf = new Array(512).fill(i + 1);
  };
  swap();
  return holder.buf.length;
}

function counterCell(i: number): number {
  let x = i;
  const bump = () => {
    x += 1;
  };
  bump();
  return x;
}

const rssMb = () => process.memoryUsage().rss / 1048576;

function growthMb(run: (i: number) => number, calls: number): number {
  const before = rssMb();
  let acc = 0;
  for (let i = 1; i <= calls; i++) acc += run(i);
  if (acc <= 0) throw new Error("no work done");
  return rssMb() - before;
}

// Warm both shapes so allocator and heap high-water marks settle first; the
// control runs before the boxed shape so RSS reuse can only favor the control.
growthMb(noBox, 5000);
growthMb(payload, 5000);
const CALLS = 60000;
const controlGrowth = growthMb(noBox, CALLS);
const payloadGrowth = growthMb(payload, CALLS);
// Leaking every call's 512-element array costs ~5 KB * 60k = ~300 MB.
console.log("payload growth bounded by control:", payloadGrowth < Math.max(controlGrowth, 0) + 96);

let cellAcc = 0;
for (let i = 1; i <= 200000; i++) cellAcc += counterCell(i);
console.log("counter cells:", cellAcc);

// ---- escaping closures keep their cells ----

function makeCounter(start: number) {
  let n = start;
  return {
    inc: () => ++n,
    add: (k: number) => {
      n += k;
      return n;
    },
    get: () => n,
  };
}

const counters: Array<ReturnType<typeof makeCounter>> = [];
for (let i = 0; i < 3000; i++) counters.push(makeCounter(i));
churn(20);
let counterSum = 0;
for (let i = 0; i < counters.length; i++) {
  counters[i].inc();
  counterSum += counters[i].add(i);
}
churn(20);
for (let i = 0; i < counters.length; i += 7) counterSum += counters[i].get();
console.log("returned counters:", counterSum);

function makeAdders(): Map<string, (v: number) => number> {
  const map = new Map<string, (v: number) => number>();
  for (let i = 0; i < 500; i++) {
    let base = i;
    map.set("k" + i, (v: number) => (base += v));
  }
  return map;
}
const adders = makeAdders();
churn(15);
let adderSum = 0;
for (const [, fn] of adders) adderSum += fn(1);
churn(15);
for (const [, fn] of adders) adderSum += fn(2);
console.log("per-iteration map closures:", adderSum);

// A loop whose iterations sometimes capture and sometimes do not: released
// uncaptured cells are reused by the next iteration, captured ones must not be.
function mixedLoop(): number[] {
  const kept: Array<() => number> = [];
  for (let i = 0; i < 400; i++) {
    let value = i * 3;
    const touch = () => (value += 1);
    if (i % 3 === 0) kept.push(touch);
    else touch();
    value += 0;
  }
  churn(10);
  return kept.map((f) => f()).slice(0, 8);
}
console.log("mixed loop:", mixedLoop().join(","));

// Nested closure created after the outer frame returned.
function outerFactory() {
  let state = "a";
  const middle = () => {
    state += "b";
    return () => {
      state += "c";
      return state;
    };
  };
  return middle;
}
const middles: Array<() => () => string> = [];
for (let i = 0; i < 200; i++) middles.push(outerFactory());
churn(10);
const inners = middles.map((m) => m());
churn(10);
console.log("nested after return:", inners[0](), inners[199](), middles[5]()());

// Self-referencing payload: the cell's value references its own closure.
function selfCycle(i: number) {
  let self: any = null;
  const getSelf = () => self;
  self = { i, getSelf };
  return getSelf;
}
const cycles: Array<() => any> = [];
for (let i = 0; i < 20000; i++) {
  const g = selfCycle(i);
  if (i % 1000 === 0) cycles.push(g);
}
churn(15);
console.log("self cycles:", cycles.map((g) => g().getSelf().i).join(","));

// Generators capture boxed state across suspensions; abandoned ones are freed.
function* running(limit: number) {
  let total = 0;
  const add = (v: number) => {
    total += v;
  };
  for (let i = 1; i <= limit; i++) {
    add(i);
    yield total;
  }
  return total;
}
const gens: Array<Generator<number, number>> = [];
for (let i = 0; i < 50; i++) gens.push(running(5));
let genSum = 0;
for (let step = 0; step < 6; step++) {
  churn(3);
  for (const g of gens) {
    const r = g.next();
    genSum += r.value ?? 0;
  }
}
for (let i = 0; i < 5000; i++) running(3).next();
console.log("generators:", genSum);

// Class method frames and closures stored on the instance.
class Account {
  report: () => string = () => "";
  owner: string;
  constructor(owner: string) {
    this.owner = owner;
  }
  open(deposit: number) {
    let balance = deposit;
    let history = [deposit];
    this.report = () => `${this.owner}:${balance}:${history.length}`;
    return (amount: number) => {
      balance += amount;
      history = history.concat([amount]);
      return balance;
    };
  }
}
const accounts: Account[] = [];
const deposits: Array<(n: number) => number> = [];
for (let i = 0; i < 300; i++) {
  const acct = new Account("u" + i);
  accounts.push(acct);
  deposits.push(acct.open(i));
}
churn(10);
for (let i = 0; i < deposits.length; i++) deposits[i](10);
churn(10);
console.log("class frames:", accounts[0].report(), accounts[299].report());

// Early returns, throws and finally on frames that own cells.
function exits(mode: number): number {
  let seen = mode;
  const mark = () => (seen += 100);
  try {
    if (mode === 0) return mark();
    if (mode === 1) throw new Error("boom" + seen);
    mark();
  } finally {
    seen += 1;
  }
  return seen;
}
let exitSum = 0;
for (let i = 0; i < 3000; i++) {
  try {
    exitSum += exits(i % 3);
  } catch (e) {
    exitSum += (e as Error).message.length;
  }
}
console.log("exits:", exitSum);

// Recursion: every frame owns its own cell.
function depth(n: number): number {
  let local = n;
  const bump = () => (local += 1);
  if (n > 0) local += depth(n - 1);
  bump();
  return local;
}
console.log("recursion:", depth(200));

// Parameters captured and reassigned are boxed too.
function paramCell(a: number, b: string) {
  const again = () => {
    a += 1;
    b = b + a;
  };
  again();
  return () => b + ":" + a;
}
const paramFns: Array<() => string> = [];
for (let i = 0; i < 1000; i++) paramFns.push(paramCell(i, "p"));
churn(10);
console.log("params:", paramFns[0](), paramFns[999]());

// async without await, and an await-ing async closure that captures a cell
// owned by an enclosing synchronous frame which returns before it resumes.
async function noAwait(i: number) {
  let y = i;
  const f = () => {
    y++;
  };
  f();
  return y;
}

function syncOwner(seed: number) {
  let shared = seed;
  const read = () => shared;
  void (async () => {
    await null;
    churn(2);
    shared += 1000;
  })();
  shared += 1;
  return read;
}

async function main() {
  let asyncSum = 0;
  for (let i = 0; i < 2000; i++) asyncSum += await noAwait(i);
  console.log("async no await:", asyncSum);

  const readers: Array<() => number> = [];
  for (let i = 0; i < 20; i++) readers.push(syncOwner(i));
  churn(10);
  await new Promise<void>((resolve) => setTimeout(resolve, 10));
  churn(10);
  console.log("async capture of outer cell:", readers.map((r) => r()).join(","));
}

main().then(() => console.log("done"));
