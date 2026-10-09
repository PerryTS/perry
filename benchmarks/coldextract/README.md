# Cold extraction probes

These probes execute the supplied upm code unchanged against the real seeded tarball corpus. Run on qb6 in `/root/lanes/perry-coldextract`. They intentionally reference read-only corpora and upm sources in other lanes; no tarballs, seeds or package installations are embedded here.

`setup_lane.py` copies the profiling upm sources, score2 runner/oracles/seed, preserved real-program drivers and timing helpers into the lane. `prepare_corpus.py` checks all 22 supplied archives and prepares extracted-file windows outside timing. Copy `driver.ts` to `micro/driver.ts`. Compile baseline and fixed programs with `compile_lane.py base` and `compile_lane.py fix`, using their corresponding source and release build in `target`; retain separate `bins-base`, `bins-fix` and `target-base`/`target` directories. Rebuild the requested release packages after source edits before compiling either arm. Default auto optimization stays enabled. The tsc-only RS4GC budget increase is documented in the report.

`measure_lane.py KIND AXIS` uses n=5 alternating baseline/fixed/Node runs and independently checks output, package trees, file modes, installed-store fingerprints and lock contents. KIND is `micro`, `programs` or `upm`; AXIS is `inst`, `cycles` or `gc`. `final_gate.sh` shows the affinity, ASLR and measurement-lock recipe. `post_profiles.sh` repeats the syscall and symbolized perf attribution on identical complete corpora. `summarize_lane.py` records medians, ranges and median absolute deviations. `thp_gate.py` selects RSS changes of at least 1 MiB, reruns n=5 with THP disabled, and invokes `smaps_control.py` for separate peak snapshots; snapshot timings are not accepted as performance results.

Micro modes:

| Mode | Code exercised |
|---|---|
| noop | Identical corpus loading and file-window construction |
| write-sync | Original exclusive-create flags/modes, mkdir per shard, writeFileSync |
| write-async | Original createWriter.ensureDir and put methods |
| write-part | Original blocking createWriter.writePart, including file hashes |
| spool | Verbatim private createSpool, two real ≥4 MiB files, FD writes, original writePart adoption and duplicate touch |
| hash | Original createHasher, compressed tarballs in original 1 MiB chunks |
| hash-small | Same data in 16 KiB updates, an overhead diagnostic |
| file-hash | Original hashOf over each extracted file window |
| verify | Original verifyTarball, 1 MiB compressed chunks |
| verdict | Original empty-input verifier, verify twice, 1,000 cases per round |
| parse-integrity | Original parseIntegrity, 1,000 passes per round |
| copies | Original concat followed by Uint8Array.set |
| views | Original file-window subarray and toBase64 |

Writes run one corpus pass. Other timed modes run five. The entire process is counted, including identical loading overhead; noop makes that overhead explicit. File cleanup and disk-content verification happen after the measured process exits. The direct write probes add no fsync, chmod or utimes; upm sets their file permissions on exclusive creation. The spool probe retains the original chmod/rename and duplicate rm/utimes.

Use `PERRY_KEEP_SYMBOLS=1`. Instructions run on CPUs 0–55 with `setarch -R`. Cycles/wall run only on CPUs 56–63 while holding `/root/MEASURE.lock`; do not use off-lock wall values to claim speedups. All full-program outputs must match Node 24.9.0. The repository parity suite separately uses its pinned Node version.

`spool_base.py` extracts the private helper verbatim into a new probe without changing upm. It preserves original-main source in `base-worktree` and compiles the extra baseline against `target-base`; `compile_lane.py` compiles the identical fixed probe. Spool runs five passes, so the first adoption covers chmod/rename and subsequent passes cover rm/utimes. The native-probe subdirectory retains the isolated provider comparison over the initial 21-package subset, 100 passes; final end-to-end digest oracles validate both providers against Node.

Run profiling and THP controls sequentially: their corpus-write probes share a lane-local scratch root. The THP gate waits for the final profile gate. Partial output from a failed gate must be discarded, not appended to a resumed n=5 series. The check_instruction_budget.py ARM command is a performance regression witness over the completed, output-checked corpus: original main fails the wide-digest instruction allowances; fixed Perry and Node pass. It is a probe assertion, not a runtime algorithm-selection rule.
