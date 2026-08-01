### CI: the GC root-dominance gate can now fail, and is baselined honestly

The static root-dominance checker added in #7198 had been **red on `main` ever
since it merged**, and blocked nothing: `gc-root-dominance` is not in branch
protection's required contexts, so the job reported failure without being able
to turn a merge red. That is hazard 2 from CLAUDE.md's "four ways a gate can be
unable to fail", and the corollary it warns about — run a new gate once, *then*
promote it; leaving the second step undone is how hazard 2 happens.

The five violations it had been reporting all along are real, and are now
tracked as #7211: `Expr::ClassExprFresh` roots its class object only when it
believes the static *initializers* can collect, and never asks whether the
lowering's own emitted `js_object_set_field_by_name` can. A class expression
whose statics are inert (`class C { static tag = tag }`) therefore holds the
object in a register across a collection point. `js_object_mark_class` does not
rescue it: that helper roots `CLASS_OBJECT_VALUES`' own copy, which keeps the
object alive and forwarded while leaving the register stale. Reachability is not
the invariant.

**Corpus** (`scripts/gc_root_dominance_corpus.sh`, new) — emission moves out of
the workflow so that reproducing a CI failure is one command rather than a
re-read of the YAML; an invocation retyped without `PERRY_GC_MOVING_LOOP_POLLS=1`
produces IR in which the bug is not expressible at all. Grown from 41 to 117
`.ll` files / 1993 functions / 2501 root stores over 99 sources, selected for the
lowerings this invariant runs through. A stale glob is a hard error and the
compiled-source count has an explicit floor.

**Allowlist** (`scripts/gc_root_dominance_allowlist.json`, new) — one named entry
per known-remaining hit, each with an issue and a written justification, instead
of a numeric threshold. A threshold cannot distinguish a new violation from an
old one, and the cheapest way to green a red build is to raise it by one. The
checker enforces that an entry matching nothing **fails** (so a fix must delete
its entry — that is the ratchet), that an entry suppresses at most its `count`,
and that an unnamed violation fails regardless of the total.

**Proof of failure** — `--seeded-violations N` splices synthetic collection
points into the *real* corpus IR between an allocation and its root store and
requires every one to be reported. `--self-test` only proves the checker fires on
frozen fixtures, which keeps passing even if perry's emitted IR drifts to a shape
the parser can no longer read; that is the case where the gate reports a serene
`violations: 0` over IR it is not analysing. `--self-test` additionally gained
arms covering the allowlist's anti-absorption properties.

**Visibility** — `--min-funcs` plus a `checked N functions / M modules` summary
line, so a silently-empty or silently-shrunken run is impossible to mistake for a
clean one.

Verified: green on `main` with the allowlist; red without it; red with any single
entry removed or its `count` lowered; exit 2 on a stale entry or an empty corpus;
40/40 seeded violations caught. Corpus emission ~80s, the check ~3s.

**Still required, and not something the workflow can do to itself:** a repo admin
must add `gc-root-dominance` to branch protection's required contexts.

### Docs

- `docs/src/internals/gc-rooting-invariant.md` — the rule stated plainly for
  codegen authors, with all five real bugs as case studies, the symptom each
  produces, and how to check your work. Includes the false-green caveat on
  `PERRY_GC_PROTECT_FROMSPACE_DEPTH` (the default of 4 is not enough; use 800).
- `docs/src/internals/rfc-rooting-by-construction.md` — design proposal for
  making the bug unrepresentable: V8's `Handle`/`HandleScope` discipline
  expressed through Rust's borrow checker, so that using an unrooted value
  across a collection point is a compile error. Four of the five real bugs would
  be caught by construction. Proposal only; nothing implemented.
