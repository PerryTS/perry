// A successful admission performs the same sequence of IEEE additions. A miss
// must preserve property access/coercion counts, including a zero-trip loop.
function total(rows: any, count: number, initial: any = 0): any {
    let sum = initial;
    for (let i = 0; i < count; i++) sum += rows[0].id;
    return sum;
}
function describe(value: any): string {
    return String(value) + ':' + String(Object.is(value, -0));
}
const rows: any = JSON.parse('[{"id":7}]');
console.log('bounds', total(rows, 0), total(rows, -2), total(rows, 0.5), total(rows, 3), total(rows, NaN));
console.log('zero-null', total(null, 0), total(undefined, -1));
console.log('coercions', total(rows, '3' as any), total(rows, 3, 'x'));
rows[0].id = 0.1;
console.log('fraction', describe(total(rows, 7)));
rows[0].id = -0;
console.log('negative-zero', describe(total(rows, 3, -0)), describe(total(rows, 0, -0)));
rows[0].id = Infinity;
console.log('infinity', describe(total(rows, 2)));
rows[0].id = NaN;
console.log('nan', describe(total(rows, 2)));
rows[0].id = 'a';
console.log('string', total(rows, 3));
rows[0].id = null;
console.log('null', total(rows, 3));
rows[0].id = true;
console.log('bool', total(rows, 3));

let indexReads = 0;
const indexed: any = [];
Object.defineProperty(indexed, '0', { get() { indexReads++; return { id: indexReads }; } });
console.log('index-getter', total(indexed, 0), indexReads, total(indexed, 4), indexReads);
let fieldReads = 0;
const record: any = {};
Object.defineProperty(record, 'id', { get() { fieldReads++; return fieldReads; } });
console.log('field-getter', total([record], 0), fieldReads, total([record], 4), fieldReads);
let conversions = 0;
const coercible: any = { valueOf() { conversions++; return conversions; } };
console.log('valueOf', total([{ id: coercible }], 4), conversions);
class WithGetter {
    get id(): number { fieldReads++; return fieldReads; }
}
fieldReads = 0;
console.log('class-getter', total([new WithGetter()], 4), fieldReads);

const inherited: any = Object.create({ id: 13 });
console.log('inherited-field', total([inherited], 3));
const missing: any = JSON.parse('[{"id":8}]');
delete missing[0].id;
console.log('missing', describe(total(missing, 3)));
const prototyped: any = [];
const proto: any = Object.create(Array.prototype);
proto[0] = { id: 11 };
Object.setPrototypeOf(prototyped, proto);
prototyped.length = 1;
console.log('hole-prototype', total(prototyped, 3));

// Cross the lazy threshold, then check pristine, exposed, and mutated backing
// reads. These are separate calls: no stale hoisted value may survive a call.
let text = '[';
for (let i = 0; i < 1200; i++) {
    if (i > 0) text += ',';
    text += '{"id":7,"name":"record-padding"}';
}
text += ']';
const lazy: any = JSON.parse(text);
console.log('lazy', total(lazy, 20));
const exposed: any = lazy[0];
exposed.id = 19;
console.log('exposed', total(lazy, 20));
lazy.push({ id: 1 });
lazy[0].id = 23;
console.log('materialized', total(lazy, 20), lazy.length);
Object.defineProperty(lazy[0], 'id', { get() { fieldReads++; return fieldReads; } });
fieldReads = 0;
console.log('lazy-getter', total(lazy, 4), fieldReads);
// Allocation pressure makes GC stress prove live moves around admitted loops.
let checksum = 0;
for (let i = 0; i < 80; i++) {
    const fresh: any = JSON.parse(text);
    checksum += total(fresh, 13);
    const held: any = fresh[0];
    const pressure: any = JSON.parse(text);
    checksum += pressure[0].id + held.id;
}
console.log('pressure', checksum);
