**GC: complete the `IMPLICIT_THIS` receiver-rooting conversion started in #8500.**

#8500 converted 22 receiver save/restore pairs that manipulated the `IMPLICIT_THIS` cell directly, rooting the displaced receiver so an evacuating collection inside the callee cannot leave the restore publishing a pre-move address. Four sites of the identical shape were missed because the save spans two lines (`let prev =` on one line, the `IMPLICIT_THIS.with(|c| c.replace(…))` on the next) and so did not match the pattern used to find the others.

No behavioural test moves — these four are latent rather than currently reachable by a failing case — but leaving four instances of a shape that is known to produce a stale `this` is the kind of residue that costs a future session a week. The conversion is now exhaustive: no unrooted direct-access save/restore pair remains in `perry-runtime`.

Does **not** address #8507 (`defineProperty` / `Reflect` own-key paths under forced evacuation), which is a distinct defect — verified by re-running that suite with these four converted, and again with all 135 `js_implicit_this_set(...)`-call pairs converted.
