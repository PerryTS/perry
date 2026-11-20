import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
try {
  createInterface({ input, signal: {} as AbortSignal }).close();
  console.log("ok");
} catch (error: any) {
  console.log(error.name, error.code);
} finally {
  input.destroy();
}
