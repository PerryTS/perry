// Async generator instances: one birth layout, a record-held prototype, the
// request queue in the generator's own record, and next/return/throw
// inherited from %AsyncGeneratorPrototype%. Every line is compared to node.
const log: string[] = [];
const out = (s: string) => log.push(s);
const tick = (tag: string) => Promise.resolve().then(() => out("tick " + tag));

async function* counter(n: number) {
  try {
    for (let i = 0; i < n; i++) {
      out("yield " + i);
      const sent = yield i;
      if (sent !== undefined) out("sent " + String(sent));
    }
    return "end";
  } finally {
    out("counter finally");
  }
}

async function queueOrder() {
  const g = counter(2);
  const p1 = g.next();
  const p2 = g.next("a");
  const p3 = g.next("b");
  const p4 = g.next("c");
  tick("q");
  const rs = await Promise.all([p1, p2, p3, p4]);
  for (const r of rs) out("q " + JSON.stringify(r));
}

async function returnAtYield() {
  const g = counter(5);
  out("r1 " + JSON.stringify(await g.next()));
  const pr = g.return(Promise.resolve("bye"));
  const pn = g.next();
  out("r2 " + JSON.stringify(await pr));
  out("r3 " + JSON.stringify(await pn));
}

async function* catcher() {
  try {
    yield 1;
  } catch (e) {
    out("caught " + String(e));
    yield 2;
  }
  yield 3;
}

async function throwBeforeStart() {
  let entered = false;
  async function* body() {
    entered = true;
    yield 1;
  }
  const g = body();
  try {
    await g.throw(new Error("early"));
  } catch (e) {
    out("t1 " + (e as Error).message + " entered=" + entered);
  }
  out("t2 " + JSON.stringify(await g.next()));
  const c = catcher();
  out("t3 " + JSON.stringify(await c.next()));
  out("t4 " + JSON.stringify(await c.throw("boom")));
  out("t5 " + JSON.stringify(await c.next()));
  out("t6 " + JSON.stringify(await c.next()));
}

async function forAwaitBreak() {
  for await (const v of counter(10)) {
    out("fa " + v);
    if (v === 1) break;
  }
  let sum = 0;
  for await (const v of counter(3)) sum += v;
  out("fa sum " + sum);
}

async function* inner() {
  yield "i1";
  await null;
  yield "i2";
  return "iret";
}

async function* outer() {
  const r = yield* inner();
  out("delegate returned " + r);
  yield* [Promise.resolve("s1"), "s2", Promise.resolve("s3")];
  yield* {
    [Symbol.asyncIterator]() {
      let i = 0;
      return {
        next() {
          i++;
          return Promise.resolve({ value: "m" + i, done: i > 2 });
        },
      };
    },
  };
}

async function delegation() {
  const vs: string[] = [];
  for await (const v of outer()) vs.push(String(v));
  out("yield* " + vs.join(","));
  const g = outer();
  out("d1 " + JSON.stringify(await g.next()));
  out("d2 " + JSON.stringify(await g.return("stop")));
  out("d3 " + JSON.stringify(await g.next()));
}

async function* thenables() {
  yield { then(r: (v: number) => void) { r(42); } };
  const x = yield Promise.resolve(7);
  out("thenable sent " + String(x));
  return { then(r: (v: string) => void) { r("ret-thenable"); } };
}

async function thenableValues() {
  const g = thenables();
  out("th1 " + JSON.stringify(await g.next()));
  out("th2 " + JSON.stringify(await g.next()));
  out("th3 " + JSON.stringify(await g.next("s")));
  out("th4 " + JSON.stringify(await g.next()));
  const rejecting = thenables();
  await rejecting.next();
  try {
    await rejecting.return(Promise.reject(new Error("ret-rejected")));
  } catch (e) {
    out("th5 " + (e as Error).message);
  }
  out("th6 " + JSON.stringify(await rejecting.next()));
}

async function microtaskOrder() {
  async function* two() {
    yield 1;
    yield 2;
  }
  const g = two();
  g.next().then((r) => out("mo next1 " + r.value));
  tick("a");
  g.next().then((r) => out("mo next2 " + r.value));
  tick("b");
  g.next().then((r) => out("mo next3 " + r.done));
  tick("c");
  await new Promise((r) => setTimeout(r, 0));
}

async function* retPrim() { return 5; }
async function* retPromise() { return Promise.resolve(6); }
async function* retRejectCaught() {
  try {
    return await Promise.reject(new Error("rj"));
  } catch (e) {
    yield "caught " + (e as Error).message;
  }
}

async function returnOrder() {
  retPrim().next().then((r) => out("ro prim " + r.value));
  retPromise().next().then((r) => out("ro promise " + r.value));
  tick("x");
  tick("y");
  tick("z");
  await new Promise((r) => setTimeout(r, 0));
  const g = retRejectCaught();
  out("ro c1 " + JSON.stringify(await g.next()));
  out("ro c2 " + JSON.stringify(await g.next()));
}

class Holder {
  base = 10;
  async *items() {
    yield this.base;
    yield this.base + 1;
  }
}

async function shapes() {
  const AsyncGeneratorPrototype = Object.getPrototypeOf(counter.prototype);
  const g = counter(1);
  out("p1 " + (Object.getPrototypeOf(g) === counter.prototype));
  out("p2 " + (Object.getPrototypeOf(counter.prototype) === AsyncGeneratorPrototype));
  out("p3 " + (g[Symbol.asyncIterator]() === g));
  out("p4 " + Object.prototype.toString.call(g));
  out("p5 " + Object.keys(g).length + " " + Object.getOwnPropertyNames(g).length);
  out("p6 " + typeof g.next + " " + (g.next === AsyncGeneratorPrototype.next));
  const replaced = { marker: 1 };
  Object.setPrototypeOf(replaced, AsyncGeneratorPrototype);
  (counter as any).prototype = replaced;
  const g2 = counter(1);
  out("p7 " + (Object.getPrototypeOf(g2) === replaced) + " " + (Object.getPrototypeOf(g) !== replaced));
  (counter as any).prototype = 5;
  out("p8 " + (Object.getPrototypeOf(counter(1)) === AsyncGeneratorPrototype));
  const h = new Holder();
  const vs: number[] = [];
  for await (const v of h.items()) vs.push(v);
  out("p9 " + vs.join(","));
  out("p10 " + (Object.getPrototypeOf(h.items()) === Holder.prototype.items.prototype));
  (g as any).extra = 3;
  out("p11 " + JSON.stringify(await g.next()) + " " + (g as any).extra);
}

async function brands() {
  const AsyncGeneratorPrototype = Object.getPrototypeOf(counter.prototype);
  for (const recv of [{}, counter.prototype, AsyncGeneratorPrototype, 1]) {
    try {
      await AsyncGeneratorPrototype.next.call(recv);
      out("b next resolved");
    } catch (e) {
      out("b " + (e as Error).constructor.name);
    }
  }
  function* sync() {
    yield 1;
  }
  try {
    await AsyncGeneratorPrototype.next.call(sync());
  } catch (e) {
    out("b sync " + (e as Error).constructor.name);
  }
  const SyncProto = Object.getPrototypeOf(sync.prototype);
  try {
    SyncProto.next.call(counter(1));
  } catch (e) {
    out("b async-to-sync " + (e as Error).constructor.name);
  }
}

async function liveness() {
  async function* waits() {
    await new Promise((r) => setTimeout(r, 5));
    yield "after-timer";
  }
  let p: Promise<IteratorResult<string>> | null = waits().next();
  const junk: unknown[] = [];
  for (let i = 0; i < 200000; i++) junk.push({ i, s: "x" + i });
  out("l1 " + JSON.stringify(await p));
  p = null;
  let born = 0;
  for (let i = 0; i < 50000; i++) {
    const g = counter(3);
    if (i % 2 === 0) g.next();
    born++;
  }
  out("l2 " + born);
}

// Async generator methods (named, symbol-keyed, computed) are async
// generator functions: their instances answer %AsyncGeneratorPrototype%'s
// next/return/throw, also when the method is read off the instance.
const methodKey = "dyn" + "Key";
class Methods {
  async *[Symbol.asyncIterator]() { yield "sym"; }
  async *[methodKey]() { yield "computed"; }
  async *named() { yield "named"; }
  static async *[Symbol.for("st")]() { yield "static"; }
}
async function methodKinds() {
  const AGP = Object.getPrototypeOf(Object.getPrototypeOf((async function* () {})()));
  const m: any = new Methods();
  const its: [string, any][] = [
    ["sym", m[Symbol.asyncIterator]()],
    ["computed", m[methodKey]()],
    ["named", m.named()],
    ["static", (Methods as any)[Symbol.for("st")]()],
  ];
  for (const [n, it] of its) {
    out("m " + n + " " + (Object.getPrototypeOf(Object.getPrototypeOf(it)) === AGP));
    const next = it.next;
    out("m " + n + " " + JSON.stringify(await next.call(it)));
  }
  for await (const v of m) out("m for-await " + v);
  out("m named.prototype " + (Object.getPrototypeOf(m.named.prototype) === AGP));
}

// `return E` is `return ? Await(E)` inside the enclosing try: a returned
// rejected promise is caught by the generator's own catch.
async function* retReject() { try { return Promise.reject(new Error("rr1")); } catch (e) { yield "caught " + (e as Error).message; } }
async function* retRejectLocal() { try { const p = Promise.reject(new Error("rr2")); return p; } catch (e) { yield "caught " + (e as Error).message; } }
async function* retRejectFinally() { try { return Promise.reject(new Error("rr3")); } catch (e) { yield "caught " + (e as Error).message; } finally { out("r finally"); } }
async function* retResolveFinally() { try { return Promise.resolve("rv"); } finally { out("r finally2"); } }
async function returnInTry() {
  for (const g of [retReject(), retRejectLocal(), retRejectFinally(), retResolveFinally()]) {
    try { out("r " + JSON.stringify(await g.next())); out("r " + JSON.stringify(await g.next())); }
    catch (e) { out("r rejected " + (e as Error).message); }
  }
}

async function main() {
  await queueOrder();
  await returnAtYield();
  await throwBeforeStart();
  await forAwaitBreak();
  await delegation();
  await thenableValues();
  await microtaskOrder();
  await returnOrder();
  await shapes();
  await brands();
  await liveness();
  await methodKinds();
  await returnInTry();
  console.log(log.filter((l) => !l.startsWith("yield ") || log.length < 400).join("\n"));
}
main();
