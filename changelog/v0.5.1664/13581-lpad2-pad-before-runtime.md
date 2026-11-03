`PERRY_LINK_PAD`, a measurement input for startup A/Bs: whitespace-separated objects placed on the link
line right after the compiled objects and before the runtime archive, on every final link perry runs
(executable, dylib, proven tiny program, WASI component), through one reader, `link::link_pad_args()`.
`PERRY_EXTRA_LINK_ARGS` keeps its place (appended last) and meaning. Unset, the link line is unchanged.

The startup A/B harness's link-pad control (`scripts/a1/linkpad.py`, `scripts/a1/irdiff.sh`) now pads in
front of the runtime. The linker lays input sections out in command-line order, so a pad between the
compiled objects and the runtime archive keeps every runtime byte of `.text`, `.rodata` (including the
merged `.rodata.cst*`/`str1.1` blobs), `.eh_frame` (one filler FDE), `.gcc_except_table`, `.data.rel.ro`
(anchored at the end of the RELRO region), `.data` and `.bss` at the same address in both arms; the pad
that is linked last remains only to equalize section ends and the heap start when the runtime itself
differs. The planner relinks the cached objects until the markers coincide; the proof
(`<out>/<witness>.layout.txt`) compares markers, section ends, segment starts, the image end and the
addresses of every runtime symbol both binaries define. Rows that were left by the previous control (ld.so
`check_match` over runtime `.rodata` names, libc compares over runtime literals) now disappear when the
runtime is unchanged. `perry_*` record sections and `.eh_frame_hdr` stay unpadded; both arms must be built
from a perry that has `PERRY_LINK_PAD`.
