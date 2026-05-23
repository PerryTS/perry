// process.cpuUsage() returns { user, system } microsecond counters.
const c = process.cpuUsage();
console.log("user:", typeof c.user === "number");
console.log("system:", typeof c.system === "number");
