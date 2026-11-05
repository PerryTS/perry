// A typed array that owns its bytes hands that storage out when `.buffer` is
// read: `js_typed_array_backing_buffer` materializes a backing `ArrayBuffer`
// and rebinds the array to alias it, so element 0 stops following the header.
//
// Codegen's proven-view tiers read `header + 16 + idx*width` directly, on a
// proof taken at CONSTRUCTION (`new Uint32Array(1)` — a literal length, so
// storage is inline). Nothing revoked that proof when `.buffer` created a
// second view, so the write below landed in the orphaned pre-materialization
// bytes while the byte view read the buffer — and neither direction aliased.
//
// The runtime's own inline reader is guarded by `PERRY_TA_VIEW_GUARD`, which
// `register_view_meta` bumps. These tiers are the compile-time proof that skips
// that check, so the hazard has to be recorded where the alias is created.

// The issue's reproduction. The sum is endian-independent: the four bytes are
// 1, 2, 3 and 4 in either order.
function writeAfterAliasing(): number {
  const words = new Uint32Array(1);
  const bytes = new Uint8Array(words.buffer);
  words[0] = 0x01020304;
  return bytes[0] + bytes[1] + bytes[2] + bytes[3];
}
console.log("write after aliasing:", writeAfterAliasing());

// Writing BEFORE the second view was always correct — the materialization
// copies the current bytes — which is what made this look like a timing quirk
// rather than a lost alias.
function writeBeforeAliasing(): number {
  const words = new Uint32Array(1);
  words[0] = 0x01020304;
  const bytes = new Uint8Array(words.buffer);
  return bytes[0] + bytes[1] + bytes[2] + bytes[3];
}
console.log("write before aliasing:", writeBeforeAliasing());

// The reverse direction is the same bug and was equally broken: bytes written
// through the view must be visible through the original typed array.
function writeThroughTheByteView(): number {
  const words = new Uint32Array(1);
  const bytes = new Uint8Array(words.buffer);
  bytes[0] = 1;
  bytes[1] = 2;
  bytes[2] = 3;
  bytes[3] = 4;
  return words[0];
}
console.log("write through the byte view:", writeThroughTheByteView());

// Buffer identity was already right — only the bytes were not shared, which is
// why this reads as a correctness bug rather than a modelling one.
function bufferIdentity(): string {
  const words = new Uint32Array(2);
  const bytes = new Uint8Array(words.buffer);
  return `${words.buffer === bytes.buffer} ${words.buffer.byteLength} ${bytes.length}`;
}
console.log("buffer identity:", bufferIdentity());

// The ArrayBuffer-FIRST shape (issue #579) was fixed earlier and must stay
// fixed: this one never had an inline-storage proof to lose.
function arrayBufferFirst(): number {
  const buf = new ArrayBuffer(4);
  const words = new Uint32Array(buf);
  const bytes = new Uint8Array(buf);
  words[0] = 0x01020304;
  return bytes[0] + bytes[1] + bytes[2] + bytes[3];
}
console.log("ArrayBuffer first:", arrayBufferFirst());

// An offset view over the materialized buffer sees the same bytes.
function offsetView(): string {
  const words = new Uint32Array(2);
  const tail = new Uint8Array(words.buffer, 4, 4);
  words[1] = 0x01020304;
  return `${tail[0] + tail[1] + tail[2] + tail[3]} ${tail.byteOffset}`;
}
console.log("offset view:", offsetView());

// A typed array whose `.buffer` is NEVER read keeps its inline storage, so the
// fast path it is meant to serve must still produce the right answer. This is
// the direction a too-broad revocation would break silently — by being correct
// but slow — so it is asserted for VALUE here and left to the benchmarks for
// speed.
function neverAliased(): number {
  const values = new Uint32Array(4);
  let total = 0;
  for (let i = 0; i < 4; i++) {
    values[i] = i * 1000 + 7;
  }
  for (let i = 0; i < 4; i++) {
    total += values[i];
  }
  return total;
}
console.log("never aliased:", neverAliased());

// Two independent arrays: reading one's `.buffer` must not disturb the other.
function onlyTheAliasedOne(): string {
  const aliased = new Uint32Array(1);
  const plain = new Uint32Array(1);
  const view = new Uint8Array(aliased.buffer);
  aliased[0] = 0x01020304;
  plain[0] = 42;
  return `${view[0] + view[1] + view[2] + view[3]} ${plain[0]}`;
}
console.log("only the aliased one:", onlyTheAliasedOne());
