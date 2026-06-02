// Pattern: Map of N entries: insert, lookup, iterate. Models any
// in-memory cache, ID lookup, request-id correlation map.

const N = 500_000;

// Insert N entries.
const m = new Map<string, number>();
for (let i = 0; i < N; i++) {
    m.set("key_" + i, i * 2);
}

// Lookup each key.
let lookupSum = 0;
for (let i = 0; i < N; i++) {
    const v = m.get("key_" + i);
    if (v !== undefined) lookupSum += v;
}

// Iterate.
let iterSum = 0;
for (const v of m.values()) iterSum += v;

// Random misses (different shape).
let missCount = 0;
for (let i = 0; i < 10_000; i++) {
    if (!m.has("missing_" + i)) missCount++;
}

console.log("checksum:", lookupSum, iterSum, missCount, "size:", m.size);
