import * as os from "node:os";

const uptime = os.uptime();
console.log("uptime finite:", Number.isFinite(uptime));
console.log("uptime nonnegative:", uptime >= 0);
