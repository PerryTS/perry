import * as os from "node:os";

console.log("signals object:", os.constants.signals !== null && typeof os.constants.signals === "object");
console.log("SIGINT number:", typeof os.constants.signals.SIGINT === "number");
console.log("SIGTERM number:", typeof os.constants.signals.SIGTERM === "number");
