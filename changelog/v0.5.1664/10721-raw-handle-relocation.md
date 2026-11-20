**The raw-handle debt ledger can now express a pure file move, so a debt-carrying
module can be split for the 2000-line cap.** `raw_handle_debt.py --no-raise-vs`
compares recorded ceilings strictly per path and treats any path absent at the
merge base as a raise from zero. That is right for new debt and wrong for a
relocation — and `scripts/check_file_size.sh` forces relocations regularly.
Splitting a listed module makes the *bare* run demand the emptied source's line be
deleted (rule 3, "ceiling of 4 matches nothing — DELETE its line") and the
destination listed, whereupon `--no-raise-vs` fails with
`vtable_access.rs: ceiling raised to 4 (was not listed at the merge base)` although
the total never moved and the moved bodies are byte-identical. The two required
invocations of one gate disagreed about the same tree. #10565 escaped only by luck:
all four of `object/native_module.rs`'s sites sat in one block, so a different split
carried none — a file whose debt is spread across it could not be split at all
without first paying it down.

A ledger entry may now declare where its debt came from:

```
4 crates/…/object/native_module/vtable_access.rs  # moved-from: crates/…/object/native_module.rs
```

`--no-raise-vs` credits the destination with what the source **actually surrendered
between the merge base and head** (`base ceiling − head ceiling`, floored at zero),
and with nothing else. Monotonicity is preserved on every axis the gate owns: the
total check is untouched so the sum still cannot rise; the credit is bounded by a
real reduction in the same diff, so a relocation cannot launder new sites; two
destinations naming one source drain a shared pool rather than each claiming it
whole; and the annotation goes inert once the move lands, because base and head then
agree and the source surrenders 0 — a stale annotation is a comment, not a standing
permit. What it deliberately does *not* prove is that the moved bodies are the same
bodies: per-path monotonicity becomes total monotonicity plus one declared,
reviewable transfer that names its source in the diff. A text ratchet cannot tell a
move from a rewrite, and the docstring says so rather than implying otherwise.

Two supporting details, both of which would have silently revoked a relocation the
same commit declared: a malformed annotation (`moved_from:`, or any other trailing
comment on an entry) is now a hard parse failure instead of an ignored comment —
otherwise the typo surfaces as "was not listed at the merge base", a diagnostic
naming the destination and never the typo; and `--update`, which rewrites the ledger
wholesale, now carries surviving entries' annotations through (the writer is split
out as `render_ledger` so the round trip can be asserted).

`--self-test` grows twelve cases: the undeclared move is still rejected (so
relocation support is not the per-path rule being deleted), the declared move and a
legal 1+1 three-way split pass, and laundering, an over-draw, a double-spend, a
stale annotation and a self-reference are each rejected by their own diagnostic.
Each anti-laundering case was checked against three plausible *wrong*
implementations — "declared ⇒ allowed", "credit the source's whole base ceiling",
and "correct credit but re-read per destination instead of draining a pool" — and
each is caught, so the cases fail against the feature written badly and not only
against its absence. The laundering case holds the total flat so the total rule
cannot be what fires. No ceiling in `scripts/raw_handle_debt_files.txt` changed
(906, baseline 906); only its header, which documents the new form. (#10583, found
while landing merge train 216)
