// The process methods are callable function values.
console.log("cwd:", typeof process.cwd === "function");
console.log("nextTick:", typeof process.nextTick === "function");
console.log("hrtime:", typeof process.hrtime === "function");
console.log("exit:", typeof process.exit === "function");
console.log("on:", typeof process.on === "function");
console.log("uptime:", typeof process.uptime === "function");
console.log("memoryUsage:", typeof process.memoryUsage === "function");
console.log("cpuUsage:", typeof process.cpuUsage === "function");
