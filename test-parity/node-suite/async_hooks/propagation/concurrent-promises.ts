import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const firstGate = deferred();
const secondGate = deferred();
const thirdGate = deferred();

const first = storage.run("first", async () => {
  await firstGate.promise;
  console.log("first continuation:", storage.getStore());
  return "first-result";
});
const second = storage.run("second", async () => {
  await secondGate.promise;
  console.log("second continuation:", storage.getStore());
  return "second-result";
});
const third = storage.run("third", async () => {
  await thirdGate.promise;
  console.log("third continuation:", storage.getStore());
  return "third-result";
});

secondGate.resolve();
console.log("second result:", await second);
firstGate.resolve();
console.log("first result:", await first);
thirdGate.resolve();
console.log("third result:", await third);
console.log("concurrent outside:", String(storage.getStore()));
