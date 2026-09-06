A `RegExp` — and every other cell type that owns a metadata edge — can now
answer a descriptor-summary probe from its own header instead of hashing its way
into the descriptor tables.

`set_last_index_throwing` asks `get_property_attrs(re, "lastIndex")` on every
global or sticky `test()`/`exec()`, because a user can make `lastIndex`
non-writable and the spec's `Set(R, "lastIndex", n, true)` must then throw
(test262 `prototype/{exec,test}/y-fail-lastindex-no-write`). #6759 phase C2
added a per-object meta summary precisely so that question could be answered
without touching the tables, but `may_have_descriptor_entry` reached that
summary through `meta_capable_object`, which answers only for `GC_TYPE_OBJECT`.
A `RegExp` is its own cell type, so the filter returned the conservative "maybe"
for every RegExp receiver and the slow probe ran every time: a `String`
allocation for the key, a SipHash of `(usize, String)`, and a map lookup that
was always going to miss.

The capability was already present and merely unwired. #6759 phase 1 unified the
metadata edge behind `cell_meta_slot`, which answers for Object, Error, Map,
Set, RegExp, Promise and Date, and `RegExpHeader::meta` is traced by
`GcLayoutSlotKind::RegExpFields` and moves with its header. The five
descriptor-summary sites now share one predicate built on that edge, so the
change adds no state and no new invariant — it asks the narrower question the
summary actually needs rather than the `ObjectHeader`-shaped one
`meta_capable_object`'s other callers need.

The predicate's answer is deliberately three-way. `None` means the cell type has
no metadata edge and the caller must stay conservative; `Some(null)` means the
edge exists and no record was ever installed, which *proves* the tables hold no
entry for this owner; `Some(meta)` means read the summary words. Collapsing the
first two would turn a conservative *maybe* into a false *no* for the types that
still lack an edge (Temporal, the typed-array views).

Install and probe move together, which is what keeps the fast negative safe: the
installing twin of the predicate is used by `note_meta_descriptor_key`, so an
owner whose install set the key bit is always found, and
`Object.defineProperty(re, "lastIndex", { writable: false })` still makes the
next `test()` throw. `js_regexp_new` writes `meta = null` on every construction,
so a fresh header at a recycled address cannot inherit a dead tenant's bits.

Measured on one 400-character claude-code reply with `PERRY_REGEX_DIAG` armed:
424,035 descriptor-summary probes with a `RegExp` owner, **100.0 % of which the
meta summary now proves absent** — 5.02 per global `test()` call, each one a
`String` allocation, a SipHash and a missing map lookup that no longer happen.
Two diagnostic counters (`desc_regexp_probes`, `desc_regexp_meta_negative`) are
added behind that same environment variable; the second was 0 by construction
before this change, so a single binary measures both arms.
