import { EventEmitter, getEventListeners, getMaxListeners, setMaxListeners, listenerCount } from "node:events";

const em = new EventEmitter();
em.on("x", () => {});
setMaxListeners(13, em);
console.log("listeners:", getEventListeners(em, "x").length);
console.log("max:", getMaxListeners(em));
console.log("count:", listenerCount(em, "x"));
