#!/usr/bin/env node
// Resolve the WIT tool from the same platform package used by the compiler.
const { spawn } = require("child_process");
const { PLATFORM_PACKAGES, detectPlatform, readHost } = require("./detect.cjs");
const detected = detectPlatform(readHost());
const name = process.platform === "win32" ? "perry-wit.exe" : "perry-wit";
let binary;
for (const key of detected.candidates) {
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) continue;
  try {
    binary = require.resolve(`${pkg}/bin/${name}`);
    break;
  } catch (_) {
    // Try the detector's next compatible package, never another architecture.
  }
}
if (!binary) {
  console.error("[perry-wit] No WIT tool is installed for this platform. " +
    "Prebuilt component tools are available for macOS ARM64, Linux x64 glibc, " +
    "and Windows x64. Install matching @perryts/perry platform packages or " +
    "build perry-wit from source.");
  process.exit(1);
}
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT"]) {
  process.on(signal, () => { try { child.kill(signal); } catch (_) {} });
}
child.on("close", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code == null ? 0 : code);
});
child.on("error", (error) => {
  console.error(`[perry-wit] Failed to start ${binary}: ${error.message}`);
  process.exit(1);
});
