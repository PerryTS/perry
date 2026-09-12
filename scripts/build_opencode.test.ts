import { expect, test } from "bun:test";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { opencodeDefines } from "./build_opencode";

test("OpenCode harness supplies the eight Linux defines from upstream inputs", async () => {
  const checkout = await mkdtemp(join(tmpdir(), "perry-opencode-defines-"));
  try {
    const root = join(checkout, "packages/opencode");
    await mkdir(join(root, "script"), { recursive: true });
    const core = join(root, "node_modules/@opentui/core");
    await mkdir(core, { recursive: true });
    await writeFile(join(root, "package.json"), JSON.stringify({ version: "1.18.30" }));
    await writeFile(join(root, "script/generate.ts"), `export const modelsData = '{"provider":{"models":{}}}';`);
    await writeFile(join(core, "package.json"), JSON.stringify({ exports: { "./parser.worker": "./worker.js" } }));
    await writeFile(join(core, "worker.js"), "postMessage('ready');");
    const { defines } = await opencodeDefines(checkout, "linux", "musl");
    expect(Object.keys(defines)).toHaveLength(8);
    expect(JSON.parse(defines.OPENCODE_VERSION)).toBe("1.18.30");
    expect(JSON.parse(defines.OPENCODE_CHANNEL)).toBe("latest");
    expect(JSON.parse(defines.OPENCODE_MODELS_DEV)).toEqual({ provider: { models: {} } });
    expect(JSON.parse(defines.FFF_LIBC)).toBe("musl");
    expect(JSON.parse(defines.OPENCODE_LIBC)).toBe("musl");
    expect(JSON.parse(defines["process.env.OPENTUI_LIBC"])).toBe("musl");
    expect(isAbsolute(JSON.parse(defines.OPENCODE_WORKER_PATH))).toBe(true);
    expect(JSON.parse(defines.OTUI_TREE_SITTER_WORKER_PATH)).toBe(join(core, "worker.js"));
    const mac = await opencodeDefines(checkout, "darwin", "glibc");
    expect(mac.defines.OPENCODE_LIBC).toBe("undefined");
    expect(mac.defines["process.env.OPENTUI_LIBC"]).toBeUndefined();
    expect(JSON.parse(mac.defines.FFF_LIBC)).toBe("gnu");
    const configPath = join(root, "perry.json");
    await writeFile(configPath, JSON.stringify({ other: true, define: { CUSTOM: "42", ...defines } }));
    const child = Bun.spawn([
      process.execPath, fileURLToPath(new URL("./build_opencode.ts", import.meta.url)),
      checkout, "--prepare-only", "--os", "darwin",
    ], { stdout: "pipe", stderr: "pipe" });
    expect(await child.exited).toBe(0);
    const config = await Bun.file(configPath).json();
    expect(config.other).toBe(true);
    expect(config.define.CUSTOM).toBe("42");
    expect(config.define.OPENCODE_LIBC).toBe("undefined");
    expect(config.define["process.env.OPENTUI_LIBC"]).toBeUndefined();
  } finally {
    await rm(checkout, { recursive: true, force: true });
  }
});
