import { Writable } from "node:stream";
// writable.writableCorked counts the active corks (incremented per cork(),
// decremented per uncork()).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("corked initial:", w.writableCorked);
w.cork();
console.log("after 1 cork:", w.writableCorked);
w.cork();
console.log("after 2 corks:", w.writableCorked);
w.uncork();
console.log("after 1 uncork:", w.writableCorked);
w.uncork();
console.log("after 2 uncorks:", w.writableCorked);
