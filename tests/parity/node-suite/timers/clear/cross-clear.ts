const events: string[] = [];
const t = setTimeout(() => events.push("timeout"), 5);
clearInterval(t as any);
const i = setInterval(() => events.push("interval"), 5);
clearTimeout(i as any);
clearTimeout(null as any);
clearInterval(undefined as any);
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(",") || "none");
