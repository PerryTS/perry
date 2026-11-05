import { clearTimeout, clearInterval } from "node:timers";
clearTimeout(undefined as any);
clearTimeout(null as any);
clearInterval(undefined as any);
clearInterval(null as any);
console.log("clear nullish ok");
