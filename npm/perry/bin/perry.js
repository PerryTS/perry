#!/usr/bin/env node
// Wrapper that resolves the platform-specific @perryts/perry-* binary and
// execs it with stdio inherited + argv passed through. Keeps the installed
// @perryts/perry package tiny (this script) while the native bits live in
// optional-dependency packages that npm picks by os/cpu/libc.

const { spawn } = require("child_process");
const {
  PLATFORM_PACKAGES,
  GLIBC_BUILD_FLOOR,
  detectPlatform,
  readHost,
} = require("./detect.cjs");

const host = readHost();
const detected = detectPlatform(host);
const binName = process.platform === "win32" ? "perry.exe" : "perry";

// Walk the candidate packages in preference order. On Linux the first entry may
// be the fully-static musl build (musl host, or a glibc host older than the one
// the glibc binaries were built on — #6298); everywhere else there is exactly
// one candidate and this loop is the old single require.resolve.
let binPath = null;
let usedKey = null;
const resolveErrors = [];
for (const key of detected.candidates) {
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) continue;
  try {
    binPath = require.resolve(`${pkg}/bin/${binName}`);
    usedKey = key;
    break;
  } catch (err) {
    resolveErrors.push(`${pkg}: ${err.message}`);
  }
}

if (!binPath) {
  reportResolutionFailure();
  process.exit(1);
}

// Tell the user once — not on every invocation — when they are not running the
// binary their platform string implies.
if (detected.reason === "glibc-too-old" && usedKey.endsWith("-musl")) {
  noticeOnce(
    `[perry] glibc ${detected.glibc} is older than the prebuilt glibc binary requires ` +
      `(>= ${GLIBC_BUILD_FLOOR}), so Perry is running its fully-static Linux build ` +
      `(${PLATFORM_PACKAGES[usedKey]}).\n` +
      `[perry] Same compiler; the static build cannot produce perry/ui (GTK4) apps. ` +
      `Set PERRY_NO_FALLBACK_NOTICE=1 to silence this. Details: https://github.com/PerryTS/perry/issues/6298`
  );
}

const child = spawn(binPath, process.argv.slice(2), { stdio: "inherit" });

// Forward termination signals so Ctrl-C / supervisor kills propagate to perry.
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT"]) {
  process.on(sig, () => {
    try {
      child.kill(sig);
    } catch (_) {}
  });
}

child.on("close", (code, signal) => {
  if (signal) {
    // Re-raise so the parent shell sees the signal exit, not a plain 1.
    process.kill(process.pid, signal);
  } else {
    process.exit(code == null ? 0 : code);
  }
});

child.on("error", (err) => {
  console.error(`[perry] Failed to spawn ${binPath}: ${err.message}`);
  process.exit(1);
});

// --- helpers ---------------------------------------------------------------

// Print `msg` at most once per (version, platform key) on this machine. The
// stamp lives in the temp dir, so it comes back after a reboot/cleanup — that's
// deliberate: the message is worth seeing again occasionally, just not on every
// single `perry` invocation in a build loop.
function noticeOnce(msg) {
  if (process.env.PERRY_NO_FALLBACK_NOTICE) return;
  const fs = require("fs");
  const path = require("path");
  const os = require("os");
  let version = "unknown";
  try {
    version = require("../package.json").version;
  } catch (_) {}
  const stamp = path.join(
    os.tmpdir(),
    `perry-static-fallback-${version}-${usedKey}.stamp`
  );
  try {
    // "wx" fails with EEXIST if we already printed this.
    fs.closeSync(fs.openSync(stamp, "wx"));
  } catch (err) {
    if (err && err.code === "EEXIST") return;
    // Any other stamp problem (read-only tmp, etc.) — print, don't crash.
  }
  console.error(msg);
}

function reportResolutionFailure() {
  const key = detected.candidates[0];
  const pkg = PLATFORM_PACKAGES[key];

  if (!pkg) {
    console.error(
      `[perry] No prebuilt binary for ${key}.\n` +
        `Supported: ${Object.keys(PLATFORM_PACKAGES).join(", ")}\n` +
        `File an issue: https://github.com/PerryTS/perry/issues`
    );
    return;
  }

  let version = "";
  try {
    version = `@${require("../package.json").version}`;
  } catch (_) {}

  if (detected.reason === "glibc-too-old") {
    // npm skipped the musl package because this host is glibc — its `libc`
    // selector says "musl". Without --force npm refuses to install it here, so
    // spell the command out rather than leaving the user with a loader error.
    console.error(
      `[perry] This system has glibc ${detected.glibc}, but Perry's prebuilt Linux binary\n` +
        `        needs glibc >= ${GLIBC_BUILD_FLOOR} (it is built on ubuntu-24.04). Running it would fail\n` +
        `        in the dynamic loader with "GLIBC_${GLIBC_BUILD_FLOOR} not found".\n` +
        `\n` +
        `        Perry also ships a fully-static Linux build that needs no glibc at all, but\n` +
        `        npm skipped it here because that package is tagged libc: ["musl"]. Install it\n` +
        `        the same way you installed perry:\n` +
        `\n` +
        `          if perry is GLOBAL (npm i -g @perryts/perry):\n` +
        `            npm install -g --force ${pkg}${version}\n` +
        `\n` +
        `          if perry is a PROJECT dependency:\n` +
        `            npm install --force ${pkg}${version}\n` +
        `\n` +
        `        ...then re-run perry — this launcher picks it up automatically.\n` +
        `\n` +
        `        Or install outside npm (this picks the static build for you):\n` +
        `            curl -fsSL https://perryts.com/install.sh | sh\n` +
        `\n` +
        `        Tracking: https://github.com/PerryTS/perry/issues/6298`
    );
    return;
  }

  console.error(
    `[perry] The ${pkg} package is not installed.\n` +
      `This usually means npm skipped the optional dependency for ${key}.\n` +
      `Install it the same way you installed perry:\n` +
      `  global:  npm install -g --force ${pkg}${version}\n` +
      `  project: npm install --force ${pkg}${version}\n` +
      `Or reinstall @perryts/perry with a matching npm (≥8.12) so os/cpu/libc selectors apply.\n` +
      `Underlying error: ${resolveErrors.join("; ")}`
  );
}
