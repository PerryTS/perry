// @runtime {"name": "binary-dataview-set", "category": "binary-node", "verification": "checksum", "sources": [{"file": "crates/perry-runtime/src/buffer/dataview.rs", "function": "js_data_view_set"}], "hypothesis": "Each accessor validates/coerces offsets and dispatches the requested numeric kind and byte order.", "notes": "n uint32 accesses. The setter case immediately reads each written value to validate writes; getter-only control separates that cost.", "asynchronous": false, "output_stderr": true, "fresh_input": true}
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

function setup(n: number): {view: DataView, values: number[]} {
  const view = new DataView(new ArrayBuffer(n * 4));
  const values = numbers(n);
  for (let i = 0; i < n; i++) view.setUint32(i * 4, values[i], true);
  return {view, values};
}
function run(input: {view: DataView, values: number[]}): number {
  let h = 0;
  for (let i = 0; i < input.values.length; i++) {
    input.view.setUint32(i * 4, input.values[i] + 1, true);
    h = (h + input.view.getUint32(i * 4, true)) % 1000000007;
  }
  return h;
}

// Size is the final argument: both native Perry and Node expose it reliably.
const n = Number(process.argv[process.argv.length - 1]);
if (!(n > 0)) throw new Error("Expected a positive size argument");
function benchmarkMain(): void {
  seed = 0x12345678;

  let checksum = 0;
  let seen = false;
  let warmMs = 0;
  let warmRuns = 0;
  while (warmMs < 200 || warmRuns < 5) {
    seed = 0x12345678;
    const input = setup(n);
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
      const input = setup(n);
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
  console.error(JSON.stringify({name: "binary-dataview-set", category: "binary-node", n,
    ms_per_run: samples[3], runs, checksum}));
}
benchmarkMain();
