Disassembly of build_tape_into in the actual linked workers. Commands use
xcrun llvm-objdump --disassemble-symbols with the symbol stored in summary.json
and --no-show-raw-insn. Worker hashes bind each body to its measured release.

The disassembled span is 5,392 bytes for Unicode, 10,216 for the empty-object
checkpoint and 8,356 for the new ARM mask handling. The latter reduces the span
by 1,860 bytes (18.2%) and removes 465 instruction addresses. This is code-layout
evidence, not proof that instruction-cache effects cause the numeric slowdown.
