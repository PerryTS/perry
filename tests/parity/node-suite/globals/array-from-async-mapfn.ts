function showArray(label: string, value: unknown[]): void {
  console.log(label + ":", value.join("|"));
}

async function showError(label: string, run: () => Promise<unknown>): Promise<void> {
  try {
    await run();
    console.log(label + ": ok");
  } catch (err) {
    const e = err as Error;
    console.log(label + ":", e.constructor.name);
  }
}

async function* values() {
  yield Promise.resolve(3);
  yield 4;
}

async function main(): Promise<void> {
  showArray("array", await Array.fromAsync([1, 2, 3]));
  showArray("promise-array", await Array.fromAsync([
    Promise.resolve(10),
    Promise.resolve(20),
  ]));

  const ctx = { factor: 10 };
  const mapped = await Array.fromAsync([1, 2], async function (this: typeof ctx, value, index) {
    return value * this.factor + index;
  }, ctx);
  showArray("map-this", mapped);

  const genMapped = await Array.fromAsync(values(), async (value, index) => value + index);
  showArray("async-iter-map", genMapped);

  const arrayLike = await Array.fromAsync({
    0: Promise.resolve(5),
    1: 6,
    length: 2,
  });
  showArray("array-like", arrayLike);

  await showError("noncallable", () => Array.fromAsync([1], 123 as unknown as (value: number) => number));
  await showError("null-input", () => Array.fromAsync(null as unknown as Iterable<number>));
  await showError("undefined-input", () => Array.fromAsync(undefined as unknown as Iterable<number>));
}

await main();
