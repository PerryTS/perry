- **The glibc-2.31 image now takes `bullseye-security` from a pinned
  `snapshot.debian.org` timestamp.** Bullseye is EOL and Debian is retiring it,
  which broke this image three times in four days:

  - run 34197616242 — `Release file for .../bullseye-security/InRelease is
    expired` (`Valid-Until: Mon, 07 Sep 2026 21:13:04 UTC`).
  - run 34272956353 — with `check-valid-until=no`, the same suite began returning
    **404** for `.deb`s from some Fastly nodes (151.101.74.132) while serving 200
    from others. A CDN lottery.
  - run 34293996179 — dropping the suite entirely then broke apt's resolver,
    because the **pinned base image already carries security versions**:

    ```
    libc6-dev : Depends: libc6 (= 2.31-13+deb11u11) but ...u14 is to be installed
    libssl-dev: Depends: libssl1.1 (= 1.1.1w-0+deb11u1) but ...u8 is to be installed
    perl      : Depends: perl-base (= 5.32.1-4+deb11u3) but ...u5 is to be installed
    ```

  `archive.debian.org` does not carry `debian-security` (404), so the only stable
  source of those exact versions is `snapshot.debian.org` — Debian's timestamped
  time-machine, immutable by design and immune to both expiry and CDN state.

  Verified at `20260901T000000Z`, **both architectures**: `libc6-dev`
  2.31-13+deb11u14 on amd64 and arm64, plus libssl1.1 1.1.1w-0+deb11u8, perl-base
  5.32.1-4+deb11u5, gpgv 2.2.27-2+deb11u3 — exactly what the pinned base image has
  installed.

  The timestamp is part of the reproducibility contract: bump it only alongside a
  base-image digest bump, and re-check those versions when you do.

- **LLVM packages are pinned to `apt.llvm.org`, and apt retries are enabled.**
  Debian's `bullseye-security` genuinely ships LLVM 22 packages (`clang-22`,
  `libpolly-22-dev`, …), so apt preferred snapshot's copies and tried to pull the
  **large** LLVM `.deb`s through snapshot — an archival service, not a throughput
  mirror. It reset the connection (run 34314310247):

  ```
  E: Failed to fetch .../libpolly-22-dev_22.1.8-1~deb11u1_amd64.deb
     Error reading from server. Remote end closed connection
  ```

  An apt preference pinning `origin apt.llvm.org` at 1001 keeps the bulk on the
  fast upstream mirror, leaving snapshot to serve only the four small base
  packages it is actually needed for (libc6, libssl1.1, perl-base, gpgv).
  `Acquire::Retries=5` covers the remaining transient resets.

  Note the dependency resolution itself was already fixed by the snapshot pin —
  this run installed all base packages cleanly and reached the LLVM step, which
  the previous three attempts never did.
