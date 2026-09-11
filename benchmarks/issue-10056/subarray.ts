// @runtime {"name": "binary-uint8array-subarray", "category": "binary-node", "verification": "checksum", "sources": [{"file": "crates/perry-runtime/src/buffer/access.rs", "function": "js_buffer_slice"}, {"file": "crates/perry-runtime/src/buffer/view.rs", "function": "remove_entries_for_dead_buffer"}, {"file": "crates/perry-runtime/src/object/buffer_dispatch.rs", "function": "dispatch_buffer_method"}], "hypothesis": "Buffer-backed Uint8Array.subarray copies each complete suffix before registering alias metadata, causing quadratic copied bytes across n suffixes; per-view GC cleanup can also scan the full view registry.", "notes": "n suffix views over n source bytes; result length/first byte consumption detects incorrect offsets without scanning each view. This Uint8Array path uses BufferHeader storage and js_buffer_slice; its copied storage plus alias metadata differs from a zero-copy view. Read-only checksums do not prove write aliasing.", "asynchronous": false, "output_stderr": true, "fresh_input": false}
// Standalone file. Shared helpers/driver are inlined by common.py.

let seed = 0x12345678;
function rnd(): number {
  seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
  return (seed >>> 0) / 4294967296;
}
function numbers(n: number): number[] {
  const a: number[] = [];
  for (let i = 0; i < n; i++) a.push(Math.floor(rnd() * 1000000));
  return a;
}
function hashArray(a: number[]): number {
  let h = a.length;
  for (let i = 0; i < a.length; i++) h = (h * 31 + a[i]) % 1000000007;
  return h;
}
// Bounded checksum work avoids making string slicing/indexing part of every
// string benchmark's asymptotic cost. The workload itself consumes its result.
function hashString(s: string): number {
  let h = s.length;
  const step = Math.max(1, Math.floor(s.length / 32));
  for (let i = 0; i < s.length; i += step) h = (h * 31 + s.charCodeAt(i)) % 1000000007;
  return h;
}

function hashBytes(a: Uint8Array): number {
  let h = a.length;
  const step = Math.max(1, Math.floor(a.length / 32));
  for (let i = 0; i < a.length; i += step) h = (h * 31 + a[i]) % 1000000007;
  return h;
}

function setup(n: number): Uint8Array {
  const a = new Uint8Array(n);
  for (let i = 0; i < n; i++) a[i] = Math.floor(rnd() * 256);
  return a;
}
function run(input: Uint8Array): number {
  let h = 0;
  for (let i = 0; i < input.length; i++) {
    const view = input.subarray(i);
    h = (h + view.length + view[0]) % 1000000007;
  }
  return h;
}

// Size is the final argument: both native Perry and Node expose it reliably.
const n = Number(process.argv[process.argv.length - 1]);
if (!(n > 0)) throw new Error("Expected a positive size argument");
function benchmarkMain(): void {
  seed = 0x12345678;
  const preparedInput = setup(n);
  let checksum = 0;
  let seen = false;
  let warmMs = 0;
  let warmRuns = 0;
  while (warmMs < 200 || warmRuns < 5) {
    seed = 0x12345678;
    const input = preparedInput;
    const start = performance.now();
    const value = run(input);
    const elapsed = performance.now() - start;
    if (!(elapsed >= 0)) throw new Error("Invalid monotonic timer");
    warmMs += elapsed;
    warmRuns++;
    if (seen && value !== checksum) throw new Error("CORRECTNESS: unstable checksum during warmup");
    checksum = value;
    seen = true;
  }
  const samples: number[] = [];
  let runs = 0;
  for (let sample = 0; sample < 7; sample++) {
    let elapsed = 0;
    let count = 0;
    // Mutable workloads prepare fresh input BEFORE each timer; immutable
    // workloads reuse setup. Neither preparation nor validation is measured.
    while (elapsed < 20) {
      seed = 0x12345678;
      const input = preparedInput;
      const start = performance.now();
      const value = run(input);
      const duration = performance.now() - start;
      if (!(duration >= 0)) throw new Error("Invalid monotonic timer");
      elapsed += duration;
      count++;
      if (value !== checksum) throw new Error("CORRECTNESS: unstable checksum during sampling");
    }
    samples.push(elapsed / count);
    runs += count;
  }
  // Do not depend on Array.sort to compute the median of a sort benchmark.
  for (let i = 1; i < samples.length; i++) {
    const v = samples[i];
    let j = i - 1;
    while (j >= 0 && samples[j] > v) { samples[j + 1] = samples[j]; j--; }
    samples[j + 1] = v;
  }
  console.error(JSON.stringify({name: "binary-uint8array-subarray", category: "binary-node", n,
    ms_per_run: samples[3], runs, checksum}));
}
benchmarkMain();
