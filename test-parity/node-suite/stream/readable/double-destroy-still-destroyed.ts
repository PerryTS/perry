import { Readable } from "node:stream";
// destroy(); destroy(); — destroyed flag remains true.
const r = new Readable({ read() {} });
r.destroy();
const first = r.destroyed;
r.destroy();
const second = r.destroyed;
console.log("first:", first);
console.log("second:", second);
console.log("both true:", first === true && second === true);
