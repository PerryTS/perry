// process.resourceUsage() returns a struct of getrusage(RUSAGE_SELF) counters.
const r = process.resourceUsage();
console.log("is object:", typeof r === "object");
console.log("userCPUTime is number:", typeof r.userCPUTime === "number");
console.log("maxRSS is number:", typeof r.maxRSS === "number");
