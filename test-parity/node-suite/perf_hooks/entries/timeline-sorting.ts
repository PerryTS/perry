import { performance } from "node:perf_hooks";
performance.clearMarks();
performance.clearMeasures();
try {
  performance.mark("late", { startTime: 20 });
  performance.mark("early", { startTime: 5 });
  performance.measure("span", { start: 5, duration: 3 });
  console.log(
    performance.getEntries().map((e) =>
      `${e.name}:${e.entryType}:${e.startTime}`
    ).join(","),
  );
} finally {
  performance.clearMarks();
  performance.clearMeasures();
}
