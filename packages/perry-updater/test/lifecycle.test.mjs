import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const sourcePath = new URL("../src/index.ts", import.meta.url);
const nativeImport = /import \{([\s\S]*?)\} from "perry\/updater";/;

function armed(generation, restartCount = 0) {
  return JSON.stringify({
    prevExePath: "/app.prev",
    stagedAt: "2026-01-01T00:00:00.000Z",
    currentVersion: "1.0.0",
    targetVersion: "1.0.1",
    restartCount,
    state: "armed",
    generation,
  });
}

async function loadUpdater(overrides = {}) {
  let sentinel = overrides.sentinel ?? "";
  const timers = [];
  const clearedTimers = [];
  const exits = [];
  const exitHandlers = [];
  const clears = [];
  const writes = [];
  const native = {
    compareVersions: () => -1,
    verifyHash: () => 1,
    verifySignature: () => 1,
    verifySignatureV2: () => 1,
    computeFileSha256: () => "",
    writeSentinel: (_path, payload) => {
      writes.push(payload);
      sentinel = payload;
      return overrides.writeResult ?? 1;
    },
    readSentinel: () => sentinel,
    clearSentinel: () => {
      clears.push(sentinel);
      if ((overrides.clearResult ?? 1) === 1) sentinel = "";
      return overrides.clearResult ?? 1;
    },
    getExePath: () => "/app",
    getSentinelPath: () => "/sentinel",
    installUpdate: () => overrides.installResult ?? 1,
    performRollback: () => overrides.rollbackResult ?? 1,
    relaunch: () => overrides.relaunchResult ?? 123,
    ...overrides.native,
  };

  const previous = {
    native: globalThis.__perryUpdaterNative,
    process: globalThis.process,
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    fetch: globalThis.fetch,
  };
  globalThis.__perryUpdaterNative = native;
  Object.defineProperty(globalThis, "process", {
    configurable: true,
    value: {
      platform: "darwin",
      arch: "arm64",
      exit: (code) => exits.push(code),
      on: (event, callback) => {
        if (event === "exit") exitHandlers.push(callback);
      },
    },
  });
  globalThis.setTimeout = (callback, ms) => {
    const timer = { callback, ms };
    timers.push(timer);
    return timer;
  };
  globalThis.clearTimeout = (timer) => clearedTimers.push(timer);

  const source = await readFile(sourcePath, "utf8");
  const transformed = source.replace(nativeImport, (_match, bindings) => {
    const destructuring = bindings.replace(
      /(\w+)\s+as\s+(\w+)/g,
      "$1: $2",
    );
    return `const {${destructuring}} = globalThis.__perryUpdaterNative;`;
  });
  const modulePath = `${repoRoot}.updater-test-${Date.now()}-${Math.random()}.ts`;
  await writeFile(modulePath, transformed);
  const updater = await import(pathToFileURL(modulePath).href);

  return {
    updater,
    timers,
    clearedTimers,
    exits,
    exitHandlers,
    clears,
    writes,
    native,
    get sentinel() {
      return sentinel;
    },
    set sentinel(value) {
      sentinel = value;
    },
    async restore() {
      await rm(modulePath, { force: true });
      globalThis.__perryUpdaterNative = previous.native;
      Object.defineProperty(globalThis, "process", { configurable: true, value: previous.process });
      globalThis.setTimeout = previous.setTimeout;
      globalThis.clearTimeout = previous.clearTimeout;
      globalThis.fetch = previous.fetch;
    },
  };
}

test("rollback failure preserves the sentinel and does not exit", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1", 1), rollbackResult: 0 });
  try {
    await assert.rejects(ctx.updater.initUpdater({ crashLoopThreshold: 2 }), /rollback failed/);
    assert.equal(ctx.sentinel, armed("generation-1", 1));
    assert.deepEqual(ctx.exits, []);
  } finally {
    await ctx.restore();
  }
});

test("a clear failure after rollback is reported and does not exit", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1", 1), clearResult: 0 });
  try {
    await assert.rejects(ctx.updater.initUpdater({ crashLoopThreshold: 2 }), /failed to clear sentinel/);
    assert.equal(ctx.sentinel, armed("generation-1", 1));
    assert.deepEqual(ctx.exits, []);
  } finally {
    await ctx.restore();
  }
});

test("a stale health timer cannot clear a newer generation", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1") });
  try {
    await ctx.updater.initUpdater({ crashLoopThreshold: 3, healthCheckMs: 10 });
    const oldTimer = ctx.timers.at(-1);
    ctx.sentinel = armed("generation-2");
    oldTimer.callback();
    assert.equal(ctx.sentinel, armed("generation-2"));
  } finally {
    await ctx.restore();
  }
});

test("a stale exit hook cannot clear a newer generation", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1") });
  try {
    await ctx.updater.initUpdater({ crashLoopThreshold: 3 });
    const oldExitHook = ctx.exitHandlers.at(-1);
    ctx.sentinel = armed("generation-2");
    oldExitHook();
    assert.equal(ctx.sentinel, armed("generation-2"));
  } finally {
    await ctx.restore();
  }
});

test("a new install invalidates callbacks from the previous generation", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1"), relaunchResult: -1 });
  try {
    await ctx.updater.initUpdater({ crashLoopThreshold: 3 });
    const oldTimer = ctx.timers.at(-1);
    globalThis.fetch = async () => ({
      ok: true,
      json: async () => ({
        schemaVersion: 1,
        version: "1.0.1",
        notes: "",
        platforms: {
          "darwin-aarch64": { url: "https://example.test/app", sha256: "x", signature: "x", size: 1 },
        },
      }),
    });
    const update = await ctx.updater.checkForUpdate({
      manifestUrl: "https://example.test/manifest.json",
      publicKey: "key",
      currentVersion: "1.0.0",
    });
    await assert.rejects(update.installAndRelaunch(), /relaunch failed/);
    const installedSentinel = ctx.sentinel;
    oldTimer.callback();
    assert.equal(ctx.sentinel, installedSentinel);
    assert.ok(ctx.clearedTimers.includes(oldTimer));
  } finally {
    await ctx.restore();
  }
});

test("markHealthy is idempotent and cancels its active timer", async () => {
  const ctx = await loadUpdater({ sentinel: armed("generation-1") });
  try {
    await ctx.updater.initUpdater({ crashLoopThreshold: 3 });
    const timer = ctx.timers.at(-1);
    ctx.updater.markHealthy();
    ctx.updater.markHealthy();
    assert.equal(ctx.sentinel, "");
    assert.ok(ctx.clearedTimers.includes(timer));
  } finally {
    await ctx.restore();
  }
});
