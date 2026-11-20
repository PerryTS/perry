- **The glibc-2.31 image no longer fails on expired bullseye metadata.** Debian 11
  is EOL, so nobody refreshes its `Release` files, and apt rejects them once
  `Valid-Until` passes. The security suite's Release carried
  `Valid-Until: Mon, 07 Sep 2026 21:13:04 UTC` and expired mid-release, taking
  down both Linux legs of run 34197616242 with:

  ```
  E: Release file for .../bullseye-security/InRelease is expired
     (invalid since 14h 44min 50s)
  ```

  The packages themselves still serve 200 — only the metadata is stale — so the
  fix is `[check-valid-until=no]` on the security suite, which the `main` line in
  the same file has always had. Deterministic from here on, not a flake: an
  expiry only grows.

- **Removed `prime-macos-x86_64-cache`.** It existed solely to keep the macOS
  x86_64 leg's ext-library step under GitHub's hard 360-min job ceiling, back
  when that step took **304 min**. Building the 40 ext packages in one cargo
  invocation cut it to **12–22 min**, which made the warmer pointless — while it
  still sat on the critical path (`build` depended on it), burning up to 250 min
  to prepare a cache for a sub-20-minute step. Measured at **285 min** in run
  33940039247. Removing it takes roughly 4½ hours off every release.

  Confirmed alongside, in run 34197616242: `Build native ext libraries (Unix)`
  took **18 min** on macOS aarch64, and `Verify ext archives share stdlib's
  tokio` **passed its first real comparison** — the gate fails when it compares
  zero archives, so a pass means it genuinely matched tokio-using ext archives
  against stdlib's.
