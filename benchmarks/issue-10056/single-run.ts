// Diagnostic: one invocation in a fresh process; no warmup or sampling.
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

const n = Number(process.argv[process.argv.length - 1]);
const input = setup(n);
const start = performance.now();
const checksum = run(input);
const ms_per_run = performance.now() - start;
console.error(JSON.stringify({n, ms_per_run, checksum}));
