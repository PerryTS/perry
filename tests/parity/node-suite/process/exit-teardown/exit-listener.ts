// The exit and beforeExit listeners run, once, before the process ends.
process.on("beforeExit", (code: number) => console.log("beforeExit", code));
process.on("exit", (code: number) => console.log("exit", code));
console.log("body");
