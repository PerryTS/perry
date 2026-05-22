const events: string[] = [];
const id = setInterval((a: string) => {
  events.push(a);
  if (events.length === 2) clearInterval(id);
}, 1, "tick");
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(","));
