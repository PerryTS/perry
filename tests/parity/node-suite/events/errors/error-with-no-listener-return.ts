import { EventEmitter } from "node:events";

const ee = new EventEmitter();
try { console.log("emit error ret:", ee.emit("error", new Error("boom"))); } catch (err: any) { console.log("emit error threw:", err.name, err.message); }
ee.on("error", () => {});
console.log("emit error handled:", ee.emit("error", new Error("ok")));
