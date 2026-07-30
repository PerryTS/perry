// GC ratchet probe: heap strings.
//
// Heap strings are their own allocation family (STRING_TAG, separate scanner)
// and their bytes are not required to be valid UTF-8. This probe builds and
// discards a large volume of concatenated and sliced strings through a ring so
// they genuinely reach the heap, keeping only a small digest alive. Post-collect
// retention therefore reflects string reclamation rather than object
// reclamation.

declare function gc(): void;

const ROUNDS = 3000;
const PIECES = 96;
const RING = 64;

const ring: (string | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

function buildStrings(round: number): number {
  let acc = "";
  let width = 0;
  for (let i = 0; i < PIECES; i++) {
    acc = acc + "seg" + ((round + i) & 255) + "|";
    ring[i % RING] = acc;
    if (acc.length > 512) {
      const tail = acc.slice(acc.length - 128);
      width = (width + tail.length) | 0;
      acc = tail;
    }
  }
  return (width + acc.length) | 0;
}

let checksum = 0;
for (let r = 0; r < ROUNDS; r++) {
  checksum = (checksum + buildStrings(r)) | 0;
}

for (let i = 0; i < RING; i++) {
  ring[i] = null;
}

gc();
const mu = process.memoryUsage();

console.log("probe:06_string_retention");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
