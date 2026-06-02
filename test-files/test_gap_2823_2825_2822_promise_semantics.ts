// Promise semantics parity (#2823 identity, #2825 finally adoption,
// #2822 iterable combinators).

async function main() {
  // ---- #2823: Promise.resolve identity ----
  const fulfilled = Promise.resolve(1);
  console.log("resolve-identity-fulfilled:", Promise.resolve(fulfilled) === fulfilled);

  const pending = new Promise<number>(() => {});
  console.log("resolve-identity-pending:", Promise.resolve(pending) === pending);

  const thenable = { then(resolve: (v: number) => void) { resolve(42); } };
  console.log("resolve-thenable-identity:", (Promise.resolve(thenable as any) as any) === thenable);

  // ---- #2825: finally adoption ----
  console.log("finally-ignored:", await Promise.resolve("v").finally(() => "ignored"));

  const order: string[] = [];
  await Promise.resolve("v")
    .finally(() => new Promise<string>(resolve => setTimeout(() => {
      order.push("cleanup");
      resolve("x");
    }, 1)))
    .then(v => order.push("then:" + v));
  console.log("finally-order:", JSON.stringify(order));

  try {
    await Promise.resolve("v").finally(() => Promise.reject("cleanup"));
    console.log("finally-reject-promise: NO THROW");
  } catch (e) {
    console.log("finally-reject-promise:", e);
  }

  try {
    await Promise.reject("orig").finally(() => Promise.reject("cleanup"));
    console.log("finally-reject-override: NO THROW");
  } catch (e) {
    console.log("finally-reject-override:", e);
  }

  try {
    await Promise.resolve("v").finally(() => { throw new Error("boom"); });
    console.log("finally-throw: NO THROW");
  } catch (e) {
    console.log("finally-throw:", (e as Error).message);
  }

  console.log("finally-noncallable:", await Promise.resolve("v").finally(1 as any));

  // ---- #2822: iterable combinators ----
  console.log("all-set:", JSON.stringify(await Promise.all(new Set([Promise.resolve(1), 2]))));

  const settled = await Promise.allSettled(new Set([Promise.resolve(1), Promise.reject("x")]));
  console.log("allSettled-set:", JSON.stringify(settled.map(r => r.status)));

  console.log("race-set:", await Promise.race(new Set([Promise.resolve("a")])));
  console.log("any-set:", await Promise.any(new Set([Promise.reject("x"), Promise.resolve("b")])));

  try {
    await Promise.all(1 as any);
    console.log("all-number: NO THROW");
  } catch (e) {
    console.log("all-number:", (e as Error).name, (e as Error).message);
  }

  try {
    await Promise.all(undefined as any);
    console.log("all-undefined: NO THROW");
  } catch (e) {
    console.log("all-undefined:", (e as Error).name, (e as Error).message);
  }

  try {
    await Promise.race(1 as any);
    console.log("race-number: NO THROW");
  } catch (e) {
    console.log("race-number:", (e as Error).name);
  }

  try {
    await Promise.any(1 as any);
    console.log("any-number: NO THROW");
  } catch (e) {
    console.log("any-number:", (e as Error).name);
  }

  try {
    await Promise.any([]);
    console.log("any-empty: NO THROW");
  } catch (e) {
    console.log("any-empty-errors:", (e as AggregateError).errors.length);
  }
}

main();
