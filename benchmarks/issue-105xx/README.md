These are the self-contained reproductions from #10519, #10525, #10526 and
#10528. They exercise a runtime-generated serializer, literal string splits,
TCP corking and deferred request/response writes. The harness verifies every
checksum and serializer sample against Node before recording median loop times.
Use the pinned Node from `.node-version` and separately built artifacts for each
source revision. Runtime archives must come from the static wrapper crates.

```sh
python3 benchmarks/issue-105xx/run.py \
  --perry /path/to/build/perry --runtime-dir /path/to/build \
  --output /tmp/issue-105xx-results.json
```

Auto-optimization and the compiler cache are disabled for both arms so the
interpreter and regex-engine feature sets stay comparable. Timings from a local
development profile support before/after comparisons; they do not establish the
Linux release-profile instruction or syscall targets in the issues. On Linux,
run the resulting socket reproductions under `strace -c` to inspect batching.
