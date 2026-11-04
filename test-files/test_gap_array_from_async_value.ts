// Array statics remain callable when read as values or invoked reflectively.
async function run() {
  console.log("type", typeof Array.fromAsync);
  console.log("own", Object.hasOwn(Array, "fromAsync"));
  console.log("metadata", Array.fromAsync.length, Array.fromAsync.name);
  console.log("direct", (await Array.fromAsync([1, 2, 3])).join(","));
  console.log("call", (await Array.fromAsync.call({}, [4, 5], (x: number, i: number) => x + i)).join(","));
  const f = Array.fromAsync;
  console.log("held", (await f([Promise.resolve(6), 7], (x: number) => Promise.resolve(x * 2))).join(","));
  console.log("held-call", (await f.call(null, [8, 9])).join(","));
  const context = { offset: 10 };
  console.log("thisArg", (await f.call(false, [1, 2], function (this: any, x: number) {
    return x + this.offset;
  }, context)).join(","));
}
run();
