import * as os from "node:os";

const cpus = os.cpus();
console.log("cpus array:", Array.isArray(cpus));
console.log("cpus length number:", typeof cpus.length === "number");
