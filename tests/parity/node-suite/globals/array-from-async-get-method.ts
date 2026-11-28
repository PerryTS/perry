// Array.fromAsync reads @@asyncIterator and then @@iterator once each
// (GetMethod), rejects on a bad method, boxes a primitive source, and
// still drains every built-in iterable.
const log: string[] = [];

async function show(label: string, run: () => Promise<unknown[]>): Promise<void> {
  try {
    console.log(label + ": resolved", JSON.stringify(Array.from(await run(), String)));
  } catch (err) {
    console.log(label + ": rejected", (err as Error).constructor.name);
  }
  if (log.length > 0) console.log("  " + log.join(" | "));
  log.length = 0;
}

function observed(asyncIterator: unknown, iterator: unknown): object {
  const items: Record<PropertyKey, unknown> = {};
  const define = (key: PropertyKey, name: string, value: unknown) =>
    Object.defineProperty(items, key, { get() { log.push("get " + name); return value; } });
  define(Symbol.asyncIterator, "@@asyncIterator", asyncIterator);
  define(Symbol.iterator, "@@iterator", iterator);
  define("length", "length", 2);
  define(0, "0", "a");
  define(1, "1", "b");
  return items;
}

const from = (items: unknown) => Array.fromAsync(items as ArrayLike<unknown>);

async function* agen() { yield 1; yield 2; }
function* gen() { yield 3; yield 4; }

async function main(): Promise<void> {
  await show("both nullish", () => from(observed(undefined, null)));
  await show("@@asyncIterator not callable", () => from(observed(true, undefined)));
  await show("@@iterator not callable", () => from(observed(undefined, 1)));
  await show("@@asyncIterator throws", () => from(observed(() => { throw new RangeError("a"); }, undefined)));
  await show("@@iterator throws", () => from(observed(null, () => { throw new RangeError("s"); })));
  await show("@@iterator result not an object", () => from(observed(undefined, () => 1)));
  await show("too long", () => Array.fromAsync.call({}, { length: 2 ** 32 }) as Promise<unknown[]>);

  for (const C of [Number, Boolean, BigInt, Symbol] as unknown as { prototype: Record<PropertyKey, unknown> }[]) {
    C.prototype.length = 2;
    C.prototype[0] = "x";
    C.prototype[1] = "y";
  }
  await show("number", () => from(1));
  Object.defineProperty(Number.prototype, Symbol.iterator, {
    configurable: true,
    value: function (this: unknown) {
      "use strict";
      return [typeof this][Symbol.iterator]();
    },
  });
  await show("strict @@iterator receives the primitive", () => from(2));
  delete (Number.prototype as unknown as Record<symbol, unknown>)[Symbol.iterator];
  await show("boolean", () => from(true));
  await show("bigint", () => from(1n));
  await show("symbol", () => from(Symbol("s")));
  for (const C of [Number, Boolean, BigInt, Symbol] as unknown as { prototype: Record<PropertyKey, unknown> }[]) {
    delete C.prototype.length;
    delete C.prototype[0];
    delete C.prototype[1];
  }

  await show("array", () => from([1, 2]));
  await show("string", () => from("hé"));
  await show("set", () => from(new Set([1, 2])));
  await show("map", () => from(new Map([[1, 2]])));
  await show("map keys", () => from(new Map([[1, 2]]).keys()));
  await show("typed array", () => from(new Uint8Array([5, 6])));
  await show("generator", () => from(gen()));
  await show("async generator", () => from(agen()));
  await show("own nullish @@iterator on an array", () =>
    from(Object.defineProperty([1, 2], Symbol.iterator, { value: undefined })));
  await show("headers", () => from(new Headers([["x", "y"]])));
  await show("iterable class", () => from(new (class { *[Symbol.iterator]() { yield 9; } })()));
}

await main();
