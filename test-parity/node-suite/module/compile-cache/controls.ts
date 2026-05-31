import * as Module from "node:module";
import { existsSync, rmSync } from "node:fs";

const cacheBase = `${process.cwd()}/.perry-module-cache-fixture`;
rmSync(cacheBase, { recursive: true, force: true });

const status = Module.constants.compileCacheStatus;
console.log("status enum:", status.FAILED, status.ENABLED, status.ALREADY_ENABLED, status.DISABLED);
console.log("before undefined:", Module.getCompileCacheDir() === undefined);

process.env.NODE_DISABLE_COMPILE_CACHE = "1";
const disabled = Module.enableCompileCache(cacheBase);
console.log("disabled status:", disabled.status === status.DISABLED);
console.log("disabled message:", disabled.message);
console.log("disabled dir:", Module.getCompileCacheDir() === undefined);
console.log("disabled creates base:", existsSync(cacheBase) === false);
delete process.env.NODE_DISABLE_COMPILE_CACHE;

const first = Module.enableCompileCache(cacheBase);
console.log("first status:", first.status === status.ENABLED);
console.log("first directory base:", first.directory === cacheBase);

const current = Module.getCompileCacheDir();
console.log("current string:", typeof current === "string");
console.log("current under base:", typeof current === "string" && current.startsWith(`${cacheBase}/`));
console.log("current versioned:", typeof current === "string" && current.length > cacheBase.length);

const second = Module.enableCompileCache(`${cacheBase}-ignored`);
console.log("second status:", second.status === status.ALREADY_ENABLED);
console.log("second directory current:", second.directory === current);
console.log("flush undefined:", Module.flushCompileCache() === undefined);

for (const [label, value] of [
  ["number", 1],
  ["boolean", true],
  ["symbol", Symbol("cache")],
] as const) {
  try {
    Module.enableCompileCache(value as never);
    console.log(`invalid ${label}: ok`);
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    console.log(`invalid ${label}:`, e.name, e.code);
  }
}

rmSync(cacheBase, { recursive: true, force: true });
