// #3720: node:http2 exposes the module-level `performServerHandshake`
// helper alongside the server factories. Lock in Node's observable
// export shape (callable, name, length) so it can't silently regress.
import * as http2 from "node:http2";

console.log("typeof:", typeof http2.performServerHandshake);
console.log("name:", http2.performServerHandshake.name);
console.log("length:", http2.performServerHandshake.length);
