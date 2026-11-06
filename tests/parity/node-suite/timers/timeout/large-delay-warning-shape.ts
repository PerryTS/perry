const events: string[] = [];
const t = setTimeout(() => events.push("large"), 2 ** 31);
clearTimeout(t);
setTimeout(() => events.push("small"), 1);
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(","));
