#!/usr/bin/env bun
// Build the unmodified OpenCode source tree with its release-time constants.
import { parseArgs } from "node:util";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";

export async function opencodeDefines(checkout: string, os: string, libc: string) {
  const root = join(resolve(checkout), "packages/opencode");
  const { version } = await Bun.file(join(root, "package.json")).json();
  if (typeof version !== "string") throw new Error("OpenCode package.json has no version");
  // Upstream honors MODELS_DEV_API_JSON for a pinned/offline snapshot and
  // otherwise fetches OPENCODE_MODELS_URL/api.json, exactly as its build does.
  const cwd = process.cwd();
  let modelsData: string;
  try {
    ({ modelsData } = await import(pathToFileURL(join(root, "script/generate.ts")).href));
  } finally {
    process.chdir(cwd); // upstream generate.ts changes cwd
  }
  JSON.parse(modelsData); // reject a failed/non-JSON snapshot before compiling
  const defines: Record<string, string> = {
    FFF_LIBC: JSON.stringify(libc === "musl" ? "musl" : "gnu"),
    OPENCODE_VERSION: JSON.stringify(version),
    OPENCODE_MODELS_DEV: modelsData,
    // Perry compiles these source entries into native Worker entry functions.
    OTUI_TREE_SITTER_WORKER_PATH: JSON.stringify(Bun.resolveSync("@opentui/core/parser.worker", root)),
    OPENCODE_WORKER_PATH: JSON.stringify(join(root, "src/cli/tui/worker.ts")),
    OPENCODE_CHANNEL: JSON.stringify("latest"),
    OPENCODE_LIBC: os === "linux" ? JSON.stringify(libc) : "undefined",
  };
  if (os === "linux") defines["process.env.OPENTUI_LIBC"] = JSON.stringify(libc);
  return { root, version, defines };
}

if (import.meta.main) {
  const { values, positionals } = parseArgs({
    args: Bun.argv.slice(2), allowPositionals: true,
    options: {
      perry: { type: "string", default: "perry" },
      output: { type: "string", default: "opencode-native" },
      os: { type: "string", default: process.platform },
      libc: { type: "string", default: "glibc" },
      target: { type: "string" },
      "prepare-only": { type: "boolean", default: false },
    },
  });
  if (positionals.length !== 1 || !["glibc", "musl"].includes(values.libc)) {
    throw new Error("Usage: bun scripts/build_opencode.ts <checkout> [--perry binary] [--output binary] [--os linux --libc glibc|musl] [--target target] [--prepare-only]");
  }
  const output = resolve(values.output);
  const { root, version, defines } = await opencodeDefines(positionals[0], values.os, values.libc);
  const configFile = Bun.file(join(root, "perry.json"));
  const config = await configFile.exists() ? await configFile.json() : {};
  const merged = { ...config.define, ...defines };
  // Reusing a Linux checkout for another target must remove the Linux-only flag.
  if (values.os !== "linux") delete merged["process.env.OPENTUI_LIBC"];
  await Bun.write(configFile, JSON.stringify({ ...config, define: merged }, null, 2) + "\n");
  console.log(`Prepared OpenCode ${version}, channel latest (${Object.keys(defines).length} defines)`);
  if (!values["prepare-only"]) {
    const command = [values.perry, "compile", "src/index.ts", "--platform", "bun", "--output", output];
    if (values.target) command.push("--target", values.target);
    const child = Bun.spawn(command, { cwd: root, stdin: "inherit", stdout: "inherit", stderr: "inherit" });
    process.exit(await child.exited);
  }
}
