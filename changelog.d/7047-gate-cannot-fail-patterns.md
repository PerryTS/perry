Documented the four distinct ways a CI gate can be structurally unable to fail,
in `CLAUDE.md`. All four look green-adjacent on the Actions page and none can turn
a merge red: `continue-on-error: true`; absence from branch protection's required
contexts; `concurrency` with unconditional `cancel-in-progress` on a slow-queue
branch; and the subtlest — the gate runs and passes while its subject never
actually executed.

Each has bitten this repo, three of them inside one week. The fourth is the
dangerous one because the job is genuinely green: `PERRY_GC_FORCE_EVACUATE` was
inert for every `gc()`-driven test, and the GC matrix's `--pressure` knob disabled
the very path it was measuring. The rule that follows is that a gate must assert
its subject was live, not merely that nothing threw.
