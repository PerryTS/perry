Arrays and closures derive GC payload selection from their headers. Mixed payloads use the existing tag scan instead of creating per-object slot masks; ordinary objects continue to use their shape representation. Stores no longer restore pointer-free status by clearing a last mask bit; explicit bulk re-derivation can still restore it. Numeric and raw-f64 sub-flags remain supported.

Delete the obsolete typed-layout bit 12 and its clears. Bit 12 now belongs solely to the raw-f64-or-holes proof, so pointer-free layout initialization, rebuild and relocation preserve that proof.
