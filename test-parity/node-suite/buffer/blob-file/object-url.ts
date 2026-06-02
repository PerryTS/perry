import { Blob, resolveObjectURL } from "node:buffer";

// `node:buffer` doesn't expose `URL.createObjectURL` directly — pull
// it off the global URL constructor (it's the same registry).
const b = new Blob(["payload"], { type: "text/plain" });
const url = URL.createObjectURL(b);
console.log("url-prefix:", url.startsWith("blob:"));

const resolved = resolveObjectURL(url);
console.log("resolved-size:", resolved.size);
console.log("resolved-type:", resolved.type);

URL.revokeObjectURL(url);
const after = resolveObjectURL(url);
console.log("after-revoke:", after);
