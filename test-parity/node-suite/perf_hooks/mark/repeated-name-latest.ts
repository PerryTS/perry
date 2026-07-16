import { performance } from "node:perf_hooks";
performance.clearMarks();
performance.clearMeasures();
try {
  performance.mark("same", { startTime: 1 });
  performance.mark("same", { startTime: 5 });
  console.log(
    "entries:",
    performance.getEntriesByName("same", "mark").map((entry) => entry.startTime)
      .join(","),
  );
  console.log(
    "latest lookup:",
    performance.measure("from-latest", "same").startTime,
  );
  performance.clearMarks("same");
  try {
    performance.measure("missing", "same");
    console.log("missing no throw");
  } catch (error) {
    console.log("missing:", (error as Error).name);
  }
} finally {
  performance.clearMarks();
  performance.clearMeasures();
}
