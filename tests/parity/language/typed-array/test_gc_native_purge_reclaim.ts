// Native allocator maintenance must preserve rooted owners across reclamation.
// Node oracle: node --expose-gc --experimental-strip-types <file>.
declare function gc(): void;

const live: { id: number; bytes: Uint8Array; tags: number[] }[] = [];
for (let i = 0; i < 200; i++) {
  const bytes = new Uint8Array(8192);
  bytes[0] = i & 255;
  bytes[8191] = (i + 17) & 255;
  live.push({ id: i, bytes, tags: [i, i + 1] });
}

function burst(round: number): number {
  const garbage: { id: number; bytes: Uint8Array }[] = [];
  let checksum = 0;
  for (let i = 0; i < 200; i++) {
    const bytes = new Uint8Array(65536);
    bytes[0] = (round + i) & 255;
    bytes[65535] = (round + i + 1) & 255;
    garbage.push({ id: i, bytes });
    checksum += bytes[0] + bytes[65535];
  }
  return checksum + garbage.length;
}

let sum = 0;
for (let round = 0; round < 12; round++) {
  sum += burst(round);
  gc();
  for (let i = 0; i < live.length; i++) {
    const item = live[i];
    if (item.id !== i || item.bytes[0] !== (i & 255)
        || item.bytes[8191] !== ((i + 17) & 255)
        || item.tags[1] !== i + 1) throw new Error("root lost across reclaim");
    sum += item.id + item.bytes[0] + item.bytes[8191] + item.tags[1];
  }
}
gc();
console.log("reclaim", live.length, sum);
