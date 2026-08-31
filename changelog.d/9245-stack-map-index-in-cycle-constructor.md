`GcCycleState`'s constructors now build the lazy stack-map index, replacing five
hand-wired call sites with one that every collection cycle passes through.

The history is the argument for the change. #9191 made the index lazy and wired
the four collection entry points by hand. #9231 then found that budgeted cycles
construct the cycle state directly and bypass all four, so the first root-scan
step hit #9182's fail-closed guard and aborted. #9233 wired those two sites —
by hand again. That is three rounds of the same defect, and `cycle.rs` already
carries the precedent: `arena_growth_full_escalation_due` notes that #7726
wired two sites and missed the third, "which is the site the shipped safepoint
path actually takes."

Doing it in the constructor makes a future entry point correct by construction.
`ensure_built` is an `Acquire` load and a return once the index exists, so the
entries that still call it earlier — while allocation is unambiguously legal —
keep their ordering and pay nothing for the overlap. This carries no behaviour
change of its own: #9233 had already fixed the live abort, confirmed by removing
exactly its two calls and watching both repros abort byte-identically again.
