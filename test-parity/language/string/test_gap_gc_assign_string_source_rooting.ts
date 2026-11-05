// #7214: `Object.assign(t, str)` / `{ ...str }` decoded the SOURCE string once,
// into a raw `(*const u8, len)` view, and then held that borrow across every
// allocation in the copy loop.
//
// `str_bytes_from_jsvalue` returns a pointer INTO the source `StringHeader`'s
// data region for any string past the SSO limit, and says so in its own safety
// note: "Callers must not hold this pointer past a subsequent `scratch`
// modification or a GC cycle that could sweep the heap-backed `StringHeader`."
// `object_assign_string_source` built a `&str` on that pointer and then hit
// THREE allocation points per character — two `js_string_from_bytes` calls and
// the write funnel's key interning / keys-array growth.
//
// #7207 opened a `RuntimeHandleScope` in that very function and rooted the
// target, the key and the value. It did not root the source, because the source
// is not a JSValue in that frame at all — it is a borrow. So an evacuating
// minor moved the string and `chars()` walked from-space for every remaining
// character.
//
// LIVE BY CONSTRUCTION AND ONLY ON THE MOVING ARMS. The source is a FRESH heap
// string per iteration (built with `repeat`, well past the SSO limit), reachable
// only from a shadow-bound local — so it survives the minor, which means it
// MOVES. A non-moving collection leaves the bytes where they are and the borrow
// stays accidentally valid.

const ALPHA = "abcdefghijklmnopqrstuvwxyz";
const WIDTH = 208;

function run(): string {
  let badChar = 0;
  let badCount = 0;
  for (let r = 0; r < 300; r++) {
    // Past SHORT_STRING_MAX_LEN, so this is a real heap `StringHeader` in the
    // nursery rather than an inline short string copied into the caller's
    // scratch buffer.
    const src: string = ALPHA.repeat(8);
    const out: any = Object.assign({}, src);
    let seen = 0;
    for (let i = 0; i < WIDTH; i++) {
      const got = out[i];
      if (got !== undefined) {
        seen++;
      }
      if (got !== ALPHA[i % 26]) {
        badChar++;
        break;
      }
    }
    if (seen !== WIDTH) {
      badCount++;
    }
  }
  return "char " + badChar + " count " + badCount;
}

console.log("bad", run());
