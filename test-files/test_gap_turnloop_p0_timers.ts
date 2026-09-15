// P0: callback counts prove every timer, interval and promise path ran.
// As in Node, setTimeout(0.5) is clamped to 1 ms at API registration.
// Rust driver tests separately assert a real 500 us Instant deadline.
async function main() {
  for (const delay of [0.5, 2, 10]) {
    await new Promise<void>((resolve) => setTimeout(resolve, delay));
    console.log("timeout", delay);
  }
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
  let promises = 0;
  for (let i = 0; i < 100; i++) {
    await Promise.resolve();
    promises++;
  }
  console.log("promises", promises);
  await new Promise<void>((resolve) => setTimeout(resolve, 10));
  console.log("idle complete");
}
main();
