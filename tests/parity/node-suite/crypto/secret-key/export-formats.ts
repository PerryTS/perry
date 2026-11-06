import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const source = Buffer.from("Hello World");
const key = crypto.createSecretKey(source);
console.log("default export:", key.export().toString());
console.log("empty options export:", key.export({}).toString());
console.log("buffer format export:", key.export({ format: "buffer" }).toString());
console.log("undefined format export:", key.export({ format: undefined }).toString());
const jwk = key.export({ format: "jwk" });
console.log("jwk oct:", jwk.kty, jwk.k);
