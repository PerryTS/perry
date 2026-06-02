// Pattern: stringify a 1 MB-shaped object, repeated. Models any
// HTTP response builder, log-line emitter, or queue producer.

const N = 30;

interface Rec {
    id: number;
    name: string;
    email: string;
    created_at: string;
    active: boolean;
    score: number;
    tags: string[];
}

const records: Rec[] = [];
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

let totalLen = 0;
for (let i = 0; i < N; i++) {
    const s = JSON.stringify({ users: records });
    totalLen += s.length;
}

console.log("checksum:", totalLen);
