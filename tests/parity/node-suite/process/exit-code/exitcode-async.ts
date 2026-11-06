// #6666: exitCode set inside an async task that completes before the event
// loop drains. Natural exit still honours the stored code (node rc=7).
async function work() {
  await Promise.resolve();
  process.exitCode = 7;
}
work();
console.log("scheduled");
