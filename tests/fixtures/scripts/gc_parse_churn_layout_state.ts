// #7647 end-to-end gate fixture: the "tape=0 + from-space-scan
// parse-then-churn" check for the whole layout-state family
// (#7630 / #7633 / #7635 / #7643 / #7644).
//
// Driven by scripts/gc_parse_churn_layout_gate.sh, never run bare in CI.
//
// SHAPE. `RECORDS` JSON records, each with two pointer-bearing string
// fields. Matches #7643's own reproduction numbers (4,000 records, 2 fields
// -> "dangling=8000 owners=4000" on the sabotaged runtime). Field values are
// unique per record and per field (so any cross-record or cross-field
// corruption is individually detectable) and long enough (>5 chars) to be
// real heap `StringHeader`s rather than inline SSO values. The JSON text is
// a top-level array sized inside the [1 KB, 16 MB) window `js_json_parse`'s
// Auto mode routes through the LAZY TAPE (`json_tape`, default) -- the exact
// shape #7635 found makes a naive parse-then-churn-then-read probe vacuous,
// because the tape defers materializing each record until its first read,
// and a probe that reads only after the churn churns an empty heap.
//
// Each record's two strings are reachable ONLY through that record's own
// field slots -- nothing here keeps a second, parallel reference to them.
// That is #7635's second (discriminating) probe shape: a child reachable
// solely via the slot the collector's layout state is supposed to trace
// strands with nowhere else to be found alive from.
//
// WHAT DEFEATS THE LAZINESS. The gate script runs this fixture with
// `PERRY_JSON_TAPE=0`, which forces every `JSON.parse` call through the
// direct parser regardless of blob size or shape -- the whole record cohort
// is a real materialized tree the moment `JSON.parse` returns, before the
// churn loop below allocates a single byte. This file does not itself prove
// that happened; the gate script does, by cross-checking the from-space
// scan's own `objects=` census against `RECORDS` (a lazily-parsed cohort
// would leave only a handful of tape/lazy-array objects live, not
// thousands of real record objects) -- see that script's header for why
// that is a genuine "was this eager" signal and not merely trust in the
// env var.
//
// WHAT THIS FILE ASSERTS ON ITS OWN. Byte-exact field values after the
// churn: `MISMATCHES` must be `0`. A build that strands a child does not
// necessarily crash (evacuation COPIES rather than zeroes, so a stale
// pointer can still read old, now-reclaimed-and-possibly-reused bytes) --
// this file's own comparison is the belt to the from-space scan's
// suspenders, and either one failing is real.

const RECORDS = 4000;

function fieldA(i: number): string {
    return "layout-state-alpha-" + String(i);
}
function fieldB(i: number): string {
    return "layout-state-bravo-" + String(i);
}

function buildBlob(n: number): string {
    const parts: string[] = ["["];
    for (let i = 0; i < n; i++) {
        if (i > 0) parts.push(",");
        parts.push('{"a":"' + fieldA(i) + '","b":"' + fieldB(i) + '"}');
    }
    parts.push("]");
    return parts.join("");
}

// Parse behind a function boundary so the (potentially large) JSON source
// text is a function-local, not a module-level global. Module-level
// `const`s are registered GC roots for the whole process lifetime (see
// CLAUDE.md's NaN-Boxing section); a function-local's shadow-stack root is
// popped when the function returns, so the source text is ordinary garbage
// by the time the churn loop below runs -- matching how a real "parse once,
// work with the tree" caller is shaped, and keeping the subject of this
// probe to exactly the record cohort the churn should threaten.
function parseRecords(n: number): { a: string; b: string }[] {
    const blob = buildBlob(n);
    console.log("BLOB_BYTES", blob.length);
    return JSON.parse(blob) as { a: string; b: string }[];
}

const records = parseRecords(RECORDS);
console.log("PARSED_LENGTH", records.length);

// Heap churn: enough discarded allocation, across enough separate rounds, to
// force several nursery collections (each round's `garbage` array goes dead
// as soon as the next round starts). Nothing here references `records` --
// see the shape note above.
const CHURN_ROUNDS = 600;
const CHURN_PER_ROUND = 400;
let churnTouch = 0;
for (let r = 0; r < CHURN_ROUNDS; r++) {
    const garbage: { i: number; s: string }[] = [];
    for (let i = 0; i < CHURN_PER_ROUND; i++) {
        garbage.push({ i: i, s: "churn-filler-" + r + "-" + i });
    }
    churnTouch += garbage.length;
}
console.log("CHURN_TOUCH", churnTouch);

let mismatches = 0;
for (let i = 0; i < RECORDS; i++) {
    const rec = records[i];
    if (rec.a !== fieldA(i) || rec.b !== fieldB(i)) {
        mismatches++;
        if (mismatches <= 5) {
            console.log("MISMATCH", i, rec.a, rec.b);
        }
    }
}
console.log("MISMATCHES", mismatches);

if (mismatches > 0) {
    throw new Error(
        "gc_parse_churn_layout_state: " +
            mismatches +
            " of " +
            RECORDS +
            " records corrupted after churn"
    );
}

console.log("PARSE_CHURN_LAYOUT_GATE_OK");
