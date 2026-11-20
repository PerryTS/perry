// Issue #921: throw new Error("...") inside an async function silently
// exited the process instead of surfacing the JS error when the awaited
// value came from a perry-stdlib `spawn()`-backed binding such as
// `fetch()`.
//
// Root cause: perry-stdlib's `spawn()` helper (used by fetch, ioredis,
// zlib, …) scheduled the tokio task without bumping
// EXT_BLOCKING_TASKS_INFLIGHT. The codegen-emitted event loop sees no
// active stdlib handles between the spawn and the eventual
// `queue_promise_resolution` and exits cleanly with code 0 — leaving
// the awaiter pending forever and PM2 / systemd interpreting the
// exit-0 as a crash.
//
// Pre-fix output: process exits silently with no "result:" line.
// Post-fix output: "result: caught:refresh failed: 401"

async function refreshToken(): Promise<string> {
  const r = await fetch("https://httpbin.org/status/401");
  if (!r.ok) {
    throw new Error("refresh failed: " + r.status);
  }
  return await r.text();
}

async function getAll(): Promise<string> {
  try {
    const tokens = await refreshToken();
    return "tokens:" + tokens;
  } catch (e: any) {
    return "caught:" + e.message;
  }
}

async function main(): Promise<void> {
  const result = await getAll();
  console.log("result:", result);
}

main();
