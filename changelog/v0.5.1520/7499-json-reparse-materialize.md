### JSON: a lazy tape array now batch-materializes by re-parsing its blob (#7478)

`force_materialize_lazy` built the `ArrayHeader` tree by walking the tape one
element at a time. #7478's decomposition measured that walk at ~2.3× the direct
parser's batch tree build — 56 ms/iter against 24 ms/iter on the 10k-record
fixture — and named the fix: a lazy array only ever stands for a *top-level*
array (`try_parse_via_tape` builds one only for `tape_entries[0].kind ==
KIND_ARR_START`, always with `root_idx = 0`), so its retained `blob_str` is
exactly that array's source text and one `DirectParser::parse_value()` over it
rebuilds the identical tree in a single linear pass. Identical now includes the
numbers: #7483 put the DirectParser's decimal fast path on one correctly-rounded
division, so it agrees bit-for-bit with `str::parse::<f64>` (what
`materialize_number` uses) and with node — the #7477 divergence is what blocked
this the first time it was tried.

The first attempt also SIGSEGV'd intermittently, and the reason is structural:
`DirectParser` is sound only inside a no-move window, three times over. It holds
`input: &'a [u8]` derived from the blob's `StringHeader` payload for the whole
parse with no way to re-derive it; it carries an **unrooted raw-pointer shape
cache** (`hot_shape_keys`, `hot_shape_array`) across every allocation — the
runtime-side-cache-of-a-heap-pointer shape the static root checker cannot see;
and `array_push_parse_fast` fills fresh arrays through
`note_array_slot_layout_only`, which deliberately skips the generational barrier
for young arrays *on the strength of that same suppression*. `js_json_parse` buys
all three with `gc_suppress()`. The reparse now opens a nesting-safe
`GcSuppressScope` — nesting-safe because `force_materialize_lazy` is reachable
from inside `try_stringify_lazy_array`, where the flat `gc_unsuppress()` would
end an outer window early. Around it, every header re-read is a
`RuntimeHandle::across_{mut,const}` paired with the call that can collect (no new
bare `get_raw_*_ptr`; `json_tape.rs` stays at its ceiling of 22), the parsed tree
is handed to `PARSE_ROOTS` before the window closes and promoted to a handle-scope
root before those are restored, and the refreshed header is returned on every
exit including the declining ones.

Because the reparse rebuilds every element from source, the sparse per-element
cache is patched back over the fresh slots: a cached slot holds the JSValue user
code already has a reference to and may have **mutated** through it, while the
blob still says the old value. The patch loop preserves the value and the
identity (`parsed[i] === parsed[i]`), runs inside its own suppression window, and
stores through `store_array_slot` — a raw slot write would leave a reparsed
`RawF64`-layout array flagged pointer-free with a live pointer inside it.

The reparse fires only below the measured crossover (`cached_count * 2 <
cached_length`). Past it the element-wise merge is the cheaper producer — it
copies the cached JSValues and materializes only the remainder — so a reparse
would rebuild subtrees it is about to discard. `bench_field_access` is
deliberately unchanged by this: it touches every element before stringifying, so
its bitmap is full by materialization time and its 2981 ms was always in the 10k
`lazy_get` calls, not the materializer. What benefits today is any full-array
operation on a lazy array whose elements have not been individually walked —
`.map`, `.filter`, spread, `for…of`, `sort`, `Object.keys` on a fresh
`JSON.parse` result — and the random-access `cumulative_walk_steps` trigger,
which fires with a cache count of ~4 on a 10k array.

Validation: `cargo test -p perry-runtime --lib` 1718 passed / 0 failed. A new
sabotage test plants a mutated cached element, materializes through the reparse,
and asserts both the mutation and the object identity survive; the existing
`ForceLazyArrayRooted` copied-minor sabotage test keeps its safepoint and gains
an assertion that it now runs against the reparse producer; two more tests pin
the crossover *decision* and the non-blob-root decline, all witnessed by a
thread-local reparse counter so a test asserts which producer ran rather than
only that nothing threw. Deleting the patch loop turns three tests red. An
11-scenario probe (600 records × 13 fields, the 10k `i * 3.14159` float array
from the #7477 class, edge values, whitespace blobs, identity, sort, repeated
materialization) is byte-identical to node 26.5.1 under `PERRY_JSON_TAPE=1`,
`=0` and auto, and stays byte-identical under `PERRY_GC_ZEAL=1
PERRY_GC_PROTECT_FROMSPACE=1` on a `PERRY_GC_MOVING_LOOP_POLLS=1` build with the
from-space quarantine confirmed live (`retired_set=#0…#5`, a copying minor moving
144,174 objects, 120 reparses inside the protected run).
