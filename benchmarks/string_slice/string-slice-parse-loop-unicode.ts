// @runtime {"name": "string-slice-parse-loop-unicode", "category": "strings", "verification": "checksum", "sources": [{"file": "crates/perry-runtime/src/string/slice_ops.rs", "function": "js_string_slice"}, {"file": "crates/perry-runtime/src/string/mod.rs", "function": "string_copy_range"}, {"file": "crates/perry-runtime/src/string/mod.rs", "function": "utf16_offset_to_byte_offset"}], "hypothesis": "Hypothesis: utf16_offset_to_byte_offset advances over a whole astral character when slice starts between its surrogate halves; js_string_slice then copies the remaining bytes while stamping end-start as UTF-16 length, creating a payload/header mismatch. Repeated full-suffix copies also give quadratic work.", "notes": "unicode variant. n counts repeated input tokens, not bytes; UTF-16 length and UTF-8 byte length differ for Unicode. String result hashes sample roughly 32 positions for long strings (at most 63 for short strings), plus length. Reads one UTF-16 unit then removes it, including individual emoji surrogate halves; no manual optimized parser. charCodeAt(0) inspects only the current leading code unit, so the checksum itself does not scan a growing prefix. For Unicode, an offset inside an emoji must retain its low surrogate; the source byte-offset helper instead rounds beyond the complete code point.", "asynchronous": false, "output_stderr": false, "fresh_input": false}
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

function setup(n: number): string { return "ä中😀Ö".repeat(n); }
function run(input: string): number {
  let s = input;
  let h = 0;
  while (s.length) {
    h = (h * 31 + s.charCodeAt(0)) % 1000000007;
    s = s.slice(1);
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
  console.log(JSON.stringify({name: "string-slice-parse-loop-unicode", category: "strings", n,
    ms_per_run: samples[3], runs, checksum}));
}
benchmarkMain();
