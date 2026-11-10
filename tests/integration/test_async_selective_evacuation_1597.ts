// Regression test for #1597: selective generational evacuation must keep
// Promise async-resumption edges up to date after young closures/promises move.
//
// Expected output:
//   await: ok
//   outer post: ok
//   AFTER nested - reached

async function runCb(cb: () => Promise<void>) {
  await cb();
}

async function viaAwait() {
  await Promise.resolve();
  console.log("await: ok");
}

async function nested() {
  await runCb(async () => {
    await runCb(async () => {
      await Promise.resolve();
    });
    await Promise.resolve();
    console.log("outer post: ok");
  });
}

(async () => {
  await runCb(viaAwait);
  await nested();
  console.log("AFTER nested - reached");
})();
