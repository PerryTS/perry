The collector decoded the entire GC-map section to answer a handful of frame
lookups. For claude-code that is **22.7 MB and 2,078,970 records built into a
~117 MB index** — to answer, on a `--help` run, **74 record lookups**. #9191
deferred that work to the first collection; it did not remove it, and a run that
collects still pays all of it.

The reason it could not be avoided is that the record stream is varint-chained:
reaching function `i`'s records means decoding functions `0..i` first. **v5 adds
one array** — a `u32 stream_offset` per function, after the function table — so a
function's records can be found without touching any other function's. That is
the entire format change. The record stream and the instruction-offset array are
byte for byte what v4 emitted.

The index becomes the function table: one 32-byte entry per function with
records, sorted by address. For claude-code that is 72,669 entries, **2.3 MB**,
built by reading **1.45 MB** of the section — the headers, the function tables
and the new arrays — instead of reading 22.7 MB and writing ~117 MB. A frame's
live set is decoded from the section when a walker asks, and only then.

**Cost: +290,676 bytes of map, +1.3% of the section and +0.10% of the binary.**

## The safety argument, which is the point

This is GC root metadata: a wrong answer frees live objects with no diagnostic.
The argument is not that the two paths agree today.

* **There is one decoder.** `RecordWalk` is the only thing that advances through
  the record stream. The whole-section parser drives it over a blob; the lazy
  lookup drives it over one function. The difference is where the cursor starts
  and nothing else, so "the lazy path decodes differently" is not expressible.
* **The offsets are checked at compile time, exhaustively.** The encoder records
  each function's position as `bytes.len()` at the moment it begins that function
  — it *is* the position — and `verify_roundtrip` proves it against the
  sequential decode for every function of every binary produced. That check
  landed first, against unchanged v4 bytes, where it could not fire.
* **They are checked again at run time.** The eager parser verifies every
  recorded offset against its own walk before it will publish, so gross
  corruption still fails closed at index-build time, as v4 did.
* **`PERRY_GC_STACK_MAP_CROSSCHECK=1` builds both indexes and asserts, for every
  frame the collector walks, that they name the same roots and derived slots.**
  The oracle deliberately uses the same containment rule as the lazy lookup, so
  what it checks is the genuinely new part — that a decode starting at a recorded
  offset gives the same answer as decoding everything.
* **And the cross-check is proven able to fail.** `cross_check_tests` plants the
  exact fault it exists to catch — a per-function stream offset naming another
  function's records, in both directions — and asserts the abort, alongside a
  clean control on the unmodified map. That check is not decoration: a first
  attempt to demonstrate it on a live binary used a small program whose ten
  walked frames turned out to carry no records at all, so a deliberately
  corrupted index passed. A live probe proves the net works only if the frames
  it walks are mapped; the planted fault proves it unconditionally.

Two hazards are handled by construction rather than by observing that
claude-code does not hit them, because the next bundle is not claude-code:

* **Duplicate function addresses.** Two table entries can carry the same
  relocated address — one symbol emitted by several object files, or code the
  linker folded — and each brings its own records. The lookup resolves the whole
  run of equal addresses; taking one would drop the other's roots silently.
* **Zero-record functions** are excluded from the table. One would otherwise sit
  between two real functions and shadow the containing one for every `ip` inside
  it, which is an *empty* live set for a frame that has roots. v4 derived its
  function list from records and excluded them by construction; the lazy table
  has to do it on purpose.

## Two v4 behaviours that change, both narrowing

`chain_walkable` was decided once per image by scanning every root slot in the
section — 4.9M of them — and one function using an exotic base disabled the fast
aarch64 walk everywhere. It is now decided per record, and a frame the x29 chain
cannot resolve falls back to the platform unwinder, reusing the mid-walk bail
this walker already performs for `x19_is_body_sp` and an unvalidated `caller_fp`.

The fast walk's `min_pc`/`max_pc` pre-filter is gone. The function table answers
containment exactly in one binary search, and a pc filter that is even slightly
too narrow drops a real frame's roots — not a trade worth making to save a
compare.

Also stated because it is a real difference: v4 searched every record in the
image for the globally nearest pc and *then* required it to belong to the
function containing `ip`, so a nearer record in a neighbouring function could
suppress a legitimate match. Searching inside the owning function cannot do that.
The old comment recorded that every near-match in the probe suite was already
same-function, so this is the same answer in practice — and where it differs it
returns the roots of the function `ip` is actually executing, which is the
contract the containment check was written to express.

An older-format binary still fails closed: the version byte is checked before
anything else, and the build panics rather than publishing an empty index.
