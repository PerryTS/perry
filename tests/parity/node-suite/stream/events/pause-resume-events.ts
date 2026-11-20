import { Readable } from "node:stream";
// pause()/resume() emit 'pause' and 'resume' events.
const r = new Readable({ read() {} });
let fires = "";
r.on("pause", () => (fires += "P"));
r.on("resume", () => (fires += "R"));
r.on("data", () => {});
r.pause();
r.resume();
console.log("fires:", fires);
