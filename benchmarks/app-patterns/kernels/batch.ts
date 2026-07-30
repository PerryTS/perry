// Pattern: batch-process a stream of records — map to reshaped objects,
// sort by a derived key, group/reduce into summary rows. Models the
// object/property-heavy middle of a request handler, an ETL step, or a
// report builder: allocation-heavy, property-access-dominated, and every
// record *escapes* its producing scope (pushed, returned, or passed to a
// callback).
//
// This is the workload #7034 measured `Ptr<Shape>` promotion against and
// found **zero** promoted locals. It is committed here so that
// `perry <this file> --opt-report` reproduces that finding in one command
// and names the rule that denied each candidate. Keep the escape shapes
// below intact — they are the point of the fixture, not incidental style.

const N = 40_000;

class Row {
    id: number;
    bucket: string;
    weight: number;
    score: number;

    constructor(id: number, bucket: string, weight: number) {
        this.id = id;
        this.bucket = bucket;
        this.weight = weight;
        this.score = 0;
    }

    rescore(factor: number): number {
        this.score = this.weight * factor + this.id % 7;
        return this.score;
    }
}

interface Shaped {
    key: string;
    value: number;
    tag: string;
}

interface Summary {
    bucket: string;
    count: number;
    total: number;
    peak: number;
}

const BUCKETS = ["alpha", "beta", "gamma", "delta"];

// Producer: every `new Row(...)` escapes into the array (rule 2 — the
// local is used as a call argument).
function buildRows(n: number): Row[] {
    const rows: Row[] = [];
    for (let i = 0; i < n; i++) {
        const row = new Row(i, BUCKETS[i % 4], (i % 97) * 0.5);
        rows.push(row);
    }
    return rows;
}

// Reshape: the `.map(x => ({...}))` idiom — the record is produced in a
// closure body and returned, never bound to a local at all.
function shape(rows: Row[]): Shaped[] {
    return rows.map((r) => ({
        key: r.bucket + ":" + r.id,
        value: r.rescore(1.5),
        tag: r.id % 2 === 0 ? "even" : "odd",
    }));
}

// Sort by a derived key — the comparator is a closure body invoked once
// per comparison, with no loop of its own.
function ranked(items: Shaped[]): Shaped[] {
    const copy = items.slice();
    copy.sort((a, b) => {
        if (a.value !== b.value) {
            return b.value - a.value;
        }
        return a.key < b.key ? -1 : a.key > b.key ? 1 : 0;
    });
    return copy;
}

// Reduce into per-bucket summaries — the accumulator object is returned
// from the reducer (rule 2 — return disqualifies).
function summarize(rows: Row[]): Summary[] {
    const byBucket: Summary[] = [];
    for (let b = 0; b < BUCKETS.length; b++) {
        const s: Summary = {
            bucket: BUCKETS[b],
            count: 0,
            total: 0,
            peak: 0,
        };
        byBucket.push(s);
    }
    rows.reduce((acc: Summary[], r: Row) => {
        const idx = BUCKETS.indexOf(r.bucket);
        const slot = acc[idx];
        slot.count = slot.count + 1;
        slot.total = slot.total + r.score;
        if (r.score > slot.peak) {
            slot.peak = r.score;
        }
        return acc;
    }, byBucket);
    return byBucket;
}

// Fold the summaries into one totals record and hand it back — the single
// most common record-producing idiom in real TypeScript (#7034 §4), and
// another rule-2 denial: the accumulator is returned.
function totalsRow(summaries: Summary[]): Row {
    const acc = new Row(0, "acc", 0);
    for (let i = 0; i < summaries.length; i++) {
        acc.weight = acc.weight + summaries[i].total;
        acc.score = acc.score + summaries[i].peak;
    }
    return acc;
}

const rows = buildRows(N);
const shaped = shape(rows);
const top = ranked(shaped);
const summaries = summarize(rows);

let checksum = 0;
for (let i = 0; i < 16; i++) {
    checksum = checksum + top[i].value;
}
for (let i = 0; i < summaries.length; i++) {
    checksum = checksum + summaries[i].count + summaries[i].peak;
}
const totals = totalsRow(summaries);
checksum = checksum + totals.weight + totals.score;

console.log("checksum: " + checksum.toFixed(4));
