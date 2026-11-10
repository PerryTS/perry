// AbortSignal static helpers + lifecycle (#2582) and navigator object (#2923)

// ── AbortController + signal lifecycle ──
const ac = new AbortController();
console.log("initial", ac.signal.aborted, ac.signal.reason);
let seen = 0;
ac.signal.addEventListener("abort", () => {
  seen++;
});
ac.abort("boom");
console.log("after", ac.signal.aborted, ac.signal.reason, seen);

try {
  ac.signal.throwIfAborted();
  console.log("throwIfAborted did not throw");
} catch (err) {
  console.log("throwIfAborted threw", err === ac.signal.reason);
}

// ── AbortSignal.abort(reason) ──
const s1 = AbortSignal.abort("x");
console.log("abort static", s1.aborted, s1.reason);

// not-aborted signal: throwIfAborted is a no-op
const ac2 = new AbortController();
ac2.signal.throwIfAborted();
console.log("throwIfAborted noop ok", ac2.signal.aborted);

// ── AbortSignal.timeout(ms) ──
const t = AbortSignal.timeout(5);
console.log("timeout initial", t.aborted, typeof t.reason);

// ── AbortSignal.any([signals]) ──
const a = new AbortController();
const any = AbortSignal.any([a.signal]);
console.log("any initial", any.aborted);
a.abort("y");
console.log("any after", any.aborted, any.reason);

// pre-aborted input propagates immediately
const pre = AbortSignal.abort("z");
const any2 = AbortSignal.any([pre]);
console.log("any pre-aborted", any2.aborted, any2.reason);

// ── navigator (#2923) ──
const n = globalThis.navigator;
console.log("navigator type:", typeof n);
console.log("userAgent prefix:", typeof n.userAgent, n.userAgent.startsWith("Node.js/"));
console.log("language type:", typeof n.language);
console.log("languages array:", Array.isArray(n.languages));
console.log("hardwareConcurrency type:", typeof n.hardwareConcurrency, n.hardwareConcurrency > 0);
console.log("platform type:", typeof n.platform);
