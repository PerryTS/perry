"use strict";
// Platform detection for the @perryts/perry launcher.
//
// Split out of bin/perry.js so the resolution rules can be exercised directly
// (see ../test/detect.test.js) with a synthetic host description instead of
// whatever machine happens to run the tests.

const PLATFORM_PACKAGES = {
  "darwin-arm64": "@perryts/perry-darwin-arm64",
  "darwin-x64": "@perryts/perry-darwin-x64",
  "linux-arm64": "@perryts/perry-linux-arm64",
  "linux-arm64-musl": "@perryts/perry-linux-arm64-musl",
  "linux-x64": "@perryts/perry-linux-x64",
  "linux-x64-musl": "@perryts/perry-linux-x64-musl",
  "win32-x64": "@perryts/perry-win32-x64",
};

// Minimum glibc the prebuilt *glibc* Linux binaries can run on.
//
// KEEP IN SYNC WITH THE BUILDER IMAGE. `.github/workflows/release-packages.yml`
// builds `x86_64-unknown-linux-gnu` on `ubuntu-24.04` and
// `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`. Both images ship glibc
// 2.39, so the emitted ELF carries GLIBC_2.39 symbol-version references and the
// dynamic loader refuses to start it on anything older — the process dies with
// `GLIBC_2.39 not found` before Perry's own code runs (#6298).
//
// A binary only requires the glibc version whose symbols it actually pulls in,
// so this is an upper bound: it is the version of the builder image, and the
// safe assumption is that the binary needs all of it. If the release matrix
// moves to a different base image, update this constant in the same commit —
// otherwise hosts that *could* run the glibc build get silently pushed onto the
// static build (too high a value), or hosts that can't get the loader error back
// (too low a value).
const GLIBC_BUILD_FLOOR = "2.39";

// Compare two dotted numeric versions ("2.35", "2.39.1"). Returns <0, 0, >0.
// Non-numeric components sort as 0 — glibc versions are always numeric, and a
// garbage value should not be read as "newer than the floor".
function compareVersions(a, b) {
  const pa = String(a).split(".");
  const pb = String(b).split(".");
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const na = parseInt(pa[i], 10) || 0;
    const nb = parseInt(pb[i], 10) || 0;
    if (na !== nb) return na - nb;
  }
  return 0;
}

// Read the host description this module reasons about. `glibcVersionRuntime` is
// what Node itself uses for the `libc` field of optional deps: a version string
// on glibc, and empty/absent on musl.
function readHost() {
  const host = {
    platform: process.platform,
    arch: process.arch,
    hasGlibcField: false,
    glibcVersionRuntime: undefined,
    osRelease: null,
  };
  try {
    const header = process.report && process.report.getReport().header;
    if (header && "glibcVersionRuntime" in header) {
      host.hasGlibcField = true;
      host.glibcVersionRuntime = header.glibcVersionRuntime;
    }
  } catch (_) {
    /* process.report unavailable — fall back to /etc/os-release below. */
  }
  try {
    host.osRelease = require("fs").readFileSync("/etc/os-release", "utf8");
  } catch (_) {
    /* not Linux, or no os-release — leave null. */
  }
  return host;
}

function isMusl(host) {
  if (host.platform !== "linux") return false;
  // Node reports an empty glibc version on musl. This is the same signal npm
  // uses for the `libc` selector, so it agrees with what npm installed.
  if (host.hasGlibcField) return !host.glibcVersionRuntime;
  // No report header (very old Node, or a hardened runtime): sniff os-release.
  if (host.osRelease) return /\bID=alpine\b|\bmusl\b/i.test(host.osRelease);
  return false;
}

function glibcVersion(host) {
  if (host.platform !== "linux") return null;
  if (!host.hasGlibcField) return null;
  const v = host.glibcVersionRuntime;
  return typeof v === "string" && /^\d+(\.\d+)*$/.test(v) ? v : null;
}

// Resolve a host to the ordered list of platform packages that could serve it.
//
// `candidates[0]` is the preferred package; later entries are tried only if the
// preferred one is not installed. `reason` explains the choice:
//
//   "native"        — the plain os-arch build is the right one
//   "musl"          — musl libc host (Alpine/distroless): the static build
//   "glibc-too-old" — glibc host whose glibc predates the builder image, so the
//                     glibc build's loader would reject it (#6298). The musl
//                     build is fully static, so it runs here.
function detectPlatform(host) {
  const base = `${host.platform}-${host.arch}`;

  if (host.platform !== "linux") {
    return { candidates: [base], reason: "native", glibc: null };
  }

  if (isMusl(host)) {
    // Fall back to the glibc package: some glibc systems (custom kernels, odd
    // container images) report an empty glibcVersionRuntime and land here by
    // mistake — see #116 / v0.5.118. If the musl package really isn't there,
    // the glibc one is the better guess than a hard failure.
    return { candidates: [`${base}-musl`, base], reason: "musl", glibc: null };
  }

  const glibc = glibcVersion(host);
  if (glibc && compareVersions(glibc, GLIBC_BUILD_FLOOR) < 0) {
    // Deliberately no glibc fallback: that binary physically cannot load here.
    return { candidates: [`${base}-musl`], reason: "glibc-too-old", glibc };
  }

  return { candidates: [base], reason: "native", glibc };
}

module.exports = {
  PLATFORM_PACKAGES,
  GLIBC_BUILD_FLOOR,
  compareVersions,
  detectPlatform,
  glibcVersion,
  isMusl,
  readHost,
};
