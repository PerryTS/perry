// Pattern: ISO timestamp format + parse round-trip. Models log
// emission, audit timestamps, schedule normalization.

const N = 100_000;

let total = 0;
for (let i = 0; i < N; i++) {
    // Vary the input so the JIT can't constant-fold.
    const d = new Date(1_700_000_000_000 + i * 1000);
    const iso = d.toISOString();
    const parsed = new Date(iso).getTime();
    total += parsed;
}

console.log("checksum:", total);
