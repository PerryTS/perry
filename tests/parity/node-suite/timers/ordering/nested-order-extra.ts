const events: string[] = [];
setTimeout(() => {
  events.push("timeout1");
  queueMicrotask(() => events.push("micro-in-timeout"));
  setTimeout(() => events.push("timeout2"), 0);
}, 0);
Promise.resolve().then(() => events.push("promise"));
await new Promise(resolve => setTimeout(resolve, 30));
console.log("events:", events.join("|"));
