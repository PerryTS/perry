// process.loadEnvFile(path?) loads a .env file (Node 20.12+).
import * as fs from "node:fs";
import { Buffer } from "node:buffer";

console.log("is function:", typeof process.loadEnvFile === "function");

const root = "/tmp/perry-process-load-env-file-parity";
try {
  fs.rmSync(root, { recursive: true, force: true });
} catch (_err) {
  // Ignore stale cleanup failures; mkdir/write below will surface real issues.
}
fs.mkdirSync(root, { recursive: true });

const envFile = root + "/custom.env";
const content = [
  "A=1",
  "B = two # comment",
  'C="three # not comment"',
  "D=unquoted value # comment",
  "export E=5",
  'MULTI="line1',
  'line2"',
  "BAD-NAME=bad",
  "HASH=abc#def",
  "DUP=first",
  "DUP=second",
].join("\n");
fs.writeFileSync(envFile, content);
fs.writeFileSync(root + "/.env", "DEFAULT_PATH=ok\n");

const keys = [
  "DEFAULT_PATH",
  "A",
  "B",
  "C",
  "D",
  "E",
  "MULTI",
  "BAD-NAME",
  "HASH",
  "DUP",
];

function clearEnv(): void {
  for (const key of keys) {
    delete process.env[key];
  }
}

function envSnapshot(): string {
  const out: Record<string, string> = {};
  for (const key of keys) {
    const value = process.env[key];
    if (value !== undefined) {
      out[key] = value;
    }
  }
  return JSON.stringify(out);
}

function errorCode(err: unknown): string {
  const anyErr = err as { code?: string };
  return typeof anyErr.code === "string" ? anyErr.code : "no-code";
}

function probe(label: string, fn: () => unknown): void {
  try {
    clearEnv();
    const result = fn();
    console.log("load:", label, "OK", String(result), envSnapshot());
  } catch (err) {
    console.log("load:", label, "THROW", (err as Error).name, errorCode(err));
  }
}

const before = process.cwd();
process.chdir(root);

probe("omitted", () => process.loadEnvFile());
probe("undefined", () => process.loadEnvFile(undefined));
probe("null", () => process.loadEnvFile(null as never));
probe("string", () => process.loadEnvFile(envFile));
probe("buffer", () => process.loadEnvFile(Buffer.from(envFile) as never));
probe("file-url", () => process.loadEnvFile(new URL("file://" + envFile) as never));
probe("http-url", () => process.loadEnvFile(new URL("http://example.com/x") as never));
probe("number", () => process.loadEnvFile(123 as never));
probe("object", () => process.loadEnvFile({} as never));
probe("array", () => process.loadEnvFile([] as never));
probe("boolean", () => process.loadEnvFile(true as never));
probe("symbol", () => process.loadEnvFile(Symbol("x") as never));

process.chdir(before);
try {
  fs.rmSync(root, { recursive: true, force: true });
} catch (_err) {
  // Best-effort cleanup only.
}
