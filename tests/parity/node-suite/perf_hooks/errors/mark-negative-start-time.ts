import { performance } from "node:perf_hooks";

try {
  performance.mark("neg", { startTime: -1 });
  console.log("mark negative: ok");
} catch (e) {
  console.log(`mark negative: ${(e as Error).name}:${(e as any).code || "no-code"}`);
}
