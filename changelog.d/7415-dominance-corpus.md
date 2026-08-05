**Fixed** the `GC Root Dominance` gate has been unable to return a verdict since
#7370 made statepoints the default.

The checker's entire vocabulary is `call void @js_shadow_slot_bind(...)`. Under
the stack-map lowering the final IR pass resolves those indices to native allocas
and **removes the calls** (`FunctionCodegen::stack_map_slot_count`), so the corpus
compiled 144 modules containing **zero** root stores. The gate reported
`violations: 0` and then correctly refused to pass, because its `--min-binds`
liveness floor caught that its own subject never ran — CLAUDE.md's fourth hazard,
working as designed.

Both corpus scripts now pin `PERRY_RS4GC=0`. That is sound rather than a dodge:
#7340 split the root-set *analysis* from its lowering, and this gate is about the
analysis, which both backends share. The shadow stack also remains the production
lowering wherever the runtime cannot walk frames.

Measured, same binary and source, only the knob differing:

```
arm=default   js_shadow_slot_bind calls = 0     ← reproduces the CI signature
arm=rs4gc0    js_shadow_slot_bind calls = 9
```

#7370 already fixed the equivalent breakage in the unit tests — `helpers.rs`
records that eight tests broke when the default flipped and were given
`NativeRootsPin::shadow()`. The corpus shell scripts were the same breakage in a
different idiom, and were missed.
