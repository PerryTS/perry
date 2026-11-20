### GC env knobs: `PERRY_GC_DIAG=0` no longer ENABLES diagnostics (#7991)

`gc_diag_enabled()` read its knob with `var_os(..).is_some()` — **presence,
not value** — so `PERRY_GC_DIAG=0` turned diagnostics on, and so did `off`,
`false` and the empty string.

That is a measurement-integrity bug rather than a cosmetic one. During #7803
triage it silently collapsed an A/B arm: the investigator disabled
diagnostics for the clean arm and got them in *both*, so the arms were no
longer different in the way intended. It fails toward a **confident wrong
answer**, not a visible error. It was also inconsistent with its immediate
neighbour — `PERRY_GC_PROTECT_FROMSPACE` has parsed its value properly all
along, so `=0` really was off there.

**The audit found the same shape on 25 read sites across four knobs.**
`PERRY_GC_DIAG` (20 sites), `PERRY_GC_VERIFY_MARK` (3 — `=0` armed a
whole-heap verifier), `PERRY_GC_VERIFY_RS_NONFATAL` (1), and
`PERRY_GC_VERIFY_EVACUATION`, which was *split-brain*: value-parsed in
`gc/mod.rs`, presence-parsed in the barrier's ever-dirty tracker, so `=0`
switched the verifier off while leaving its side table populated on every
barrier. One adjacent find outside the GC family: `PERRY_SHAPE_LAYOUT_KEYED`
was `v != "0"`, so its documented off-state worked only for the literal `0`
— `=off` and `=false` read as ON.

**There are now exactly two boolean vocabularies**, both pure functions of
the raw value so both directions are testable without touching the process
environment:

* `gc::env_flag_from_value` — default-OFF (#5093): `1`/`true`/`on`/`yes`;
  unset, the off-spellings, empty and anything **unrecognised** read OFF.
* `gc::env_default_on_from_value` — default-ON kill switch: OFF only on an
  explicit `0`/`off`/`false`/`no`; unrecognised leaves the shipping default
  ON.

They are deliberately **not** each other's negation — each fails toward its
own documented default — and that asymmetry has its own assertion so a future
tidy-up cannot collapse one into the other. Also unified onto them:
`PERRY_GC_TRACE`, `PERRY_GC_VERIFY_CLASSIFIER`, `PERRY_GC_FORCE_EVACUATE`,
`PERRY_GEN_GC`, `PERRY_WRITE_BARRIERS`, `PERRY_GC_MOVING_SAFEPOINT`,
`PERRY_GC_INCREMENTAL`, `PERRY_SHAPE_LAYOUT_KEYED`, and
`PERRY_GC_SAFEPOINT_ONLY`'s boolean arm (`strict` stays its own third state).

**Teeth, because this is precisely the class where a doc comment is not a
change.** `gc/tests/env_knob_parse.rs` pins both vocabularies over on / off /
unrecognised spellings — but those pure cases would *all stay green* if that
one line reverted to presence-parsing, so the decisive case observes the
**live cached reader in a child process** under a real `PERRY_GC_DIAG=0` /
`off` / `` / `1`. The ON arm is there so a fix that hard-wires `false` cannot
pass either. Separately, `scripts/check_gc_env_knobs.py` (already in `lint`)
now rejects the presence-only shape outright for the GC family; its
exemption list is empty, a **stale** entry also fails so a fix must delete
its own licence, and its `--self-test` sabotages the detector with the exact
shape that shipped and requires it to be told apart from the replacement.

Sabotage-verified: with the fix committed, `telemetry.rs` was reverted in
place and both teeth fired (`PERRY_GC_DIAG=Some("0") must read as OFF`; and
the lint gate naming the file), then restored **and rebuilt** to re-confirm
green.

Diagnostic-only by contract, so no program semantics change; every in-repo
use of these knobs is `=1`. The damage was to investigations — any prior A/B
that used `PERRY_GC_DIAG=0` as its control arm was not controlled.
