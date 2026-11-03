`test_gap_9592_child_timeout_threads.ts`'s thread-census arm now reports
**skipped** off Linux instead of printing a constant `true` (#10855, second
half; the `/bin/true` portability half shipped in v0.5.1627).

It seeded its own result from the platform:

```ts
let timeoutThreadsReleased = process.platform !== "linux";
```

so off Linux the loop never ran and the line printed `true` unconditionally. A
check that cannot fail on a platform should say "not checked" there, not
"pass" — a green that means *not checked* is precisely the failure mode that
hid this fixture's own `/bin/true` breakage, and #10851's Windows bug, until
someone happened to sweep the area.

The census genuinely cannot run off Linux (`/proc/self/task`), so the arm skips
and says so. The slow-child arm below it is the cross-platform half and does
assert real behaviour everywhere — `killed`, `signalCode === "SIGTERM"` and an
elapsed-time bound — so the fixture still covers #9592's timeout behaviour on
macOS; it simply no longer claims to cover the thread census there.
