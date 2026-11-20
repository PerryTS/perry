Loop regions again admit counter-indexed element-only reads from module-level
const arrays. Their element bases are refreshed from the GC-updated global root
after a loop poll while the region is valid, which gc-root-dominance can now
verify through the valid flag. Mutable module bindings remain ineligible.
