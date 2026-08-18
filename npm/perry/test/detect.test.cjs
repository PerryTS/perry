"use strict";
// Self-test for the @perryts/perry launcher's platform resolution.
//
//   node npm/perry/test/detect.test.cjs
//
// No dependencies, no network, no installed platform packages — it feeds
// synthetic host descriptions (the shape of `process.report.getReport().header`
// on each platform) through detectPlatform() and checks where they land.
//
// It also cross-checks the launcher against the things it has to agree with:
//   * every package it can name is a real optionalDependency of @perryts/perry
//   * the release matrix really does build the musl targets it falls back to
//   * the glibc targets are still built in the old-sysroot image the floor assumes

const assert = require("assert");
const fs = require("fs");
const path = require("path");
const {
  PLATFORM_PACKAGES,
  GLIBC_BUILD_FLOOR,
  compareVersions,
  detectPlatform,
} = require("../bin/detect.cjs");

const REPO_ROOT = path.resolve(__dirname, "..", "..", "..");
let failures = 0;

function check(name, fn) {
  try {
    fn();
    console.log(`  ok   ${name}`);
  } catch (err) {
    failures++;
    console.log(`  FAIL ${name}\n       ${err.message}`);
  }
}

// A glibc host: Node reports the runtime glibc version in the report header.
const glibcHost = (arch, version) => ({
  platform: "linux",
  arch,
  hasGlibcField: true,
  glibcVersionRuntime: version,
  osRelease: "ID=ubuntu\n",
});

// A musl host: the field is present but empty. This is exactly what npm keys
// its `libc` selector off.
const muslHost = (arch) => ({
  platform: "linux",
  arch,
  hasGlibcField: true,
  glibcVersionRuntime: "",
  osRelease: "ID=alpine\n",
});

console.log("\nglibc / musl routing (linux-x64, floor = " + GLIBC_BUILD_FLOOR + ")");
console.log("  glibc            → package                          reason");
console.log("  ---------------------------------------------------------------");
const table = [
  ["2.27 (older)", glibcHost("x64", "2.27"), "linux-x64-musl", "glibc-too-old"],
  ["2.28 (RHEL 8)", glibcHost("x64", "2.28"), "linux-x64-musl", "glibc-too-old"],
  ["2.30 (older)", glibcHost("x64", "2.30"), "linux-x64-musl", "glibc-too-old"],
  ["2.31 (build floor / Ubuntu 20.04)", glibcHost("x64", "2.31"), "linux-x64", "native"],
  ["2.34 (RHEL 9 / AL2023)", glibcHost("x64", "2.34"), "linux-x64", "native"],
  ["2.35 (Ubuntu 22.04)", glibcHost("x64", "2.35"), "linux-x64", "native"],
  ["2.36 (Debian 12)", glibcHost("x64", "2.36"), "linux-x64", "native"],
  ["2.41 (newer)", glibcHost("x64", "2.41"), "linux-x64", "native"],
  ["(musl / Alpine)", muslHost("x64"), "linux-x64-musl", "musl"],
];
for (const [label, host, wantKey, wantReason] of table) {
  const got = detectPlatform(host);
  console.log(
    `  ${label.padEnd(24)} ${got.candidates[0].padEnd(20)} ${got.reason}`
  );
  check(`${label} → ${wantKey} (${wantReason})`, () => {
    assert.strictEqual(got.candidates[0], wantKey);
    assert.strictEqual(got.reason, wantReason);
  });
}

console.log("\nother platforms");
check("linux-arm64 glibc 2.30 → linux-arm64-musl", () => {
  const got = detectPlatform(glibcHost("arm64", "2.30"));
  assert.strictEqual(got.candidates[0], "linux-arm64-musl");
  assert.strictEqual(got.reason, "glibc-too-old");
});
check("linux-arm64 glibc 2.31 → linux-arm64", () => {
  assert.strictEqual(detectPlatform(glibcHost("arm64", "2.31")).candidates[0], "linux-arm64");
});
check("linux-arm64 musl → linux-arm64-musl", () => {
  assert.strictEqual(detectPlatform(muslHost("arm64")).candidates[0], "linux-arm64-musl");
});
check("darwin-arm64 unaffected", () => {
  const got = detectPlatform({ platform: "darwin", arch: "arm64", hasGlibcField: false });
  assert.deepStrictEqual(got.candidates, ["darwin-arm64"]);
  assert.strictEqual(got.reason, "native");
});
check("win32-x64 unaffected", () => {
  const got = detectPlatform({ platform: "win32", arch: "x64", hasGlibcField: false });
  assert.deepStrictEqual(got.candidates, ["win32-x64"]);
});
check("win32-arm64 selects its native package", () => {
  const got = detectPlatform({ platform: "win32", arch: "arm64", hasGlibcField: false });
  assert.deepStrictEqual(got.candidates, ["win32-arm64"]);
});

console.log("\nhosts that don't report a glibc version");
check("no report header + alpine os-release → musl", () => {
  const got = detectPlatform({
    platform: "linux",
    arch: "x64",
    hasGlibcField: false,
    osRelease: 'ID=alpine\nNAME="Alpine Linux"\n',
  });
  assert.strictEqual(got.candidates[0], "linux-x64-musl");
  assert.strictEqual(got.reason, "musl");
});
check("no report header + glibc os-release → linux-x64 (old behaviour kept)", () => {
  const got = detectPlatform({
    platform: "linux",
    arch: "x64",
    hasGlibcField: false,
    osRelease: 'ID=ubuntu\nVERSION_ID="22.04"\n',
  });
  // Without a version we cannot know the binary won't load; guessing musl here
  // would push every unknown host onto the static build. Stay on the default.
  assert.deepStrictEqual(got.candidates, ["linux-x64"]);
  assert.strictEqual(got.reason, "native");
});
check("empty glibcVersionRuntime (musl) falls back to the glibc pkg — #116", () => {
  // Some glibc systems report an empty version (custom kernels / odd images).
  // They get the musl package first, but must still be able to land on the
  // glibc one if that's what npm actually installed.
  const got = detectPlatform(muslHost("x64"));
  assert.deepStrictEqual(got.candidates, ["linux-x64-musl", "linux-x64"]);
});
check("glibc-too-old does NOT fall back to the glibc pkg", () => {
  // That binary physically cannot load — a fallback would just resurrect the
  // Avoid resurrecting the dynamic-loader error on pre-2.31 systems.
  const got = detectPlatform(glibcHost("x64", "2.30"));
  assert.deepStrictEqual(got.candidates, ["linux-x64-musl"]);
});

console.log("\nversion comparison");
check("compareVersions is numeric, not lexical", () => {
  assert.ok(compareVersions("2.30", "2.31") < 0);
  assert.ok(compareVersions("2.31", "2.31") === 0);
  assert.ok(compareVersions("2.32", "2.31") > 0);
  assert.ok(compareVersions("2.9", "2.31") < 0, "2.9 must sort below 2.31");
  assert.ok(compareVersions("3.0", "2.31") > 0);
  assert.ok(compareVersions("2.31.1", "2.31") > 0);
});
check("a garbage glibc string is not treated as 'newer than the floor'", () => {
  const got = detectPlatform({
    platform: "linux",
    arch: "x64",
    hasGlibcField: true,
    glibcVersionRuntime: "not-a-version",
    osRelease: "ID=ubuntu\n",
  });
  // Unparseable → we don't know → keep the default package (status quo), never
  // silently claim it satisfies the floor by string comparison.
  assert.deepStrictEqual(got.candidates, ["linux-x64"]);
});

console.log("\nagreement with what actually ships");
check("every platform package is an optionalDependency of @perryts/perry", () => {
  const tmpl = fs.readFileSync(
    path.join(REPO_ROOT, "npm", "perry", "package.json.tmpl"),
    "utf8"
  );
  const manifest = JSON.parse(tmpl.replace(/__VERSION__/g, "0.0.0"));
  const optional = Object.keys(manifest.optionalDependencies || {});
  for (const pkg of Object.values(PLATFORM_PACKAGES)) {
    assert.ok(optional.includes(pkg), `${pkg} missing from optionalDependencies`);
  }
});
check("every detected platform package has a publishable manifest", () => {
  for (const key of Object.keys(PLATFORM_PACKAGES)) {
    const dir = path.join(REPO_ROOT, "npm", "perry-" + key);
    const tmpl = JSON.parse(
      fs.readFileSync(path.join(dir, "package.json.tmpl"), "utf8").replace(/__VERSION__/g, "0.0.0")
    );
    assert.strictEqual(tmpl.name, PLATFORM_PACKAGES[key]);
  }
});
check("release matrix builds the musl targets the fallback relies on", () => {
  const wf = fs.readFileSync(
    path.join(REPO_ROOT, ".github", "workflows", "release-packages.yml"),
    "utf8"
  );
  assert.ok(wf.includes("x86_64-unknown-linux-musl"), "x86_64 musl target missing");
  assert.ok(wf.includes("aarch64-unknown-linux-musl"), "aarch64 musl target missing");
});
check(`glibc legs use glibc ${GLIBC_BUILD_FLOOR} builders`, () => {
  // GTK4 still builds on the matrix's noble runner, but the compiler and core
  // archives must come from the old-sysroot image. Pin this coupling so a
  // workflow refactor cannot silently make the launcher floor a lie.
  const wf = fs.readFileSync(
    path.join(REPO_ROOT, ".github", "workflows", "release-packages.yml"),
    "utf8"
  );
  const entries = [...wf.matchAll(
    /-\s+os:\s*(\S+)\s*\n\s+target:\s*(\S+)\s*\n\s+artifact:\s*(\S+)\s*\n\s+old_glibc_image:\s*(\S+)/g
  )].map((m) => ({ os: m[1], target: m[2], image: m[4] }));
  const gnu = entries.filter((e) => e.target.endsWith("-unknown-linux-gnu"));
  assert.strictEqual(gnu.length, 2, `expected two linux-gnu legs, saw ${gnu.length}`);
  for (const leg of gnu) {
    assert.ok(
      leg.image.includes("debian:bullseye-slim@sha256:"),
      `${leg.target} uses ${leg.image}; expected the pinned Debian 11 builder image`
    );
  }
  assert.deepStrictEqual(
    gnu.map((leg) => leg.target).sort(),
    ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
  );
  assert.strictEqual(GLIBC_BUILD_FLOOR, "2.31");
  assert.ok(wf.includes("scripts/build_linux_glibc_2_31.sh"));
  assert.ok(wf.includes(`libc6 (>= ${GLIBC_BUILD_FLOOR})`));

  const dockerfile = fs.readFileSync(
    path.join(REPO_ROOT, "scripts", "linux-glibc-2.31.Dockerfile"),
    "utf8"
  );
  assert.ok(dockerfile.includes("debian:bullseye-slim@sha256:"));
  assert.ok(dockerfile.includes("llvm-toolchain-bullseye-22"));
});

console.log("");
if (failures) {
  console.log(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
