// Completed JSON graphs stay ordinary mutable values after deferred collection.
// The document exceeds the grace-period admission size. Run with default GC,
// seeded moving GC, and a small heap budget; do not assert a collection timing.
interface Row { id: number; text: string; nested: { n: number } }
const rows: Row[] = [];
for (let i = 0; i < 2_000; i++) {
    rows.push({ id: i, text: 'payload '.repeat(24), nested: { n: i * 3 } });
}
const arraySource = JSON.stringify(rows);
const source = '{"rows":' + arraySource + '}';
const kept: any[] = [];
let newest: any = null;
let sum = 0;

function makeChild(index: number): any {
    const value: any = JSON.parse(source);
    const child = value.rows[index];
    child.nested.n += 1;
    child.extra = index + 7;
    return child;
}

for (let i = 0; i < 48; i++) {
    const child: any = makeChild(i * 13);
    if (i % 4 === 0) kept.push(child);
    newest = child;
    const typed = JSON.parse<Row[]>(arraySource);
    sum += typed[i].id + child.nested.n;
    if (i % 8 === 0) {
        const again: any = JSON.parse(JSON.stringify(child));
        sum += again.extra;
    }
}
let keptSum = 0;
for (let i = 0; i < kept.length; i++) {
    kept[i].nested.n += i;
    keptSum += kept[i].nested.n + kept[i].extra;
}
let malformed = 0;
try { JSON.parse(source.slice(0, source.length - 1) + ',}'); }
catch { malformed = 1; }
console.log('document', source.length > 256 * 1024);
console.log('totals', sum, kept.length, keptSum, newest.id, newest.nested.n, malformed);
console.log('retained', JSON.stringify(kept[0]), JSON.stringify(kept[kept.length - 1]));
