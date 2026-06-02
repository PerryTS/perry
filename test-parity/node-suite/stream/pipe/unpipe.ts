import { Readable, Writable } from "node:stream";
// unpipe(writable) detaches a previously-piped target; subsequent reads
// shouldn't reach it.
const r = new Readable({ read() {} });
let writes = 0;
const w = new Writable({
  write(_c, _e, cb) { writes++; cb(); },
});
r.pipe(w);
r.unpipe(w);
r.push("after-unpipe");
r.push(null);
setImmediate(() => console.log("writes after unpipe:", writes));
