// Large buffers are allocated, dropped, collected and allocated again. The
// blocks the collector frees are held back, discarded to the host once too many
// pile up, and handed out again, so a block may come back from either. What the
// program writes into each must read back unchanged, and a zero-initialised
// buffer allocated after a collection must read as zero even where it took over
// a block that still held the previous round's data.
declare function gc(): void;

const SIZE = 256 * 1024;
const COUNT = 40;
const STRIDE = 251;

function fill(round: number): Buffer[] {
  const buffers: Buffer[] = [];
  for (let n = 0; n < COUNT; n++) {
    const b = Buffer.alloc(SIZE, 0);
    for (let i = 0; i < SIZE; i += STRIDE) b[i] = (i + round + n) & 0xff;
    b[SIZE - 1] = (round + n) & 0xff;
    buffers.push(b);
  }
  return buffers;
}

function bad(buffers: Buffer[], round: number): number {
  let count = 0;
  for (let n = 0; n < buffers.length; n++) {
    const b = buffers[n];
    for (let i = 0; i < SIZE; i += STRIDE) if (b[i] !== ((i + round + n) & 0xff)) count++;
    if (b[SIZE - 1] !== ((round + n) & 0xff)) count++;
  }
  return count;
}

// The bytes fill() wrote are at these positions; a block that was reused
// without being cleared would show them.
function dirty(zeroed: Uint8Array[]): number {
  let count = 0;
  for (let n = 0; n < zeroed.length; n++) {
    const z = zeroed[n];
    for (let i = 0; i < SIZE; i += STRIDE) if (z[i] !== 0) count++;
    if (z[SIZE - 1] !== 0) count++;
  }
  return count;
}

let wrong = 0;
let zeroed: Uint8Array[] = [];
for (let round = 0; round < 4; round++) {
  let buffers: Buffer[] | null = fill(round);
  wrong += bad(buffers, round);
  gc();
  // Still reachable after a collection that freed the previous round's blocks.
  wrong += bad(buffers, round);
  buffers = null;
  zeroed = [];
  gc();
  // The blocks just freed are held back; zeroed storage is not filled by the
  // program, so it has to come back clean from the allocator.
  for (let n = 0; n < COUNT; n++) zeroed.push(new Uint8Array(SIZE));
  wrong += dirty(zeroed);
}
console.log(wrong === 0 ? "intact" : "bad " + wrong);
