import EventEmitter from "node:events";

const em = new EventEmitter();
let heard = 0;
em.on("ping", () => { heard++; });
em.emit("ping");
console.log("heard:", heard);
