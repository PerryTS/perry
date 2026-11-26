// A sloppy callback's `this` is its thisArg boxed (OrdinaryCallBindThis),
// BigInt included, whether the builtin or `call` supplies it.
function describe(this: unknown): string {
  return typeof this + ":" + String((this as { valueOf(): unknown }).valueOf());
}

for (const thisArg of [3.5, "s", true, 42n]) {
  console.log([0].map(describe, thisArg)[0], describe.call(thisArg));
}

async function main(): Promise<void> {
  const [viaFromAsync] = await Array.fromAsync([0], describe, 7n);
  console.log(viaFromAsync);
}

main();
