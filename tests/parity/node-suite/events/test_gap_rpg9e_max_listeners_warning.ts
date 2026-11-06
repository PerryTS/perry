// The max-listener warning reaches process.emitWarning without the program
// naming the process module.
import { EventEmitter } from "events";
process.on("warning", (w: any) => {
  console.log(w.name, w.count, w.type);
});
const e = new EventEmitter();
e.setMaxListeners(1);
e.on("x", () => {});
e.on("x", () => {});
