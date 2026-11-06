const events: string[] = [];
setTimeout(() => events.push("neg"), -1);
setTimeout(() => events.push("nan"), NaN as any);
setTimeout(() => events.push("zero"), 0);
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.sort().join(","));
