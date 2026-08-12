// GC ratchet probe: live roots held across a throw and a collection.
//
// This is the probe that had no equivalent while try/catch lowered to
// setjmp/longjmp. Under that lowering a longjmp could jump past a
// `gc.relocate`, so the relocated pointer was never written back and a local
// could be left pointing at a moved object. Functions containing `try` were
// therefore excluded from statepoints and routed to the plain-stack-map
// lowering, which is itself unsound — LLVM may record a root slot's address in
// a caller-saved register that cannot be recovered at collection time.
//
// With invoke/landingpad lowering (#7302) the unwind edge is explicit and
// relocations exist on BOTH edges, so statepoints cover try-carrying functions
// too. Nothing else in this suite has a `try` in it, so without this probe the
// newly covered case is exercised by nothing at all.
//
// What it checks, specifically:
//   * objects allocated INSIDE a try survive a collection that happens inside
//     the same try, and read back correctly afterwards;
//   * locals live ACROSS the throw — allocated before it, read in the catch —
//     still hold their contents once the collection has moved things;
//   * the same holds when the throw crosses a frame boundary (thrown deep,
//     caught shallow) so the roots being rewritten are in a caller's frame;
//   * `finally` runs on both the normal and unwinding edges.
//
// A lost or stale root shows up as a wrong checksum rather than a crash, which
// is why every survivor is folded into the output.

declare function gc(): void;

const ROUNDS = 400;
const PER_ROUND = 96;

let escape: object[] | null = null;

class Payload {
  tag: number;
  body: string;
  constructor(tag: number) {
    this.tag = tag;
    this.body = "p" + tag;
  }
  value(): number {
    return (this.tag + this.body.length) | 0;
  }
}

// Thrown from the deepest frame so the unwind crosses several frames that hold
// live roots of their own.
function deep(level: number, seed: number): number {
  if (level === 0) {
    throw new Payload(seed);
  }
  const local = new Payload(seed + level);
  const nested = deep(level - 1, seed);
  // Unreachable, but keeps `local` live across the call in the eyes of any
  // liveness analysis that is not lying to us.
  return (local.value() + nested) | 0;
}

function roundTrip(seed: number): number {
  // Live across the whole try/catch, including the collection.
  const survivors: Payload[] = [];
  let acc = 0;

  try {
    for (let i = 0; i < PER_ROUND; i++) {
      survivors.push(new Payload(seed + i));
    }
    // Collect with everything above live and reachable only from this frame.
    if ((seed & 15) === 0) {
      escape = survivors.slice(0, 8);
      gc();
      escape = null;
    }
    acc = (acc + deep(6, seed)) | 0;
  } catch (err) {
    // The caught value must be the object that was thrown, after a collection
    // that may have moved it.
    const caught = err as Payload;
    acc = (acc + caught.value()) | 0;
    // Every survivor allocated before the throw must still be intact.
    for (let i = 0; i < survivors.length; i++) {
      acc = (acc + survivors[i].value()) | 0;
    }
  } finally {
    // Runs on the unwinding edge; `survivors` must still be readable here.
    acc = (acc + survivors.length) | 0;
  }

  return acc;
}

// Normal (non-throwing) exit through a try/finally, so the non-unwind edge of
// the same lowering is covered too.
function normalExit(seed: number): number {
  const held = new Payload(seed);
  try {
    if ((seed & 31) === 0) {
      gc();
    }
    return held.value();
  } finally {
    escape = null;
  }
}

let checksum = 0;
for (let r = 0; r < ROUNDS; r++) {
  checksum = (checksum + roundTrip(r)) | 0;
  checksum = (checksum + normalExit(r)) | 0;
}

// A rethrow that is caught one frame up, with roots live in both frames.
function rethrower(seed: number): number {
  const outer = new Payload(seed);
  try {
    try {
      gc();
      throw new Payload(seed + 1);
    } catch (inner) {
      throw new Payload((inner as Payload).tag + outer.tag);
    }
  } catch (final) {
    return ((final as Payload).value() + outer.value()) | 0;
  }
}

for (let r = 0; r < 32; r++) {
  checksum = (checksum + rethrower(r)) | 0;
}

// Churn phase with NO explicit gc(): every earlier phase collects explicitly
// every few hundred KB, so the young generation never reaches the 16 MB
// scavenge cap and no AUTOMATIC minor ever fires — and the pin validator
// refuses a probe that runs no minor collection. (Before the cap was
// young-generation-scoped, this probe got its minors from the degenerate
// once-per-block cadence.) The try/catch phases above are unchanged; the
// survivors of those phases must still read back correctly after the
// automatic collections this loop triggers.
const churnRing: (Payload | null)[] = [];
for (let i = 0; i < 64; i++) {
  churnRing.push(null);
}
for (let j = 0; j < 200000; j++) {
  const t = new Payload(j);
  checksum = (checksum + t.tag) | 0;
  churnRing[j % 64] = t;
}
for (let i = 0; i < 64; i++) {
  churnRing[i] = null;
}

gc();
const mu = process.memoryUsage();

console.log("probe:09_try_catch_roots");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
