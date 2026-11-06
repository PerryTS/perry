// turnloop P0: timer ordering and counts across the precise park — timeouts at
// sub-millisecond, 2 ms and 10 ms, a sub-millisecond remainder, an interval,
// unref'd and ref'd handles, clearTimeout, and promise churn between parks.
async function sleep(ms: number): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, ms));
}

async function main() {
  for (const delay of [0.5, 2, 10]) {
    await sleep(delay);
    console.log("timeout", delay);
  }

  const start = performance.now();
  const remainder = new Promise<void>((resolve) => setTimeout(resolve, 2));
  while (performance.now() - start < 1.6) {
    // leave less than a millisecond for the park
  }
  await remainder;
  console.log("remainder");

  const order: string[] = [];
  setTimeout(() => order.push("b"), 5);
  setTimeout(() => order.push("a"), 1);
  const cancelled = setTimeout(() => order.push("cancelled"), 2);
  clearTimeout(cancelled);
  await sleep(15);
  console.log("order", order.join(","));

  let ticks = 0;
  await new Promise<void>((resolve) => {
    const interval = setInterval(() => {
      ticks++;
      if (ticks === 3) {
        clearInterval(interval);
        resolve();
      }
    }, 2);
  });
  console.log("interval", ticks);

  const unrefed = setTimeout(() => console.log("unref'd timer must not fire"), 60_000);
  unrefed.unref();
  console.log("hasRef", unrefed.hasRef());
  unrefed.ref();
  unrefed.unref();

  let churn = 0;
  for (let i = 0; i < 1000; i++) {
    await Promise.resolve();
    churn++;
  }
  console.log("churn", churn);
  await sleep(10);
  console.log("done");
}

main();
