import { Readable } from "node:stream";
// pause()/resume() toggle isPaused() and gate the flow of 'data' events.
const r = new Readable({ read() {} });
console.log("default paused:", r.isPaused());
r.on("data", () => {});
console.log("after data listener:", r.isPaused());
r.pause();
console.log("after pause:", r.isPaused());
r.resume();
console.log("after resume:", r.isPaused());
