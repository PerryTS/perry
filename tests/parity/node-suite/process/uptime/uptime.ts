// process.uptime() returns the process uptime in seconds (a non-negative
// number).
const u = process.uptime();
console.log("is number:", typeof u === "number");
console.log("non-negative:", u >= 0);
