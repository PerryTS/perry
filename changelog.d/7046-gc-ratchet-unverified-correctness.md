**GC ratchet — unverified correctness is now a gate failure** (follow-up to
#7045): `gc_ratchet.py check` failed a probe whose stdout stopped matching the
Node oracle, but passed one whose correctness could not be established at all
(`unchecked` — Node missing, Node non-zero, or no oracle supplied). That made
"we did not verify" indistinguishable from "we verified and it was fine", which
is the exact hazard the oracle diff exists to close: a probe that silently stops
allocating exits 0 and reports a beautifully small retained heap, and the ratchet
would have read that as a memory improvement. `check` now fails on any status
other than `pass`, `validate_artifact` refuses a baseline pinned from an
unverified run, and the CI measure step resolves the oracle explicitly rather
than searching. The #7045 baseline still validates unchanged — all eight probes
were pinned with `pass` against Node 26.5.0.
