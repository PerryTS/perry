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
//   * the glibc targets are still built on the image GLIBC_BUILD_FLOOR assumes

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
  ["2.31 (Ubuntu 20.04)", glibcHost("x64", "2.31"), "linux-x64-musl", "glibc-too-old"],
  ["2.34 (RHEL 9 / AL2023)", glibcHost("x64", "2.34"), "linux-x64-musl", "glibc-too-old"],
  ["2.35 (Ubuntu 22.04)", glibcHost("x64", "2.35"), "linux-x64-musl", "glibc-too-old"],
  ["2.36 (Debian 12)", glibcHost("x64", "2.36"), "linux-x64-musl", "glibc-too-old"],
  ["2.39 (Ubuntu 24.04)", glibcHost("x64", "2.39"), "linux-x64", "native"],
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
check("linux-arm64 glibc 2.35 → linux-arm64-musl", () => {
  const got = detectPlatform(glibcHost("arm64", "2.35"));
  assert.strictEqual(got.candidates[0], "linux-arm64-musl");
  assert.strictEqual(got.reason, "glibc-too-old");
});
check("linux-arm64 glibc 2.39 → linux-arm64", () => {
  assert.strictEqual(detectPlatform(glibcHost("arm64", "2.39")).candidates[0], "linux-arm64");
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
  // "GLIBC_2.39 not found" error the user reported.
  const got = detectPlatform(glibcHost("x64", "2.35"));
  assert.deepStrictEqual(got.candidates, ["linux-x64-musl"]);
});

console.log("\nversion comparison");
check("compareVersions is numeric, not lexical", () => {
  assert.ok(compareVersions("2.35", "2.39") < 0);
  assert.ok(compareVersions("2.39", "2.39") === 0);
  assert.ok(compareVersions("2.40", "2.39") > 0);
  assert.ok(compareVersions("2.9", "2.39") < 0, "2.9 must sort below 2.39");
  assert.ok(compareVersions("3.0", "2.39") > 0);
  assert.ok(compareVersions("2.39.1", "2.39") > 0);
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
check("the musl packages the fallback targets have publishable manifests", () => {
  for (const key of ["linux-x64-musl", "linux-arm64-musl"]) {
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
check(`glibc legs still build on the image GLIBC_BUILD_FLOOR=${GLIBC_BUILD_FLOOR} assumes`, () => {
  // The floor is a property of the *builder image*, not of Perry. If the glibc
  // legs move to another runner, this fails and whoever moved them has to
  // revisit GLIBC_BUILD_FLOOR in bin/detect.cjs instead of silently shipping a
  // binary that the launcher's routing no longer describes.
  //   ubuntu-24.04 / ubuntu-24.04-arm → glibc 2.39
  const wf = fs.readFileSync(
    path.join(REPO_ROOT, ".github", "workflows", "release-packages.yml"),
    "utf8"
  );
  const entries = [
    ...wf.matchAll(/-\s+os:\s*(\S+)\s*\n\s+target:\s*(\S+)/g),
  ].map((m) => ({ os: m[1], target: m[2] }));
  const gnu = entries.filter((e) => e.target.endsWith("-unknown-linux-gnu"));
  assert.ok(gnu.length >= 2, `expected the linux-gnu legs in the matrix, saw ${gnu.length}`);
  const IMAGE_GLIBC = { "ubuntu-24.04": "2.39", "ubuntu-24.04-arm": "2.39" };
  for (const leg of gnu) {
    const glibc = IMAGE_GLIBC[leg.os];
    assert.ok(
      glibc,
      `${leg.target} now builds on '${leg.os}', an image this test doesn't know the ` +
        `glibc of. Add it to IMAGE_GLIBC and re-check GLIBC_BUILD_FLOOR in bin/detect.cjs.`
    );
    assert.strictEqual(
      glibc,
      GLIBC_BUILD_FLOOR,
      `${leg.target} builds on ${leg.os} (glibc ${glibc}) but GLIBC_BUILD_FLOOR is ` +
        `${GLIBC_BUILD_FLOOR} — update bin/detect.cjs.`
    );
  }
});

console.log("");
if (failures) {
  console.log(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
