An untyped `obj[i] = v` store declines from both its typed-array tier and its
Array tier to one shared runtime block, so the admitted-`Uint8Array` byte arm
and the complete `[[Set]]` call are emitted once per site instead of once per
tier. The hit paths are unchanged; the compiled tsc workload drops 0.27 MB.
