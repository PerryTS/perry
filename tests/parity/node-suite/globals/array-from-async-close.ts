// Array.fromAsync reads an array-like's length before Construct and its
// elements after, and closes the source iterator on every abrupt step.
const log: string[] = [];

async function show(label: string, run: () => Promise<unknown[]>): Promise<void> {
  try {
    const result = await run();
    console.log(label + ": resolved", JSON.stringify(Array.from(result)));
  } catch (err) {
    console.log(label + ": rejected", (err as Error).constructor.name);
  }
  if (log.length > 0) console.log("  " + log.join(" | "));
  log.length = 0;
}

function Recorder(this: unknown, ...args: unknown[]): void {
  log.push("construct(" + args.join(",") + ")");
}

function NonExtensible(this: object): void {
  Object.preventExtensions(this);
}

function syncSource(): Iterable<number> {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next: () => ({ value: i++, done: false }),
        return: () => {
          log.push("sync return");
          return { value: undefined, done: true };
        },
      };
    },
  };
}

function asyncSource(): AsyncIterable<unknown> {
  return {
    [Symbol.asyncIterator]() {
      let i = 0;
      return {
        next: () => Promise.resolve({ value: i++, done: false }),
        return: () => {
          log.push("async return");
          return Promise.resolve({ value: undefined, done: true });
        },
      };
    },
  };
}

function rawAsync(next: () => unknown): AsyncIterable<unknown> {
  return { [Symbol.asyncIterator]: () => ({ next }) } as unknown as AsyncIterable<unknown>;
}

async function main(): Promise<void> {
  const arrayLike = {
    get length() { log.push("get length"); return 2; },
    get 0() { log.push("get 0"); return Promise.resolve("a"); },
    get 1() { log.push("get 1"); return "b"; },
  };
  await show("array-like order", () => Array.fromAsync.call(Recorder, arrayLike) as Promise<unknown[]>);
  await show("array-like element throws", () =>
    Array.fromAsync({ length: 1, get 0() { throw new Error("element"); } } as ArrayLike<unknown>));
  await show("number source", () => Array.fromAsync(5 as unknown as ArrayLike<unknown>));

  await show("sync mapfn throws", () => Array.fromAsync(syncSource(), () => { throw new Error("map"); }));
  await show("async mapfn throws", () => Array.fromAsync(asyncSource(), () => { throw new Error("map"); }));
  await show("async mapfn rejects", () => Array.fromAsync(asyncSource(), async () => { throw new Error("map"); }));
  await show("sync define fails", () => Array.fromAsync.call(NonExtensible, syncSource()) as Promise<unknown[]>);
  await show("async define fails", () => Array.fromAsync.call(NonExtensible, asyncSource()) as Promise<unknown[]>);

  await show("next throws", () => Array.fromAsync(rawAsync(() => { throw new Error("next"); })));
  await show("result not an object", () => Array.fromAsync(rawAsync(() => 1)));
  let step = 0;
  await show("plain result objects", () =>
    Array.fromAsync(rawAsync(() => (step < 2 ? { value: step++, done: false } : { done: true }))));
  let once = false;
  const values = await Array.fromAsync(rawAsync(() => {
    const result = { value: Promise.resolve(7), done: once };
    once = true;
    return Promise.resolve(result);
  }));
  console.log("async values are not awaited:", values[0] instanceof Promise);
  class StaticIterable {
    static *[Symbol.iterator]() { yield "s"; }
  }
  await show("class with static @@iterator", () => Array.fromAsync(StaticIterable as unknown as Iterable<string>));
  await show("done read before value", () => Array.fromAsync(rawAsync(() => ({
    get done() { log.push("get done"); return true; },
    get value() { log.push("get value"); return 1; },
  }))));
}

await main();
