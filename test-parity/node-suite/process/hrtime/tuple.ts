// process.hrtime() returns [seconds, nanoseconds] from a monotonic clock.
const t = process.hrtime();
console.log("is array:", Array.isArray(t));
console.log("length 2:", t.length === 2);
console.log("secs is number:", typeof t[0] === "number");
console.log("nanos is number:", typeof t[1] === "number");
