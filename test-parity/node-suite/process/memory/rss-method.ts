// process.memoryUsage.rss() is a fast path returning just the RSS number.
console.log("rss is function:", typeof process.memoryUsage.rss === "function");
console.log("rss() is number:", typeof process.memoryUsage.rss() === "number");
