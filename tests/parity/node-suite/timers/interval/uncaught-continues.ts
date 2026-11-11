const events: string[] = [];
process.once("uncaughtException", (err: any) => events.push("caught:" + err.message));
const interval = setInterval(() => {
  clearInterval(interval);
  throw new Error("boom");
}, 1);
setTimeout(() => events.push("after"), 2);
await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log("events:", events.sort().join("|"));
