// Census follow-up (gc_runtime_root_holders ffi/ext coverage):
// REQUEST_REGISTRY's RequestRecord.signal (crates/perry-stdlib/src/fetch/
// mod.rs) holds a NaN-boxed AbortSignal OBJECT. For `new Request(url)` with
// no caller-supplied signal, signal_or_default() builds a fresh
// AbortController and this registry slot is the signal's ONLY holder — no
// scanner visited it before scan_request_registry_roots was wired into
// fetch/gc.rs, so a full collection freed the object and a copying minor
// left the slot pointing into from-space. `request.clone()` copies the same
// stale bits into a second entry. The reads below cross an allocating loop
// so the seeded schedule can collect between construction and the `.signal`
// reads. Run under the seeded instrument env (see the sibling
// test_gap_gc_fetch_method_value_cache_rooting.ts header); a green run
// needs the retired_set line and nonzero copying_minors.

function churn(): number {
  let keep: Array<{ i: number; s: string }> = [];
  for (let i = 0; i < 30000; i++) {
    keep.push({ i, s: "pad-" + i });
    if (keep.length > 64) keep = [];
  }
  return keep.length;
}

const req = new Request("https://example.test/x", { method: "POST", body: "b" });
churn();
// Pre-fix: req.signal returns the registry's stale bits; property reads on
// it dereference the dead/moved object.
console.log("signal:", typeof req.signal, req.signal.aborted);
const clone = req.clone();
churn();
console.log("clone signal:", typeof clone.signal, clone.signal.aborted);
console.log("same object:", req.signal === req.signal);

const ctl = new AbortController();
const req2 = new Request("https://example.test/y", { signal: ctl.signal });
churn();
ctl.abort();
console.log("user signal:", req2.signal.aborted);
