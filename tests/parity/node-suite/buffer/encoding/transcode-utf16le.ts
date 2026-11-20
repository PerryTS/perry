import { transcode, Buffer } from "node:buffer";

// Canonical Deno node-compat case: "Hi" as UTF-16LE → UTF-8.
const hi = transcode(Buffer.from("48006900", "hex"), "utf16le", "utf8");
console.log("hi:", hi.toString());
console.log("hi len:", hi.length);

// Round-trip the other way: UTF-8 → UTF-16LE → UTF-8.
const back = transcode(transcode(Buffer.from("Hi"), "utf8", "utf16le"), "utf16le", "utf8");
console.log("round-trip:", back.toString());

// ucs2 alias maps to the same UTF-16LE pair semantics as Node.
const ucs2 = transcode(Buffer.from("48006900", "hex"), "ucs2", "utf8");
console.log("ucs2:", ucs2.toString());
