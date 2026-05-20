import * as os from "node:os";

const load = os.loadavg();
console.log("load array:", Array.isArray(load));
console.log("load length:", load.length);
console.log("load numbers:", load.every((n) => typeof n === "number" && Number.isFinite(n)));
