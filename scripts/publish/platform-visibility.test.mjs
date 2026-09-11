import assert from "node:assert/strict";
import { test } from "node:test";
import { createServer } from "node:http";
import { once } from "node:events";
import { platformPackages, waitForPlatforms } from "./platform-visibility.mjs";

const a = { name: "@perryts/perry-a", version: "1", sha1: "a".repeat(40) };
const b = { ...a, name: "@perryts/perry-b" };
const wrapper = { ...a, name: "@perryts/perry" };
const quiet = { log() {} };
const visible = pkg => ({ ok: true, json: async () => ({
  time: { [pkg.version]: "2026-09-11" },
  versions: { [pkg.version]: { dist: { shasum: pkg.sha1 } } },
}) });

test("manifest refuses empty, duplicate, missing wrapper, and malformed sets", () => {
  assert.deepEqual(platformPackages({ packages: [a, wrapper] }), [a]);
  for (const packages of [[], [a], [a, a, wrapper], [a, b],
    [{ ...a, sha1: "bad" }, wrapper]]) {
    assert.throws(() => platformPackages({ packages }));
  }
});

test("visible packages pass only with both timestamp and matching immutable hash", async () => {
  const calls = [];
  await waitForPlatforms([a, b], { ...quiet, fetchImpl: async url => {
    calls.push(url);
    return visible(a);
  } });
  assert.equal(calls.length, 2);
  for (const body of [{}, { time: { 1: "published" } },
    { versions: { 1: { dist: { shasum: a.sha1 } } } },
    { time: { 1: "published" }, versions: { 1: { dist: { shasum: "wrong" } } } }]) {
    let clock = 0;
    await assert.rejects(waitForPlatforms([a], { ...quiet, budgetMs: 1,
      now: () => clock, pause: async ms => { clock += ms; },
      fetchImpl: async () => ({ ok: true, json: async () => body }),
    }), /budget expired/);
  }
});

test("polls round-robin under one shared deadline", async () => {
  let clock = 0;
  const calls = [];
  await assert.rejects(waitForPlatforms([a, b], { ...quiet,
    budgetMs: 25, intervalMs: 20, now: () => clock,
    pause: async ms => { clock += ms; },
    fetchImpl: async url => { calls.push(decodeURIComponent(url)); return { ok: false }; },
  }), /perry-a@1, @perryts\/perry-b@1/);
  assert.equal(clock, 25);
  assert.deepEqual(calls.map(url => url.split("/").at(-1)), ["perry-a", "perry-b", "perry-a", "perry-b"]);
});

test("one slow package does not hide a visible sibling", async () => {
  let clock = 0;
  const calls = [];
  await waitForPlatforms([a, b], { ...quiet, budgetMs: 50, intervalMs: 20,
    now: () => clock, pause: async ms => { clock += ms; },
    fetchImpl: async url => {
      calls.push(url);
      return url.endsWith("perry-a") && clock === 0 ? { ok: false } : visible(a);
    },
  });
  assert.equal(calls.length, 3);
});

test("network failures and invalid budgets cannot permit publication", async () => {
  for (const budgetMs of [0, -1, NaN, Infinity]) {
    await assert.rejects(waitForPlatforms([a], { ...quiet, budgetMs }));
  }
  let clock = 0;
  await assert.rejects(waitForPlatforms([a], { ...quiet, budgetMs: 1,
    now: () => clock, pause: async ms => { clock += ms; },
    fetchImpl: async () => { throw new Error("network"); },
  }), /budget expired/);
});

test("a real stalled HTTP body is aborted inside the shared budget", async () => {
  let requests = 0;
  const server = createServer((req, res) => {
    requests++;
    res.writeHead(200, { "content-type": "application/json" });
    res.write('{"time":'); // Deliberately never complete the body.
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const start = Date.now();
    await assert.rejects(waitForPlatforms([a], { ...quiet,
      budgetMs: 300, requestMs: 80, intervalMs: 10,
      fetchImpl: (_url, options) => fetch(`http://127.0.0.1:${server.address().port}`, options),
    }), /budget expired/);
    assert.ok(requests > 0, "the stalled server must actually be reached");
    assert.ok(Date.now() - start < 2000, "request must not escape the deadline");
  } finally {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
