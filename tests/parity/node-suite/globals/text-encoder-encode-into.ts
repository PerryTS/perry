const enc = new TextEncoder();

console.log("typeof encodeInto:", typeof enc.encodeInto);

const dest = new Uint8Array(8);
dest[6] = 77;
dest[7] = 88;
const result = enc.encodeInto("café", dest);
console.log("result:", JSON.stringify(result));
console.log("bytes:", Array.from(dest).join(","));

const short = new Uint8Array(4);
const shortResult = enc.encodeInto("💩a", short);
console.log("short result:", JSON.stringify(shortResult));
console.log("short bytes:", Array.from(short).join(","));

const boundary = new Uint8Array(4);
const boundaryResult = enc.encodeInto("café", boundary);
console.log("boundary result:", JSON.stringify(boundaryResult));
console.log("boundary bytes:", Array.from(boundary).join(","));

const zero = new Uint8Array(0);
console.log("zero result:", JSON.stringify(enc.encodeInto("abc", zero)));
