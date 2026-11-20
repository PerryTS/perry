import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
rl.close();
for (const call of [() => rl.pause(), () => rl.resume()]) {
  try {
    call();
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  }
}
input.destroy();
