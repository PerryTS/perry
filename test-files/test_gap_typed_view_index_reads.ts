// Indexed element access on the result of `u8.subarray(...)` / `u8.slice(...)`
// must read/write real elements when the receiver is statically typed.
//
// Perry's `subarray`/`slice` on a BufferHeader-backed Uint8Array returns
// another buffer, which the typed-array registry doesn't know — so the
// statically-typed dynamic-index helpers (`r[i]` where `r: Uint8Array` and the
// index isn't provably in-bounds) silently read `undefined` and dropped
// writes. react-server-dom's flight row parser walks exactly these chunk
// views: every RSC row Next.js streamed parsed as garbage (#5989).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

function sum(r: Uint8Array): number {
  let s = 0;
  for (var n = 0, c = r.length; n < c; ) {
    s += r[n++];
  }
  return s;
}

const base = new Uint8Array([1, 2, 3, 4]);
console.log(sum(base), sum(base.subarray(0, 4)), sum(base.subarray(1, 3)));

// slice() results go through the same buffer-backed path
console.log(sum(base.slice(0, 3)));

// writes through the typed path must land
function bump(r: Uint8Array): number {
  let i: any = 0;
  r[i] = 9;
  return r[0];
}
const sub = base.subarray(2);
console.log(bump(sub), sub[0]);

// sequential scan over subarray chunks (the flight-parser read shape — the
// full state-machine regression lives in test_gap_switch_continue_loop.ts)
function scan(r: Uint8Array): string {
  const out: number[] = [];
  for (var n = 0, c = r.length; n < c; ) {
    const d = r[n++];
    if (d === 58) out.push(n);
  }
  return out.join(",");
}
const payload = new TextEncoder().encode("1:hi\n2:jkl\n10:x\n");
console.log(scan(payload));
console.log(scan(payload.subarray(0, payload.length)));
console.log(scan(payload.subarray(5)));
