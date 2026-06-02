import { EventEmitter } from "node:events";

const em = new EventEmitter();
try {
  em.emit("error", new Error("boom"));
  console.log("threw:", false);
} catch (e: any) {
  console.log("threw:", true);
  console.log("message:", e.message);
}
