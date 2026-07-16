import { performance } from "node:perf_hooks";
performance.clearMarks();
try {
  const entry = performance.mark("identity", { startTime: 1 });
  const first = performance.getEntriesByType("mark");
  const second = performance.getEntriesByType("mark");
  first.length = 0;
  console.log("fresh arrays:", first !== second);
  console.log("same entries:", second[0] === entry);
  console.log(
    "mutation isolated:",
    performance.getEntriesByType("mark").length,
  );
} finally {
  performance.clearMarks();
}
