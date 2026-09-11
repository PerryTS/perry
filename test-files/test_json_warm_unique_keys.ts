// Warm prefixes may skip duplicate searches only while their unique key order
// matches. A mismatch, escaped duplicate or nested shape change must preserve
// last-value-wins semantics and the original property insertion order.
declare function gc(): void;

const keys = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
const warm = '{"a":0,"b":1,"c":2,"d":3,"e":4,"f":5,"g":6,"h":7}';
const records: string[] = [];
for (let prefix = 0; prefix <= 8; prefix++) {
    for (let kind = 0; kind < 4; kind++) {
        records.push(warm);
        const fields: string[] = [];
        for (let i = 0; i < prefix; i++) fields.push('"' + keys[i] + '":' + i);
        if (kind === 0) fields.push('"a":99');
        if (kind === 1) fields.push('"\\u0061":99');
        if (kind === 2) fields.push('"extra":9,"a":99');
        if (kind === 3) fields.push('"nested":{"a":1,"a":2},"a":99');
        records.push('{' + fields.join(',') + '}');
    }
}
records.push('{"a":0,"b":{"x":1,"y":2},"c":3}');
records.push('{"\\u0061":4,"b":{"other":8,"other":9},"c":5,"a":6}');
records.push('{"a":7,"b":8,"c":9}');
records.push('{"a":10,"b":11,"a":12}');
// Repeat beyond the lazy scan threshold as well as exercising object-root
// direct construction. Both wrappers must return the same complete tree.
const block = records.join(',');
const text = '[' + block + ',' + block + ',' + block + ']';
const retained: any[] = [];
for (let round = 0; round < 16; round++) {
    const parsed = JSON.parse(text);
    let total = 0;
    for (let i = 0; i < parsed.length; i++) total += parsed[i].a;
    const wrapped = JSON.parse('{"records":' + text + '}');
    if (JSON.stringify(parsed) !== JSON.stringify(wrapped.records)) {
        throw new Error('array and object roots disagree');
    }
    if (round % 4 === 0) retained.push(parsed);
    gc();
    console.log('round', round, total, JSON.stringify(wrapped.records));
}
gc();
for (const parsed of retained) console.log('retained', JSON.stringify(parsed));
