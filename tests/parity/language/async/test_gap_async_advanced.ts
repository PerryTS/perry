// Test: Advanced async features (Perry gap analysis)
// These features are NOT yet supported by Perry — this file documents the target behavior.
// Run: node --experimental-strip-types tests/parity/language/async/test_gap_async_advanced.ts

// --- Async iterable + for await...of ---
async function* asyncRange(start: number, end: number): AsyncGenerator<number> {
  for (let i = start; i < end; i++) {
    await new Promise<void>((r) => setTimeout(r, 1));
    yield i;
  }
}

async function testForAwaitOf(): Promise<void> {
  const result: number[] = [];
  for await (const n of asyncRange(0, 5)) {
    result.push(n);
  }
  console.log(result.join(",")); // 0,1,2,3,4
}

// --- async function* (async generator) with yield and await ---
async function* fetchPages(pages: number): AsyncGenerator<string> {
  for (let i = 1; i <= pages; i++) {
    await new Promise<void>((r) => setTimeout(r, 1));
    yield `page-${i}`;
  }
}

async function testAsyncGenerator(): Promise<void> {
  const gen = fetchPages(3);
  const r1 = await gen.next();
  console.log(r1.value); // page-1
  console.log(r1.done);  // false

  const r2 = await gen.next();
  console.log(r2.value); // page-2

  const r3 = await gen.next();
  console.log(r3.value); // page-3

  const r4 = await gen.next();
  console.log(r4.done); // true
}

// --- Async generator with interleaved yield and await ---
async function* asyncCounter(): AsyncGenerator<string> {
  let count = 0;
  while (count < 3) {
    const doubled = await Promise.resolve(count * 2);
    yield `count=${count},doubled=${doubled}`;
    count++;
  }
}

async function testAsyncCounterGen(): Promise<void> {
  const results: string[] = [];
  for await (const line of asyncCounter()) {
    results.push(line);
  }
  console.log(results[0]); // count=0,doubled=0
  console.log(results[1]); // count=1,doubled=2
  console.log(results[2]); // count=2,doubled=4
}

// --- Promise.allSettled() ---
async function testAllSettled(): Promise<void> {
  const results = await Promise.allSettled([
    Promise.resolve(1),
    Promise.reject(new Error("fail")),
    Promise.resolve(3),
  ]);

  console.log(results[0].status); // fulfilled
  if (results[0].status === "fulfilled") {
    console.log(results[0].value); // 1
  }

  console.log(results[1].status); // rejected
  if (results[1].status === "rejected") {
    console.log(results[1].reason.message); // fail
  }

  console.log(results[2].status); // fulfilled
  if (results[2].status === "fulfilled") {
    console.log(results[2].value); // 3
  }
}

// --- Promise.any() — first fulfilled ---
async function testPromiseAny(): Promise<void> {
  const result = await Promise.any([
    new Promise<number>((resolve) => setTimeout(() => resolve(1), 50)),
    Promise.reject(new Error("nope")),
    Promise.resolve(3),
  ]);
  console.log(result); // 3
}

// --- Promise.any() with all rejections → AggregateError ---
async function testPromiseAnyAllReject(): Promise<void> {
  try {
    await Promise.any([
      Promise.reject(new Error("e1")),
      Promise.reject(new Error("e2")),
      Promise.reject(new Error("e3")),
    ]);
    console.log("should not reach");
  } catch (e: any) {
    console.log(e instanceof AggregateError); // true
    console.log(e.errors.length);             // 3
    console.log(e.errors[0].message);         // e1
    console.log(e.errors[1].message);         // e2
    console.log(e.errors[2].message);         // e3
  }
}

// --- Promise.race() edge cases ---
async function testPromiseRace(): Promise<void> {
  // Race with immediate resolve vs delayed
  const r1 = await Promise.race([
    new Promise<string>((resolve) => setTimeout(() => resolve("slow"), 100)),
    Promise.resolve("fast"),
  ]);
  console.log(r1); // fast

  // Race where rejection wins
  try {
    await Promise.race([
      new Promise<string>((resolve) => setTimeout(() => resolve("slow"), 100)),
      Promise.reject(new Error("race-err")),
    ]);
    console.log("should not reach");
  } catch (e: any) {
    console.log(e.message); // race-err
  }
}

// --- Promise.withResolvers() ---
async function testWithResolvers(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<number>();

  setTimeout(() => resolve(42), 10);
  const val = await promise;
  console.log(val); // 42

  // Test reject path
  const { promise: p2, reject } = Promise.withResolvers<number>();
  setTimeout(() => reject(new Error("wr-err")), 10);
  try {
    await p2;
    console.log("should not reach");
  } catch (e: any) {
    console.log(e.message); // wr-err
  }
}

// --- queueMicrotask() ---
async function testQueueMicrotask(): Promise<void> {
  const order: string[] = [];

  order.push("sync1");

  queueMicrotask(() => {
    order.push("microtask1");
  });

  queueMicrotask(() => {
    order.push("microtask2");
  });

  order.push("sync2");

  // Wait a tick for microtasks to drain
  await new Promise<void>((r) => setTimeout(r, 10));

  console.log(order.join(",")); // sync1,sync2,microtask1,microtask2
}

// --- Async disposal: await using (TC39 Stage 3 / Node 22+) ---
async function testAsyncDisposal(): Promise<void> {
  const disposed: string[] = [];

  class AsyncResource {
    name: string;
    constructor(name: string) {
      this.name = name;
    }
    async [Symbol.asyncDispose](): Promise<void> {
      await new Promise<void>((r) => setTimeout(r, 1));
      disposed.push(this.name);
    }
  }

  {
    await using r1 = new AsyncResource("res1");
    await using r2 = new AsyncResource("res2");
    // r2 disposed first (reverse order), then r1
  }

  // Disposed in reverse declaration order
  console.log(disposed.join(",")); // res2,res1
}

// --- Run all tests ---
async function main(): Promise<void> {
  await testForAwaitOf();
  await testAsyncGenerator();
  await testAsyncCounterGen();
  await testAllSettled();
  await testPromiseAny();
  await testPromiseAnyAllReject();
  await testPromiseRace();
  await testWithResolvers();
  await testQueueMicrotask();
  try {
    await testAsyncDisposal();
  } catch (e: any) {
    // await using may not be supported in all Node versions
    console.log("async disposal skipped: " + e.message);
  }
  console.log("ALL ASYNC ADVANCED TESTS PASSED");
}

main();
