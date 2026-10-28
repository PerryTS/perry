// A resizable ArrayBuffer that shrinks by 64 KiB or more hands the tail's
// pages back to the host (perry_host.discard_pages) and regrows into them. The
// bytes that stay and the bytes that come back must be what the program expects
// whether the host discarded, declined, or the module has no such import.
const MAX = 1 << 20;
const KEEP = 4096;
const buffer = new ArrayBuffer(MAX, { maxByteLength: MAX });
const view = new Uint8Array(buffer);
let bad = 0;
for (let round = 1; round <= 6; round++) {
  view.fill(round);
  buffer.resize(KEEP);
  buffer.resize(MAX);
  for (let i = 0; i < KEEP; i++) if (view[i] !== round) bad++;
  for (let i = KEEP; i < MAX; i++) if (view[i] !== 0) bad++;
}
console.log(bad === 0 ? "intact" : "bad " + bad, buffer.byteLength);
