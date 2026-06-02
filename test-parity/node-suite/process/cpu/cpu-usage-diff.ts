// process.cpuUsage(prev) returns the elapsed { user, system } since `prev`.
const a = process.cpuUsage();
const d = process.cpuUsage(a);
console.log("user:", typeof d.user === "number");
console.log("system:", typeof d.system === "number");
console.log("non-negative:", d.user >= 0 && d.system >= 0);
