// A thenable is adopted by every resolution path (ECMA-262 27.2.1.3.2 Promise
// Resolve Functions, NewPromiseResolveThenableJob): the executor's `resolve`,
// `Promise.resolve`, a `then` handler's result, an async function's return,
// `await`, and the combinators. The `then` is read once, called one job later
// with a fresh pair of resolving functions (first call wins), and the order
// against other jobs is the engine's. Negative controls: primitives, plain
// objects and native promises keep their own paths and their own tick.

const log: string[] = [];
const out = (s: string): void => {
  log.push(s);
};
const flush = (title: string): void => {
  console.log("== " + title);
  for (const line of log) console.log(line);
  log.length = 0;
};
// A timer turn: every job queued so far has run.
const turn = (ms: number = 0): Promise<void> => new Promise<void>((r) => setTimeout(r, ms));
const show = (v: any): string => {
  if (v instanceof Error) return v.constructor.name;
  if (typeof v === "function") return "function";
  if (v !== null && typeof v === "object") return "object:" + Object.keys(v).join(",");
  return typeof v + ":" + String(v);
};
const watch = (name: string, p: any): void => {
  p.then(
    (v: any) => out(name + " fulfilled " + show(v)),
    (e: any) => out(name + " rejected " + show(e)),
  );
};

class Later {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
  then(resolve: any, _reject: any): void {
    resolve(this.value);
  }
}
class Base {
  then(resolve: any): void {
    resolve("inherited-class");
  }
}
class Derived extends Base {}

let getterReads = 0;
const forms = (): [string, any][] => {
  const viaGetter: any = {};
  Object.defineProperty(viaGetter, "then", {
    get() {
      getterReads++;
      return (resolve: any) => resolve("getter");
    },
  });
  const viaProto: any = Object.create({
    then(resolve: any) {
      resolve("proto");
    },
  });
  const fn: any = function () {};
  fn.then = (resolve: any) => resolve("function-with-then");
  const proxy: any = new Proxy(
    {},
    {
      get(_t: any, key: any) {
        if (key === "then") return (resolve: any) => resolve("proxy");
        return undefined;
      },
    },
  );
  return [
    [
      "method",
      {
        then(resolve: any) {
          resolve("method");
        },
      },
    ],
    ["function-property", { then: function (resolve: any) { resolve("function-property"); } }],
    ["arrow-property", { then: (resolve: any) => resolve("arrow-property") }],
    ["class", new Later(7)],
    ["inherited-class", new Derived()],
    ["getter", viaGetter],
    ["proto", viaProto],
    ["function-with-then", fn],
    ["proxy", proxy],
  ];
};

async function returns(v: any): Promise<any> {
  return v;
}
async function awaits(name: string, v: any): Promise<void> {
  try {
    out(name + " awaited " + show(await v));
  } catch (e) {
    out(name + " threw " + show(e));
  }
}

async function main(): Promise<void> {
  // 1. Every path adopts every form.
  for (const [name, t] of forms()) watch("executor " + name, new Promise((res) => res(t)));
  await turn();
  flush("executor resolve");

  for (const [name, t] of forms()) watch("Promise.resolve " + name, Promise.resolve(t));
  await turn();
  flush("Promise.resolve");

  for (const [name, t] of forms()) watch("handler " + name, Promise.resolve(0).then(() => t));
  await turn();
  flush("then handler result");

  for (const [name, t] of forms()) watch("async return " + name, returns(t));
  await turn();
  flush("async function return");

  for (const [name, t] of forms()) awaits("await " + name, t);
  await turn();
  flush("await");

  for (const [name, t] of forms()) watch("finally " + name, Promise.resolve("kept").finally(() => t));
  await turn();
  flush("finally result is awaited, value kept");
  out("getter reads " + getterReads + " for 6 resolutions");
  flush("then is read once per resolution");

  // 2. Combinators.
  const mk = (v: any) => ({
    then(resolve: any) {
      resolve(v);
    },
  });
  const bad = (v: any) => ({
    then(_resolve: any, reject: any) {
      reject(v);
    },
  });
  watch("all", Promise.all([mk(1), 2, Promise.resolve(3), new Later(4)]).then((a: any) => a.join("|")));
  watch("all rejected", Promise.all([mk(1), bad(new RangeError("r"))]));
  watch(
    "allSettled",
    Promise.allSettled([mk(1), bad("no"), 3]).then((a: any) =>
      a.map((r: any) => r.status + "=" + (r.status === "fulfilled" ? r.value : r.reason)).join("|"),
    ),
  );
  watch("race", Promise.race([mk("first"), Promise.resolve("second")]));
  watch("race native first", Promise.race([Promise.resolve("native"), mk("thenable")]));
  watch("any", Promise.any([bad("x"), mk("ok")]));
  watch("any all rejected", Promise.any([bad("x"), bad("y")]).catch((e: any) => e.errors.join("|")));
  await turn();
  flush("combinators");

  // 3. What the thenable does with its resolving functions.
  const behaviours: [string, any][] = [
    ["resolve twice", { then(res: any) { res("one"); res("two"); } }],
    ["resolve then reject", { then(res: any, rej: any) { res("kept"); rej(new Error("dropped")); } }],
    ["reject then resolve", { then(res: any, rej: any) { rej(new TypeError("kept")); res("dropped"); } }],
    ["reject", { then(_res: any, rej: any) { rej("reason"); } }],
    ["throw before", { then() { throw new RangeError("before"); } }],
    ["throw after resolve", { then(res: any) { res("settled"); throw new Error("ignored"); } }],
    ["throw after reject", { then(_res: any, rej: any) { rej("rejected"); throw new Error("ignored"); } }],
    ["resolve with undefined", { then(res: any) { res(); } }],
    ["chain", { then(res: any) { res({ then(res2: any) { res2({ then(res3: any) { res3("depth 3"); } }); } }); } }],
    ["chain to rejection", { then(res: any) { res({ then(_r: any, rej: any) { rej("inner"); } }); } }],
    ["resolves with native", { then(res: any) { res(Promise.resolve("native inside")); } }],
    ["microtask later", { then(res: any) { queueMicrotask(() => res("microtask")); } }],
    ["timer later", { then(res: any) { setTimeout(() => res("timer"), 1); } }],
    ["returns a value", { then(res: any) { res("result ignored"); return 99; } }],
  ];
  for (const [name, t] of behaviours) watch("executor: " + name, new Promise((res) => res(t)));
  await turn(10);
  flush("thenable behaviours through the executor");
  for (const [name, t] of behaviours) awaits("await: " + name, t);
  await turn(10);
  flush("thenable behaviours through await");

  // A thenable that never calls back leaves the promise pending.
  let never = "pending";
  new Promise((res) => res({ then() {} })).then(
    () => (never = "fulfilled"),
    () => (never = "rejected"),
  );
  await turn(5);
  out("never: " + never);
  flush("never resolves");

  // 4. The `then` property itself.
  const throwing: any = {};
  Object.defineProperty(throwing, "then", {
    get() {
      throw new SyntaxError("getter");
    },
  });
  watch("executor throwing getter", new Promise((res) => res(throwing)));
  watch("Promise.resolve throwing getter", Promise.resolve(0).then(() => throwing));
  awaits("await throwing getter", throwing);
  const inert = [{ then: 1 }, { then: "x" }, { then: null }, { then: undefined }, { then: {} }, { then: true }];
  for (const o of inert) {
    watch(
      "non-callable then " + show(o.then),
      new Promise((res) => res(o)).then((v) => (v === o ? "same object" : "other")),
    );
  }
  const seen: any = {
    then(this: any, resolve: any, reject: any) {
      out(
        "then called: this " +
          (this === seen) +
          ", args " +
          arguments.length +
          ", " +
          typeof resolve +
          "/" +
          typeof reject +
          ", lengths " +
          resolve.length +
          "/" +
          reject.length +
          ", names " +
          JSON.stringify(resolve.name) +
          "/" +
          JSON.stringify(reject.name),
      );
      resolve("seen");
    },
  };
  watch("receiver and arguments", new Promise((res) => res(seen)));
  const primitives: any[] = [1, "s", true, null, undefined, 10n, Symbol.iterator];
  for (const v of primitives) {
    watch("primitive " + typeof v, new Promise((res) => res(v)).then((x) => (x === v ? "same" : "other")));
  }
  await turn();
  flush("the then property");

  // 5. Self resolution, native promises, a subclass, a foreign promise.
  let resolveSelf: any;
  const self: any = new Promise((res) => (resolveSelf = res));
  resolveSelf(self);
  watch("self", self);
  const selfChained: any = Promise.resolve(0).then(() => selfChained);
  watch("self from handler", selfChained);
  const native = Promise.resolve("native");
  watch("native through executor", new Promise((res) => res(native)));
  out("Promise.resolve(native) is native: " + (Promise.resolve(native) === native));
  watch("native rejected", new Promise((res) => res(Promise.reject(new EvalError("r")))));
  class Sub extends Promise<any> {}
  const sub = Sub.resolve("subclass");
  out("Sub.resolve is Sub: " + (sub instanceof Sub) + ", Promise.resolve(sub) is sub: " + (Promise.resolve(sub) === sub));
  watch("subclass through executor", new Promise((res) => res(sub)));
  watch("thenable through subclass", Sub.resolve({ then(res: any) { res("into subclass"); } }));
  // A promise of another realm is not branded here: only its `then` speaks.
  const inner = Promise.resolve("foreign");
  const foreign: any = {
    constructor: Promise,
    then(onFulfilled: any, onRejected: any) {
      return inner.then(onFulfilled, onRejected);
    },
    catch(onRejected: any) {
      return inner.catch(onRejected);
    },
  };
  watch("foreign through executor", new Promise((res) => res(foreign)));
  awaits("foreign", foreign);
  out("Promise.resolve(foreign) is a new promise: " + (Promise.resolve(foreign) !== foreign));
  await turn();
  flush("self, native, subclass, foreign");

  // 6. Order. Seven chains start in one synchronous stretch next to a tick
  // counter; a thenable costs one job more than a value, a native promise two.
  const ticks = (n: number): void => {
    let p: Promise<any> = Promise.resolve();
    for (let i = 1; i <= n; i++) p = p.then(() => out("tick " + i));
  };
  const loud = (name: string) => ({
    then(resolve: any) {
      out(name + ": then called");
      resolve(name);
    },
  });
  ticks(8);
  new Promise((res) => res("value")).then((v) => out("executor " + v));
  new Promise((res) => res(loud("executor thenable"))).then((v) => out("settled " + v));
  new Promise((res) => res(Promise.resolve("executor native"))).then((v) => out("settled " + v));
  Promise.resolve(loud("Promise.resolve thenable")).then((v) => out("settled " + v));
  Promise.resolve("value").then(() => loud("handler thenable")).then((v) => out("settled " + v));
  Promise.resolve("value").then(() => Promise.resolve("handler native")).then((v) => out("settled " + v));
  Promise.resolve("value").then(() => "handler value").then((v) => out("settled " + v));
  out("sync end");
  await turn();
  flush("order: resolve functions");

  ticks(8);
  returns("async value").then((v) => out("settled " + v));
  returns(loud("async thenable")).then((v) => out("settled " + v));
  returns(Promise.resolve("async native")).then((v) => out("settled " + v));
  awaits("await value", "v");
  awaits("await thenable", loud("awaited thenable"));
  awaits("await native", Promise.resolve("n"));
  (async () => {
    out("two awaits: start");
    await loud("first");
    out("two awaits: between");
    await loud("second");
    out("two awaits: end");
  })();
  out("sync end");
  await turn();
  flush("order: async functions and await");

  // A thenable settled from a job, a chain of thenables, a rejection.
  ticks(10);
  new Promise((res) =>
    res({
      then(resolve: any) {
        out("outer then");
        resolve({
          then(resolve2: any) {
            out("inner then");
            resolve2("nested");
          },
        });
      },
    }),
  ).then((v) => out("settled " + v));
  new Promise((res) =>
    res({
      then(_resolve: any, reject: any) {
        out("rejecting then");
        reject("rejected");
      },
    }),
  ).then(undefined, (e) => out("settled " + e));
  new Promise((res) =>
    res({
      then(resolve: any) {
        queueMicrotask(() => {
          out("queued resolve");
          resolve("from microtask");
        });
      },
    }),
  ).then((v) => out("settled " + v));
  queueMicrotask(() => out("queueMicrotask"));
  process.nextTick(() => out("nextTick"));
  out("sync end");
  await turn();
  flush("order: nested, rejected, deferred");

  // 7. 300 adoptions through one site; a job outlives its resolve call.
  let sum = 0;
  const all: Promise<any>[] = [];
  for (let i = 0; i < 300; i++) {
    const t = i % 3 === 0 ? new Later(i) : i % 3 === 1 ? mk(i) : i;
    all.push(new Promise((res) => res(t)).then((v: any) => (sum += v)));
  }
  await Promise.all(all);
  out("sum " + sum);
  flush("many");
}

main().then(
  () => console.log("done"),
  (e) => console.log("main rejected", e),
);
