import { Readable } from "node:stream";
// destroy() is idempotent — calling twice is fine.
const r = new Readable({ read() {} });
r.destroy();
let threw = false;
try { r.destroy(); } catch { threw = true; }
console.log("second destroy threw:", threw);
