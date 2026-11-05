import { setTimeout } from "node:timers";
await new Promise<void>((resolve) => {
  setTimeout(() => {
    console.log("timeout fired");
    resolve();
  }, 0);
});
