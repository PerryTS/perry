import { Buffer } from "node:buffer";

// allocUnsafe and allocUnsafeSlow must both return a Buffer of exactly the
// requested length for a size above any typical pool (Node's default pool is
// 8 KiB). The two helpers stay observationally equivalent here — the only
// difference is the pool path, which is invisible from JS.
const big = 9000;
const a = Buffer.allocUnsafe(big);
const b = Buffer.allocUnsafeSlow(big);
console.log("unsafe len:", a.length === big);
console.log("unsafeSlow len:", b.length === big);
console.log("unsafe instance:", a instanceof Uint8Array);
console.log("unsafeSlow instance:", b instanceof Uint8Array);
