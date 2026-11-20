import { EventEmitter, getMaxListeners, setMaxListeners } from "node:events";

const em = new EventEmitter();
setMaxListeners(7, em);
console.log("max:", getMaxListeners(em));
