import { gzip, gunzip } from "node:zlib";
import { promisify } from "node:util";

const gzipP = promisify(gzip);
const gunzipP = promisify(gunzip);

const input = Buffer.from("promisify roundtrip");
const compressed = await gzipP(input);
const out = (await gunzipP(compressed)).toString();
console.log("promisify equal:", out === "promisify roundtrip");
console.log("promisify out:", out);
