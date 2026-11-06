import { Writable } from "node:stream";
// The final(cb) option fires after end() before 'finish' so user code can
// flush trailing state.
let called = false;
const w = new Writable({
  write(_c, _e, cb) { cb(); },
  final(cb) { called = true; cb(); },
});
w.on("finish", () => console.log("final ran:", called));
w.end();
