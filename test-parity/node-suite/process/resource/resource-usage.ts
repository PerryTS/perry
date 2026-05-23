// process.resourceUsage() returns a struct of numeric usage counters.
const r = process.resourceUsage();
console.log("maxRSS:", typeof r.maxRSS === "number");
console.log("userCPUTime:", typeof r.userCPUTime === "number");
