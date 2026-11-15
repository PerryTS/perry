// The runtime's promise machinery (resolving functions, combinator element
// functions, finally thunks, await continuations, thenable jobs) keeps its
// state in closures that capture promises, arrays and other closures. Those
// captures are heap references in value slots: a collection between the
// creation of a reaction and its run must keep and move them like any other
// value. Every case allocates between scheduling and settling.

function churn(n: number): number {
  let keep: any[] = [];
  let total = 0;
  for (let i = 0; i < n; i++) {
    const o = { i, s: "v" + i, a: [i, i + 1, i + 2] };
    if (i % 89 === 0) keep.push(o);
    if (keep.length > 64) keep = keep.slice(32);
    total += o.a.length;
  }
  return total + keep.length;
}

function later<T>(value: T, ticks: number): Promise<T> {
  return new Promise<T>((resolve) => {
    let left = ticks;
    const step = () => {
      churn(3000);
      if (--left <= 0) resolve(value);
      else Promise.resolve().then(step);
    };
    Promise.resolve().then(step);
  });
}

function failing(reason: string, ticks: number): Promise<never> {
  return later(0, ticks).then(() => {
    throw new Error(reason);
  });
}

const thenable = (value: any, ticks: number) => ({
  then(resolve: (v: any) => void) {
    later(value, ticks).then(resolve);
  },
});

async function combinators(): Promise<void> {
  const inputs = [later("a", 3), later({ k: [1, 2] }, 1), 7, thenable("t", 2), later("z", 5)];
  churn(30000);
  console.log("all", JSON.stringify(await Promise.all(inputs)));
  const settled = await Promise.allSettled([later(1, 2), failing("no", 3), thenable(3, 1), 4]);
  churn(30000);
  console.log("allSettled", JSON.stringify(settled.map((r) => (r.status === "fulfilled" ? ["f", r.value] : ["r", (r.reason as Error).message]))));
  console.log("any", await Promise.any([failing("x", 1), later("first", 4), later("second", 6)]));
  try {
    await Promise.any([failing("e1", 1), failing("e2", 3)]);
  } catch (e) {
    const err = e as AggregateError;
    console.log("any rejects", err.constructor.name, err.errors.map((x: Error) => x.message).join(","));
  }
  console.log("race", await Promise.race([later("slow", 6), later("fast", 1), failing("late", 9)]));
  try {
    await Promise.all([later(1, 4), failing("all-fails", 2), later(3, 6)]);
  } catch (e) {
    console.log("all rejects", (e as Error).message);
  }
  const many: Promise<number>[] = [];
  for (let i = 0; i < 200; i++) many.push(later(i, 1 + (i % 4)));
  churn(60000);
  const sum = (await Promise.all(many)).reduce((a, b) => a + b, 0);
  console.log("all 200", sum);
}

async function chains(): Promise<void> {
  const log: string[] = [];
  const v = await later("v", 2)
    .finally(() => {
      log.push("finally-1");
      churn(20000);
      return later("ignored", 2);
    })
    .then((x) => x + "!")
    .finally(() => log.push("finally-2"));
  try {
    await failing("boom", 2).finally(() => {
      log.push("finally-3");
      churn(20000);
    });
  } catch (e) {
    log.push("caught " + (e as Error).message);
  }
  try {
    await later(1, 1).finally(() => {
      throw new Error("from finally");
    });
  } catch (e) {
    log.push("caught " + (e as Error).message);
  }
  console.log("finally", v, log.join(" "));

  // Resolution with a promise and with a thenable: the resolving functions
  // capture the outer promise.
  const outer = new Promise((resolve) => {
    churn(10000);
    resolve(later("inner", 3));
  });
  const viaThenable = new Promise((resolve) => resolve(thenable("via-thenable", 2)));
  const nested = Promise.resolve(later(later("deep", 2), 2));
  churn(40000);
  console.log("resolve", await outer, await viaThenable, await nested);

  let resolveLater: (v: string) => void = () => {};
  let rejectLater: (e: Error) => void = () => {};
  const p1 = new Promise<string>((resolve) => (resolveLater = resolve));
  const p2 = new Promise<string>((_, reject) => (rejectLater = reject));
  const seen: string[] = [];
  for (let i = 0; i < 5; i++) {
    p1.then((x) => seen.push("f" + i + x));
    p2.catch((e) => seen.push("r" + i + e.message));
  }
  churn(60000);
  resolveLater("!");
  resolveLater("ignored");
  rejectLater(new Error("?"));
  await later(0, 2);
  console.log("deferred", seen.join(" "));
}

async function awaits(): Promise<void> {
  async function leaf(i: number): Promise<number> {
    await later(0, 1);
    churn(2000);
    return i * 2;
  }
  async function mid(i: number): Promise<number> {
    const a = await leaf(i);
    const b = await leaf(i + 1);
    try {
      if (i % 5 === 0) await failing("mid" + i, 1);
    } catch (e) {
      return -(a + b);
    }
    return a + b;
  }
  let total = 0;
  for (let i = 0; i < 40; i++) total += await mid(i);
  const parallel = await Promise.all([mid(1), mid(2), mid(5), mid(10)]);
  console.log("await", total, JSON.stringify(parallel));

  async function* gen(n: number): AsyncGenerator<number, string, unknown> {
    for (let i = 0; i < n; i++) {
      churn(3000);
      yield await later(i, 1);
    }
    return "done";
  }
  const got: number[] = [];
  for await (const x of gen(12)) got.push(x);
  const it = gen(3);
  const steps = [await it.next(), await it.next(), await it.return("early"), await it.next()];
  console.log("for await", got.join(","), JSON.stringify(steps));
  // Array.fromAsync over array and Set sources (an async-generator source is a
  // separate, open collection-safety bug of `Array.fromAsync` and is not part
  // of this fixture).
  const mapped = await (Array as any).fromAsync([later(1, 2), 2, thenable(3, 1), 4, later(5, 3)], (x: number) => later(x * 10, 1));
  const fromSync = await (Array as any).fromAsync([later(1, 2), 2, thenable(3, 1)]);
  const fromSet = await (Array as any).fromAsync(new Set([later(7, 1), 8]));
  console.log("fromAsync", JSON.stringify(mapped), JSON.stringify(fromSync), JSON.stringify(fromSet));
}

async function main(): Promise<void> {
  await combinators();
  await chains();
  await awaits();
  console.log("done");
}

main().catch((e) => console.log("FAILED", e));
