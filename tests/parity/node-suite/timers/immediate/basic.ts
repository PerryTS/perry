import { setImmediate } from "node:timers";
await new Promise<void>((resolve) => {
  setImmediate(() => {
    console.log("immediate fired");
    resolve();
  });
});
