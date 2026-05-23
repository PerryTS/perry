// process.threadCpuUsage() returns { user, system } in microseconds.
const u = process.threadCpuUsage();
console.log("ok:", typeof u === "object" && typeof u.user === "number" && typeof u.system === "number");
