// Issue #4828: an async fn whose tail returns an un-awaited promise
// (`return someAsyncCall()`) must adopt the returned promise's eventual
// state, not resolve to the Promise object itself.
async function inner(): Promise<{ id: string }> {
  await Promise.resolve();
  return { id: "abc", n: 1 } as any;
}
async function outerBroken(): Promise<{ id: string }> {
  await Promise.resolve();
  return inner(); // un-awaited tail promise
}
async function outerFixed(): Promise<{ id: string }> {
  await Promise.resolve();
  const r = await inner(); // explicit await
  return r;
}
const a = await outerBroken();
const b = await outerFixed();
console.log("broken: typeof=" + typeof a + " json=" + JSON.stringify(a) + " .id=" + JSON.stringify((a as any)?.id));
console.log("fixed:  typeof=" + typeof b + " json=" + JSON.stringify(b) + " .id=" + JSON.stringify((b as any)?.id));
