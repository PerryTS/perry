// Pattern: parse a 1 MB JSON API response, repeated. Models the
// hot path of any HTTP endpoint that consumes upstream JSON.

const N = 30; // ~1 MB × 30 = 30 MB of parsing

// Build a synthetic 1 MB payload deterministically so the input is
// identical across runs. Shape: array of 5000 user-record objects.
const records: unknown[] = [];
for (let i = 0; i < 5000; i++) {
    records.push({
        id: i,
        name: "user_" + i,
        email: "user_" + i + "@example.com",
        created_at: "2026-05-09T12:00:00Z",
        active: i % 3 !== 0,
        score: i * 1.5,
        tags: ["tag_" + (i % 10), "tag_" + (i % 7)],
    });
}
const payload = JSON.stringify({ users: records });

let total = 0;
for (let i = 0; i < N; i++) {
    const parsed = JSON.parse(payload) as { users: { id: number; score: number }[] };
    for (let j = 0; j < parsed.users.length; j++) {
        total += parsed.users[j].id;
    }
}

console.log("checksum:", total, "bytes:", payload.length);
