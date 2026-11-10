import { setTimeout, clearTimeout, clearInterval } from "node:timers";
// clearTimeout/clearInterval should tolerate non-Timer arguments without
// throwing AND must not accidentally clear an unrelated live timer.
let fired = 0;
const live = setTimeout(() => { fired++; }, 5);
clearTimeout({} as any);
clearInterval({ valueOf: () => 1 } as any);
await new Promise((resolve) => setTimeout(resolve, 20));
console.log("object clear ok");
console.log("unrelated timer fired:", fired);
clearTimeout(live);
