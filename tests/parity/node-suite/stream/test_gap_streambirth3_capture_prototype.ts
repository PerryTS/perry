import { EventEmitter } from "node:events";
let printing = true;
let sink = 0;
function log(...args: any[]) {
  const text = args.map((a) => String(a)).join(" ") + "#" + sink;
  sink = (sink + text.length) % 9973;
  if (printing) console.log(...args);
}
function witness() {
const E: any = EventEmitter;
const original = E.prototype;
const key = Object.getOwnPropertySymbols(original)[0];
log(String(key), E.captureRejections);
E.captureRejections = true;
log(E.captureRejections);
new E();
E.prototype = {};
log(E.captureRejections);
new E();
E.captureRejections = true;
log(E.captureRejections);
E.prototype = original;
log(E.captureRejections);
original[key] = false;
log(E.captureRejections);
new E();
E.prototype = Object.create(original);
log(E.captureRejections);
E.captureRejections = true;
log(E.captureRejections, original[key]);
new E();
E.prototype = null;
try { log(E.captureRejections); } catch (error: any) { log(error.name); }
try { new E(); log("constructed"); } catch (error: any) { log(error.name); }
E.prototype = original;
let gets = 0;
try {
  Object.defineProperty(E, "captureRejections", {
    configurable: true, get() { gets++; return true; }
  });
} catch (error: any) { log(error.name); }
new E();
new E();
log(gets);
}
for (let run = 0; run < 52; run++) {
  printing = run < 2;
  const junk: any[] = [];
  for (let i = 0; i < 20; i++) junk.push({ i, s: "x" + i + run });
  sink = (sink + junk.length) % 9973;
  witness();
}
