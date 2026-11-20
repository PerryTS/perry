import { performance } from "node:perf_hooks";
performance.clearMarks();
performance.mark("same", { startTime: 1 });
performance.mark("same", { startTime: 2 });
performance.mark("other", { startTime: 3 });
performance.clearMarks("same");
console.log("same:", performance.getEntriesByName("same", "mark").length);
console.log("other:", performance.getEntriesByName("other", "mark").length);
performance.clearMarks();
