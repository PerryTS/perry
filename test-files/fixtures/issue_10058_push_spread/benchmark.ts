let seed = 0x12345678;

function rnd(): number {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 4294967296;
}

function hashArray(a: number[]): number {
  let h = a.length;
  for (let i = 0; i < a.length; i++) h = (h * 31 + a[i]) % 1000000007;
  return h;
}

function setup(n: number): number[][] {
  const chunks: number[][] = [];
  for (let i = 0; i < n; i += 32) {
    const chunk: number[] = [];
    for (let j = i; j < Math.min(n, i + 32); j++) {
      chunk.push(Math.floor(rnd() * 1000000));
    }
    chunks.push(chunk);
  }
  return chunks;
}

function run(chunks: number[][]): number {
  const a: number[] = [];
  for (let i = 0; i < chunks.length; i++) a.push(...chunks[i]);
  return hashArray(a);
}

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
    const start = performance.now();
    const value = run(preparedInput);
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
    while (elapsed < 20) {
      seed = 0x12345678;
      const start = performance.now();
      const value = run(preparedInput);
      const duration = performance.now() - start;
      if (!(duration >= 0)) throw new Error("Invalid monotonic timer");
      elapsed += duration;
      count++;
      if (value !== checksum) throw new Error("CORRECTNESS: unstable checksum during sampling");
    }
    samples.push(elapsed / count);
    runs += count;
  }
  for (let i = 1; i < samples.length; i++) {
    const value = samples[i];
    let j = i - 1;
    while (j >= 0 && samples[j] > value) {
      samples[j + 1] = samples[j];
      j--;
    }
    samples[j + 1] = value;
  }
  console.log(JSON.stringify({
    name: "array-push-spread-chunk",
    category: "arrays",
    n,
    ms_per_run: samples[3],
    runs,
    checksum,
  }));
}

benchmarkMain();
