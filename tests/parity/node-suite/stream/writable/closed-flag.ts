import { Writable } from "node:stream";
// writable.closed reflects close-event state.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("close", () => console.log("closed:", w.closed));
w.end();
