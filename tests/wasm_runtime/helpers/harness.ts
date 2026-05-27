import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");

export function runWasm(source: string): string {
  const dir = mkdtempSync(join(tmpdir(), "perry-wasm-runtime-"));
  const input = join(dir, "input.ts");
  const html = join(dir, "out.html");
  writeFileSync(input, source);

  execFileSync("cargo", ["run", "-q", "-p", "perry", "--", input, "--target", "wasm", "-o", html], {
    cwd: root,
    stdio: "pipe",
  });

  const page = readFileSync(html, "utf8");
  const scripts = [...page.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]).join("\n");
  const bootable = scripts.replace(/^(bootPerryWasm\()/m, "await $1");

  const runner = `
    globalThis.window = globalThis;
    const stub = { id: '', appendChild() {}, getElementById() { return null; }, style: {}, textContent: '' };
    globalThis.document = { createElement() { return { ...stub }; }, getElementById() { return null; }, title: '', head: stub, body: stub };
    const atob = (x) => Buffer.from(x, "base64").toString("binary");
    const cryptoMod = require("node:crypto");
    if (!globalThis.crypto) globalThis.crypto = { randomUUID: () => cryptoMod.randomUUID(), getRandomValues: (a) => cryptoMod.getRandomValues(a) };
    (async () => { ${bootable} })().catch((e) => { console.error(e && e.stack || e); process.exit(1); });
  `;

  return execFileSync(process.execPath, ["-e", runner], { encoding: "utf8" }).trimEnd();
}

export function assertOutput(actual: string, expected: string): void {
  if (actual !== expected.trim()) {
    throw new Error(`output mismatch\nexpected:\n${expected}\nactual:\n${actual}\n`);
  }
}
