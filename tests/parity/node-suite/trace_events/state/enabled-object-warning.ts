import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";
import { createTracing } from "node:trace_events";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-warning-"));
const originalEmitWarning = process.emitWarning;
const tracers: ReturnType<typeof createTracing>[] = [];
let warnings = 0;
let expectedMessage = false;

try {
  chdir(temporary);
  process.emitWarning = ((warning: string | Error) => {
    warnings++;
    const message = warning instanceof Error
      ? warning.message
      : String(warning);
    expectedMessage = message.includes("more than 10 enabled Tracing objects");
  }) as typeof process.emitWarning;

  for (let index = 0; index < 11; index++) {
    const tracing = createTracing({ categories: [`warning-${index}`] });
    tracing.enable();
    tracers.push(tracing);
  }
  console.log("warnings/message:", warnings, expectedMessage);
} finally {
  for (const tracing of tracers) tracing.disable();
  process.emitWarning = originalEmitWarning;
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
