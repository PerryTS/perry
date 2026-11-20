// SNC 8: AsyncLocalStorage run/enterWith in the loop, getStore per iteration.
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();
const log: string[] = [];
async function loop(tag: string): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await i;
    log.push(tag + i + ":" + als.getStore());
    if (i === 2) als.enterWith(tag + "-entered");
  }
}
als.run("outer", () => {
  loop("x").then(() => log.push("x-done:" + als.getStore()));
});
als.run("other", () => {
  loop("y").then(() => log.push("y-done:" + als.getStore()));
});
loop("z").then(() => log.push("z-done:" + als.getStore()));
setTimeout(() => console.log(log.join("\n")), 5);
