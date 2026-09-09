# First-block comparison experiment R4: compiled away

Source `029ec13d2` restores two vector comparisons and OR in the initial 16-byte
opening probe. The wide 64-byte tail remains separate. The matched release
build at `36067866ea15fa771a848643f367eb6a1e16ef25` passed in 5m20s after all 285
JSON release tests passed single-threaded with frozen source hashes. Version is
0.5.1530. Build identity and source stayed fixed through the full tests and
matched build. An initial 153-test subset preceded this full run.

**LLVM normalizes both source forms into identical machine instructions.**
The complete depth scanner is 3676 bytes at the same address as R3; the separate
284-byte search tail is also identical. r3-r4-instruction-proof.json records
instruction hashes, with both disassemblies archived. The proof distinguishes
symbol spans from disassembled instruction bytes: the exported parse entry has
a 156-byte span but 112 explicitly disassembled bytes. It does not infer whole
binary identity. Ordinary string parse/constructor spans remain 976/1972 bytes
and their stack frames remain 96 bytes.

This candidate is parked without a performance run: its intended change to the
first-block machine code did not occur. It cannot establish a fix for R3's
measured sparse/heterogeneous costs. No R4 timing, memory gain or acceptance is
claimed, and the source change is not part of PR #10036.

Compiled correctness passes all fourteen escaped-record comparisons, with twelve
failures in the unfixed-main negative control. The protected scan performs 1323
copying minors and moves 208531 objects; retained outputs survive normal,
scheduled-moving and full-GC runs. The 1000-parse Unicode diagnostic performs 26
minors. Stringify malloc-trigger cadence is 39,40,40,40 in candidate and fresh-main
reference. Source/addr/root/fmt/file-size/store gates pass. Human-audited store
classes are not claimed as machine-verified.

Exact local drivers are historical records with local paths. Logs, hashes,
source patch, compiled output and GC witnesses are archived here. R3's complete
measurements and fresh-main report remain authoritative for performance.
