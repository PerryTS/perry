import * as os from "node:os";

console.log("priority object:", os.constants.priority !== null && typeof os.constants.priority === "object");
console.log("normal zero:", os.constants.priority.PRIORITY_NORMAL === 0);
console.log("low number:", typeof os.constants.priority.PRIORITY_LOW === "number");
