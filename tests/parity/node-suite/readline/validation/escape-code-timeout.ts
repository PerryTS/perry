import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

for (const value of [-1, NaN, Infinity, "50"]) {
  const input = new PassThrough();
  try {
    createInterface({ input, escapeCodeTimeout: value as any }).close();
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  } finally {
    input.destroy();
  }
}
