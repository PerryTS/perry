import { performance } from "node:perf_hooks";
performance.mark("direct named export", { detail: { value: 9 } });
console.log(performance.getEntriesByName("direct named export")[0].detail.value);
performance.clearMarks("direct named export");
