Fixed a compiler nontermination in recursive structural type aliases whose
bodies contain several references back to the same alias. Alias resolution now
cuts active cycles and has a deterministic per-resolution expansion budget, so
the allocation-point GC fixture compiles and runs unchanged.
