### Performance

- **`str.split(stringSeparator[, limit])` answers before the general algorithm's
  setup (#10519).** With the regex engine linked, a plain string split still
  paid the engine entry's prelude before delegating to the byte splitter: a
  setjmp exception trap, a proxy-table probe, a `ToString` of the separator
  that copied an inline separator such as `"."` to the heap, two more wrapper
  layers with their own handle scopes, and a `Vec` of part ranges per call.
  For `"a.b.c".split(".")` that was ~1,800 of ~3,250 instructions.

  - `crates/perry-runtime/src/string/split.rs` — `split_string_by_string`:
    a heap-string receiver with a string separator (heap or inline) and an
    absent or plain-Number `limit` goes straight to the byte split. Every
    coercion the spec performs on those inputs is the identity or `ToUint32`
    of a Number, and a primitive string has no `@@split`, so nothing else can
    be observed. It declines an empty separator (split by code unit), a
    separator containing a WTF-8 lone surrogate (the byte scan cannot match
    half of a pair), and any other separator, receiver or `limit` type; those
    still take the general path unchanged. Both `js_string_split_js` entries
    (with and without `regex-engine`) try it first.
  - `split_by_delimiter` (the non-empty-delimiter half of `js_string_split_n`,
    now shared) keeps part ranges in a 16-entry stack buffer that spills to a
    `Vec` only past that, and stops scanning at `limit` instead of splitting
    the whole string and truncating.

  Instructions per call (callgrind, `--no-auto-optimize`, control-subtracted,
  the issue's benchmark, Linux x64):

  | variant | before | after | `indexOf`/`slice` control |
  |---|---:|---:|---:|
  | `jwt.split(".")[1]` (179 chars) | 3,865 | 2,022 | 1,237 |
  | `jwt.split(".", 1)[0]` | 3,413 | 1,163 | 709 |
  | `"a.b.c".split(".")` | 3,259 | 1,415 | — |

  `split` is now 1.6× its `indexOf`/`slice` control, inside the issue's 2×
  target. What remains is the work itself: three part allocations, the
  `memchr` scan, the result array and its store barriers.

  Tests: `string::split_fast_tests` (accepted inputs against a `str::split`
  oracle, including more parts than the inline buffer holds; `ToUint32` limit
  forms; every declined shape) and
  `test-files/test_gap_10519_split_string_separator.ts` (byte-identical to
  Node, also under `PERRY_GC_SCHEDULE_SEED` + `PERRY_GC_SCHEDULE_RATE=1` +
  `PERRY_GC_PROTECT_FROMSPACE=1` with 20,025 copying minors).
