import { finished, pipeline } from "node:stream/promises";

async function printRejection(label: string, fn: () => Promise<unknown>) {
  try {
    await fn();
    console.log(`${label}: fulfilled`);
  } catch (err) {
    const error = err as any;
    console.log(`${label}: ${error.name} ${error.code} ${error.message}`);
  }
}

await printRejection("finished(123)", () => finished(123 as any));
await printRejection("finished(\"x\")", () => finished("x" as any));
await printRejection("pipeline()", () => pipeline());
await printRejection("pipeline(123)", () => pipeline(123 as any));
await printRejection("pipeline(123, 456)", () => pipeline(123 as any, 456 as any));
