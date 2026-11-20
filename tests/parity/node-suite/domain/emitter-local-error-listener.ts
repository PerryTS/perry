import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
d.on("error", () => console.log("domain"));
emitter.on("error", (error) => console.log("local", error.message));
console.log(emitter.emit("error", new Error("handled")));
d.remove(emitter);
