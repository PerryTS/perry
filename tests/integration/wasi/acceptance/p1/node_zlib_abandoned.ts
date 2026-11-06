// 300 dictionary-deflate streams are created and abandoned: never written, never
// ended. Each holds roughly 400 KB of compression state, so under the 64 MiB
// linear-memory cap (see the .json) the run only survives if a collection gives
// the abandoned streams' state back. The collection is explicit: the automatic
// trigger for external bytes does not yet reclaim young payloads.
import * as zlib from "node:zlib";
import { collect } from "perry/gc";

const dictionary = Buffer.from("perry runtime dictionary ".repeat(40));
let started = 0;
for (let i = 0; i < 300; i++) {
  const codec = zlib.createDeflate({ dictionary });
  if (!codec) throw new Error("no codec");
  started++;
  if (i % 10 === 9) collect();
}
console.log("started", started);
const check = zlib.inflateSync(zlib.deflateSync(Buffer.from("still works"), { dictionary }), { dictionary });
console.log(check.toString());
