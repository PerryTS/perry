'use strict';
const bytes = new Uint8Array([72, 101, 108, 108, 111]);
const viaApply = String.fromCharCode.apply(null, bytes as any);
const viaSpread = String.fromCharCode(...bytes);
const min = Math.min.apply(null, new Uint8Array([3, 1, 2]) as any);
if (viaApply !== "Hello" || viaSpread !== "Hello" || min !== 1) {
  console.log(`bad:${viaApply}:${viaSpread}:${min}`);
  process.exit(1);
}
console.log(`ok:${viaApply}:${min}`);
