`PERRY_EXTRA_LINK_ARGS` now reaches every final link perry runs, through one reader: the
executable and WASI links as before, and now also the dylib link and the proven tiny-program
link. The tiny-program path used to refuse the variable and fall back to the normal pipeline,
so setting it changed which program was built. Builds that do not set it are unchanged.

The startup A/B harness (`scripts/a1/irdiff.sh`) gains a link-pad control, on by default
(`IRDIFF_LINKPAD=0` turns it off). Rows with equal call counts whose cost follows addresses
(mimalloc page map, address-sorted freed ranges, libc compare alignment) moved whenever one
arm's sections were smaller. `scripts/a1/linkpad.py` links an object of unreferenced,
retained (`SHF_GNU_RETAIN`) `.text`/`.rodata`/`.gcc_except_table`/`.data.rel.ro`/`.data`/`.bss`
input sections last into each arm. It relinks until every paddable section ends at the same
address in both arms, so later sections, segment starts and the heap start coincide. The
readelf proof is written per witness. Sections the runtime walks record by record (`perry_*`,
`.eh_frame`, stack maps) are never padded.
