import crypto from "node:crypto";
// crypto.getRandomValues (WebCrypto) fills a typed array in place.
console.log("is function:", typeof crypto.getRandomValues === "function");
const a = new Uint8Array(8);
const ret = crypto.getRandomValues(a);
console.log("returns same:", ret === a);
console.log("length:", a.length);
