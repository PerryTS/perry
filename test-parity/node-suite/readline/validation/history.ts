import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

for (const value of ["bad", 123, {}, true, null]) {
  const input = new PassThrough();
  try {
    createInterface({ input, history: value as any }).close();
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  } finally {
    input.destroy();
  }
}
