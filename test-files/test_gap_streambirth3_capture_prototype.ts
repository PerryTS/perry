import { EventEmitter } from "node:events";
function witness() {
const E: any = EventEmitter;
const original = E.prototype;
const key = Object.getOwnPropertySymbols(original)[0];
console.log(String(key), E.captureRejections);
E.captureRejections = true;
console.log(E.captureRejections);
new E();
E.prototype = {};
console.log(E.captureRejections);
new E();
E.captureRejections = true;
console.log(E.captureRejections);
E.prototype = original;
console.log(E.captureRejections);
original[key] = false;
console.log(E.captureRejections);
new E();
E.prototype = Object.create(original);
console.log(E.captureRejections);
E.captureRejections = true;
console.log(E.captureRejections, original[key]);
new E();
E.prototype = null;
try { console.log(E.captureRejections); } catch (error: any) { console.log(error.name); }
try { new E(); console.log("constructed"); } catch (error: any) { console.log(error.name); }
E.prototype = original;
let gets = 0;
try {
  Object.defineProperty(E, "captureRejections", {
    configurable: true, get() { gets++; return true; }
  });
} catch (error: any) { console.log(error.name); }
new E();
new E();
console.log(gets);
}
for (let run = 0; run < 2; run++) witness();
