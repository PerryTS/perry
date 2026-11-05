// SNC 9: async_hooks createHook enabled mid-loop, then v8 promise hooks.
import * as ah from "node:async_hooks";
import { promiseHooks } from "node:v8";
const c = { init: 0, before: 0, after: 0, v8before: 0 };
const log: string[] = [];
async function loop(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    if (i === 4) ah.createHook({ init() { c.init++; }, before() { c.before++; }, after() { c.after++; } }).enable();
    if (i === 8) promiseHooks.onBefore(() => { c.v8before++; });
    await i;
    log.push("a" + i);
  }
}
loop().then(() => {
  log.push("end");
  setTimeout(() => console.log(log.join(" "), c.init > 0, c.before > 0, c.after > 0, c.v8before > 0), 5);
});
