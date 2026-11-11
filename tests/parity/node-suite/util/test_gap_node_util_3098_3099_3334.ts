// Gap test for node:util TextEncoder.encodeInto argument validation (#3098).
// Validates that encodeInto requires a string src and a Uint8Array dest,
// throwing TypeError ERR_INVALID_ARG_TYPE otherwise, while valid calls
// return Node's { read, written } shape.
import util from "node:util";

const { TextEncoder } = util;
const enc = new TextEncoder();

// valid ASCII write into a Uint8Array destination
const u8 = new Uint8Array(8);
const r1 = enc.encodeInto("abc", u8);
console.log(JSON.stringify(r1));
console.log(u8[0], u8[1], u8[2]);

// valid multibyte write (UTF-16 read units differ from bytes written)
const u8b = new Uint8Array(8);
const r2 = enc.encodeInto("aé", u8b);
console.log(JSON.stringify(r2));

// invalid src: undefined -> TypeError ERR_INVALID_ARG_TYPE
try {
  enc.encodeInto(undefined as any, new Uint8Array(8));
  console.log("no throw src");
} catch (e: any) {
  console.log(e.code, e instanceof TypeError);
}

// invalid src: number -> TypeError ERR_INVALID_ARG_TYPE
try {
  enc.encodeInto(123 as any, new Uint8Array(8));
  console.log("no throw num");
} catch (e: any) {
  console.log(e.code, e instanceof TypeError);
}

// invalid dest: ArrayBuffer is not a Uint8Array -> TypeError ERR_INVALID_ARG_TYPE
try {
  enc.encodeInto("abc", new ArrayBuffer(8) as any);
  console.log("no throw dest");
} catch (e: any) {
  console.log(e.code, e instanceof TypeError);
}
