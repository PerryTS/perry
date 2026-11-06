// process.uptime() is monotonic across successive reads.
const a = process.uptime();
const b = process.uptime();
console.log("monotonic:", b >= a);
