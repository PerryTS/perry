// #10601: rules out a "purify the NaN payload to a canonical bit pattern on
// DataView read" fix (the standard purifyNaN/PNaN technique other
// NaN-boxing-adjacent engines use). Node does NOT purify: a crafted NaN's
// exact bits survive a `getFloat64`/`setFloat64` round-trip byte-for-byte
// (V8 doesn't use NaN-boxing for its own tagging -- HeapNumbers are boxed
// behind an ordinary tagged pointer, so an arbitrary double payload is never
// ambiguous with its own internal representation) while `Number.isNaN`
// STILL correctly reports `true`. A Perry fix that canonicalizes the
// payload on read (to make Number.isNaN correct) would therefore diverge
// from Node on this round-trip -- trading the reported bug for a new,
// currently-untested one. Any real fix has to make NaN *classification*
// correct without discarding the payload, which is the same
// registered/valid-id disambiguation problem #10592 solved for `instanceof`
// -- but for every consumer of a NaN-boxed value, not just one.
const dv = new DataView(new ArrayBuffer(8));
dv.setUint32(0, 0x7ffe0000, false);
dv.setUint32(4, 5, false);
const crafted: any = dv.getFloat64(0, false);
console.log("typeof:", typeof crafted, "isNaN:", Number.isNaN(crafted));

const dv2 = new DataView(new ArrayBuffer(8));
dv2.setFloat64(0, crafted, false);
console.log("roundtrip bytes:", dv2.getUint32(0, false).toString(16), dv2.getUint32(4, false).toString(16));
