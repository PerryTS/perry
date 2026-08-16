Fixed the GC ratchet's own test suite demanding a selective re-pin receipt from
every pinned baseline, which made `windows-build` red on every open PR.

`accepted_deterministic_deltas` is the receipt for a *selective* re-pin — the
dangerous kind, which can turn one red row green while leaving no
machine-readable answer to which rows moved or why. A *full* re-pin carries
artifact-wide provenance instead, and the validator says so explicitly
(`if receipt is None: return`). #8204 moved 130 of 168 cells, so it correctly
shipped no receipt; three tests that hard-subscripted the key on the live
pinned baseline errored with `KeyError`. The gate punished the correct action.

Those tests had frozen one historical selective re-pin — #8069's exact 21 cells
and causes — into assertions against whatever baseline happens to be current,
which could only stay green by the world never changing.

The two tamper tests remain, but build their fixture synthetically from the pin
rather than assuming the pinned artifact carries a receipt: a fixture taken
from the artifact under test cannot independently test it. The structural
invariant survives — a receipt, if present, must name real probes/metrics,
agree with the pinned medians, and reference declared causes. And the contract
#8204 exercised is now a test rather than a docstring: a full re-pin with no
receipt is valid, so the next full re-pin will not red the gate again.

Sabotage-tested: removing the pinned-median comparison, accepting any
timestamp, or making a missing receipt a defect each fails the corresponding
test. Test-only; no runtime, codegen or baseline changes.
