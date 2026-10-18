// A Readable with a `_read` hook is read again after every push while its
// buffer is below the high-water mark (node's `reading` flag, `maybeReadMore`,
// and `read()` calling `_read` before it hands out the head chunk), flowing
// and paused, with synchronous and asynchronous pushes.
import { Readable } from "node:stream";

// Each stream logs on its own: the order within a stream is node's, the
// interleaving of timers and immediates across streams is not asserted.
const logs: Record<string, string[]> = {};
const out = (s: string) => (logs[s.split(" ")[0]] ??= []).push(s);
let ended = 0;
const end = (tag: string) => { out(tag + " end"); if (++ended === 5) setTimeout(report, 10); };
function report() { for (const k of ["sf", "af", "sp", "ap", "om"]) console.log(logs[k].join("\n")); }

// Flowing, synchronous push from a class `_read`.
class SyncFlow extends Readable {
  n = 0;
  _read() { out("sf _read " + this.n); this.push(this.n < 3 ? String(this.n++) : null); }
}
const sf = new SyncFlow();
sf.on("data", (d: any) => out("sf data " + String(d)));
sf.on("end", () => end("sf"));

// Flowing, asynchronous push.
class AsyncFlow extends Readable {
  n = 0;
  _read() { out("af _read " + this.n); setTimeout(() => this.push(this.n < 3 ? String(this.n++) : null), 1); }
}
const af = new AsyncFlow();
af.on("data", (d: any) => out("af data " + String(d)));
af.on("end", () => end("af"));

// Paused ('readable' + read()), synchronous push.
class SyncPaused extends Readable {
  n = 0;
  _read() { out("sp _read " + this.n); this.push(this.n < 3 ? String(this.n++) : null); }
}
const sp = new SyncPaused();
sp.on("readable", () => { let c; while ((c = sp.read()) !== null) out("sp read " + String(c)); });
sp.on("end", () => end("sp"));

// Paused, asynchronous push.
let apN = 0;
const ap = new Readable({
  read() { out("ap _read " + apN); setImmediate(() => ap.push(apN < 3 ? String(apN++) : null)); },
});
ap.on("readable", () => { let c; while ((c = ap.read()) !== null) out("ap read " + String(c)); });
ap.on("end", () => end("ap"));

// Object mode with a small high-water mark: reads ahead only to the mark.
let omN = 0;
const om = new Readable({
  objectMode: true,
  highWaterMark: 2,
  read() { out("om _read " + omN); this.push(omN < 5 ? { v: omN++ } : null); },
});
setTimeout(() => {
  out("om buffered " + om.readableLength);
  om.on("data", (d: any) => out("om data " + d.v));
  om.on("end", () => end("om"));
}, 5);
