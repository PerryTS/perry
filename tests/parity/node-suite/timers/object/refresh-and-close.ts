const events: string[] = [];
const t: any = setTimeout(() => events.push("timeout"), 5);
console.log("refresh type:", typeof t.refresh);
console.log("close type:", typeof t.close);
t.refresh?.();
t.close?.();
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(",") || "none");
