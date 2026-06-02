// process.cpuUsage() returns { user, system } in microseconds.
const u = process.cpuUsage();
console.log("ok:", typeof u === "object" && typeof u.user === "number" && typeof u.system === "number");
