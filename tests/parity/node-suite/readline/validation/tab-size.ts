import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

for (const value of [0, 4.5, "4"]) {
  const input = new PassThrough();
  try {
    createInterface({ input, tabSize: value as any }).close();
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  } finally {
    input.destroy();
  }
}
