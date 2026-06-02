import { Writable } from "node:stream";
// end() with no chunk + callback fires finish; passing null as chunk is
// effectively the same.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("finish", () => console.log("finish"));
(w as any).end();
