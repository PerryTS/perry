// process.nextTick(cb, ...args) — JS spec forwards the trailing args to
// the callback when the tick fires. Regression cover for #1351 (Perry was
// silently dropping them).
await new Promise<void>((resolve) => {
  process.nextTick(
    (a: string, b: number, c: boolean) => {
      console.log("args:", a, b, c);
      resolve();
    },
    "x",
    2,
    true,
  );
});
await new Promise<void>((resolve) => {
  process.nextTick((v: string) => {
    console.log("single:", v);
    resolve();
  }, "hello");
});
await new Promise<void>((resolve) => {
  process.nextTick(() => {
    console.log("no args ok");
    resolve();
  });
});
