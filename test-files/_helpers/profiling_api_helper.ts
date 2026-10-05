export function sampleTiming() {
  const alias = performance;
  const first = performance.now();
  const last = performance.now();
  const event = { t: performance.timeOrigin + performance.now() };
  const epoch = typeof alias.now === "function" ? alias.timeOrigin + alias.now() : NaN;
  return {
    finite: Number.isFinite(first) && Number.isFinite(last) && Number.isFinite(event.t) && Number.isFinite(epoch),
    monotonic: last >= first && first >= 0,
    plausible: Math.abs(epoch - Date.now()) < 10000 && Math.abs(event.t - Date.now()) < 10000,
    identity: alias === globalThis.performance,
    origin: performance.timeOrigin,
    stable: alias.timeOrigin === performance.timeOrigin,
  };
}
