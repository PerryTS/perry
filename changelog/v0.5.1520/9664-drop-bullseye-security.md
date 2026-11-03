- **The glibc-2.31 image now builds from `archive.debian.org` only.** Bullseye is
  EOL and Debian is actively retiring it, which broke this image twice in four
  days:

  - run 34197616242 — `E: Release file for .../bullseye-security/InRelease is
    expired (invalid since 14h 44min 50s)`; its Release carried
    `Valid-Until: Mon, 07 Sep 2026 21:13:04 UTC`.
  - run 34272956353 — with `check-valid-until=no` added, the same suite started
    returning **404** for its `.deb`s from some Fastly nodes (IP 151.101.74.132)
    while serving 200 from others. A CDN lottery, not a clean removal.

  There is no archive fallback for it: `archive.debian.org` carries `bullseye`,
  `-backports`, `-proposed-updates` and `-updates`, but **not** `debian-security`.

  So the `bullseye-security` suite is dropped and everything comes from the
  archive. Verified against
  `archive.debian.org/debian/dists/bullseye/main/binary-arm64/Packages`: every
  package this image installs is present — build-essential 12.9, cmake
  3.18.4-2+deb11u1, curl 7.74.0-1.3+deb11u13, gnupg 2.2.27-2+deb11u2, libssl-dev
  1.1.1w-0+deb11u1, libzstd-dev 1.4.8+dfsg-2.1, perl 5.32.1-4+deb11u3, pkg-config
  0.29.2-1, xz-utils 5.2.5-2.1~deb11u1, zlib1g-dev 1.2.11.dfsg-2+deb11u2,
  ca-certificates 20210119.

  The trade-off is explicit: these are archived versions without later security
  patches. That is acceptable for a **build toolchain** image whose only purpose
  is linking against glibc 2.31 — it ships no runtime surface itself — and it is
  the standard configuration for an EOL Debian base.

- **`cargo-test-perry`: `timeout-minutes` 120 → 180.** Shard 8/8 ran **111 min**
  in run 33959469688 and then overran the cap in run 34230915868 — killed at
  exactly 2h00m06s — costing a rerun on an otherwise-green tier. It passed on
  that rerun, so this is headroom, not a hang. 180 keeps a genuine hang well
  under GitHub's 360-min hosted-runner ceiling. That is the fourth cap in this
  campaign sized for a smaller suite (`doc-tests` 119/120, `simctl` 54/60,
  macOS ext build 361/360), which is why a headroom check belongs in CI.
