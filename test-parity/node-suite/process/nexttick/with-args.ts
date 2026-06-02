// process.nextTick(cb, ...args) invokes the callback with the trailing args.
process.nextTick((a: string, b: string) => console.log("args:", a, b), "x", "y");
console.log("sync");
