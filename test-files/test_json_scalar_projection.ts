// Own scalar reads must preserve lazy-record identity, mutation and getters.
function id(rows: any, index: number): any { return rows[index].id; }
function active(rows: any, index: number): any { return rows[index].active; }
function empty(rows: any, index: number): any { return rows[index].empty; }
function nested(rows: any, index: number): any { return rows[index].nested; }
function name(rows: any, index: number): any { return rows[index].name; }
function absent(rows: any, index: number): any { return rows[index].absent; }

let source = '[';
for (let i = 0; i < 128; i++) {
    if (i > 0) source += ',';
    source += '{"id":' + String(i) + ',"active":true,"empty":null,"name":"record","nested":{"v":7}}';
}
source += ']';
function make(): any { return JSON.parse(source); }

const saved: any[] = [];
let sum = 0;
for (let pass = 0; pass < 40; pass++) {
    const rows: any = make();
    for (let i = 0; i < 128; i++) sum += id(rows, i);
    sum += active(rows, 127) ? 1 : 0;
    sum += empty(rows, 64) === null ? 1 : 0;
    saved.push(rows);
}
console.log('scan', sum, saved.length);
let retained = 0;
for (let pass = 0; pass < saved.length; pass++) {
    const rows: any = saved[pass];
    retained += id(rows, 127);
    retained += id(rows, 0);
    retained += id(rows, 53);
}
console.log('retained', retained);

// Repeat and rescan scalars before exposing records, including both zero signs.
const memo: any = make();
let memoSum = 0;
for (let pass = 0; pass < 3; pass++) {
    for (let i = 0; i < 128; i++) memoSum += id(memo, i);
    memoSum += id(memo, 0) + id(memo, 127) + id(memo, 53);
}
console.log('memo', memoSum, JSON.stringify(memo) === source);
console.log('memo-switch', active(memo, 7), id(memo, 7), empty(memo, 7));
const memoRecord: any = memo[8];
memoRecord.id = 808;
console.log('memo-mutation', id(memo, 8), memo[8] === memoRecord);
memo.push({id: 128});
console.log('memo-growth', id(memo, 128), id(memo, 8), memo[8] === memoRecord);
memo.length = 12;
Object.defineProperty(memo, '9', {get() { return {id: 909}; }, configurable: true});
console.log('memo-materialized-getter', id(memo, 9), memo.length);

// Materialized backing arrays grow through forwarding headers. Read after
// every push so the first access after a capacity change must resolve it.
const grown: any = make();
const grownAlias: any = grown;
const grownFirst: any = grown[0];
let growthSum = 0;
for (let i = 128; i < 1024; i++) {
    grown.push({id: i});
    growthSum += id(grownAlias, i);
}
console.log('materialized-growth', growthSum, grownAlias[0] === grownFirst,
    id(grownAlias, 1023), grownAlias.length);
delete grown[300];
Object.defineProperty(Array.prototype, '300', {
    get() { return {id: 30303}; }, configurable: true,
});
console.log('materialized-prototype-hole', id(grownAlias, 300), id(grownAlias, 301));
delete (Array.prototype as any)[300];
grown.length = 8;
console.log('materialized-shrink', id(grownAlias, 7), grownAlias.length, grownAlias[8]);

const rows: any = make();
const record: any = rows[4];
record.id = 401;
console.log('cached', id(rows, 4), rows[4] === record);
let calls = 0;
Object.defineProperty(record, 'id', {get() { calls++; return 402; }});
console.log('getter', id(rows, 4), calls);
console.log('children', nested(rows, 5) === nested(rows, 5), name(rows, 6));
console.log('missing', absent(rows, 7));
Object.defineProperty(Object.prototype, 'absent', {
    get() { calls++; return 88; }, configurable: true,
});
console.log('inherited', absent(rows, 8), calls);
delete (Object.prototype as any).absent;

const defined: any = make();
const returned: any = Object.defineProperty(defined, '0', {value: {id: 99}});
console.log('define-data', id(defined, 0), returned === defined);
const accessor: any = make();
Object.defineProperty(accessor, '0', {get() { calls++; return {id: 98}; }});
console.log('define-getter', id(accessor, 0), calls);
const deleted: any = make();
delete deleted[0];
Object.defineProperty(Array.prototype, '0', {
    get() { return {id: 95}; }, configurable: true,
});
console.log('delete-index', id(deleted, 0));
delete (Array.prototype as any)[0];

let evaluations = 0;
function index(): number { evaluations++; return 1; }
function base(): any { evaluations++; return rows; }
console.log('evaluation', base()[index()].id, evaluations);
const stringIndex: any = '3';
console.log('string-index', rows[stringIndex].id);
let indexCoercions = 0;
const objectIndex: any = {toString() { indexCoercions++; return '3'; }};
console.log('object-index', rows[objectIndex].id, indexCoercions);
const key: any = {toString() { evaluations++; return '2'; }};
const coercion: any = make();
Object.defineProperty(coercion, key, {value: {id: 77}});
console.log('coercion', id(coercion, 2), evaluations);

// Key conversion precedes descriptor getters/validation, and occurs once.
const order: string[] = [];
const orderedKey: any = {toString() { order.push('key'); return '0'; }};
const orderedDescriptor: any = {
    get value() { order.push('descriptor'); throw new Error('stop'); },
};
try { Object.defineProperty(coercion, orderedKey, orderedDescriptor); } catch (error) {}
console.log('key-order', order.join(','));
order.length = 0;
try { Object.defineProperty(coercion, orderedKey, 1 as any); } catch (error) {}
console.log('invalid-descriptor-order', order.join(','));

for (const record of [
    '{"id":1,"id":2}',
    '{"id":1,"\\u0069d":3}',
    '{"\\u0069d":4,"id":5}',
    '{"id":{"x":1},"id":6}',
    '{"id":-0}',
    '{"id":1e400}',
    '{"id":9007199254740993}',
]) {
    let input = '[';
    for (let i = 0; i < 128; i++) input += (i > 0 ? ',' : '') + record;
    const values: any = JSON.parse(input + ']');
    const value: any = id(values, 57);
    console.log('value', value, Object.is(value, -0));
}
