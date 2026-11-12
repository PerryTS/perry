// `new Promise(executor)` is the ONLY asynchronous construct of this program:
// no async function, no `await`, no timer, no `Promise.` static, no
// `queueMicrotask`. The compiler's "this program schedules nothing" proof
// read the constructor's name as data, ended `main` without the event loop,
// and every reaction below was dropped: the program printed "sync end" only.
// Keep it that way: one more asynchronous construct here hides the bug.

const thenable = {
  then(resolve: any) {
    resolve("thenable");
  },
};
class Later {
  then(resolve: any) {
    resolve("class thenable");
  }
}

new Promise((resolve: any) => resolve(1)).then((v: any) => console.log("fulfilled", v));
new Promise((_resolve: any, reject: any) => reject(new Error("no"))).catch((e: any) =>
  console.log("rejected", e.message),
);
const chain: any = new Promise((resolve: any) => resolve("chain"));
chain
  .then((v: any) => {
    console.log("then", v);
    return thenable;
  })
  .then((v: any) => console.log("then", v))
  .finally(() => console.log("finally"));
new Promise((resolve: any) => resolve(thenable)).then((v: any) => console.log("adopted", v));
new Promise((resolve: any) => resolve(new Later())).then((v: any) => console.log("adopted", v));
new Promise(() => {
  throw new RangeError("executor");
}).then(undefined, (e: any) => console.log("executor threw", e instanceof RangeError, e.message));
console.log("sync end");
