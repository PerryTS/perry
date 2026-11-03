Raise the optimized machine-pipeline budget on x86-64 targets from 100,000 to
600,000 post-optimization instructions. The budget's fallback demotes the whole
codegen unit to LLVM's O0 machine pipeline, not just the over-budget function
(LLVM selects the pipeline per module, and `optnone` on one function frees its
siblings without bounding regalloc time or memory), so on the OpenCode build
61 over-budget functions dragged 140 MiB of ordinary sibling code into O0
emission with them: 42 % of the binary's text. With the corpus's whole giant
population admitted, the three specimen modules lose 25 % / 42 % / 69 % of
their `.text`, runtime instructions and RSS are unchanged, and compile cost
stays bounded (+13–28 % wall, ~2.5 GB peak on the worst specimen).

aarch64/arm64 and every other unmeasured target keep the old 100,000 ceiling:
the two observations that set it (a 100k-instruction function past ~10 GiB
RSS; a 277k-instruction function >16 min in register allocation) are arm64
and have not been re-measured. `PERRY_LL_FAST_EMIT_MAX_INSTRS` still overrides
on every target; on x86-64 `=100000` reproduces the previous output byte for
byte. The fallback diagnostic now names every over-budget function, widest
first, and says how many functions in its unit are demoted alongside it.
